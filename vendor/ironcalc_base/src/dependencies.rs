//! CellX: which formulas read which cells, so that an edit re-evaluates only the
//! formulas that depend on it instead of the whole workbook.
//!
//! The index is built from the parsed formulas the first time it is needed and
//! kept up to date cell by cell as edits come in (`Model::evaluate_changed`).
//! Anything else that changes formulas wholesale (inserting rows, renaming a
//! sheet, defined names) must drop it with `Model::invalidate_dependencies`;
//! it is then rebuilt on the next incremental evaluation.
//!
//! A formula whose inputs cannot be read off its text (INDIRECT, OFFSET, a
//! defined name, a volatile function such as NOW) is volatile: it is evaluated
//! on every incremental pass. Array formulas that spill over more than one
//! cell are not handled: while a workbook has one, every evaluation is a full one.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::evaluation::{CellKey, FormulaCell, KeyBuild};
use crate::expressions::parser::Node;
use crate::functions::Function;
use crate::model::Model;
use crate::types::Cell;

/// Ranges wider than this many columns are kept in one list scanned in full;
/// narrower ones are filed under each of their columns.
const WIDE: i32 = 32;

/// One coordinate of a reference as written in a shared formula: an absolute
/// position, or an offset from the formula's own cell.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Coord {
    pub(crate) absolute: bool,
    pub(crate) value: i32,
}

impl Coord {
    fn at(self, base: i32) -> i32 {
        if self.absolute {
            self.value
        } else {
            self.value + base
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RelArea {
    pub(crate) sheet: u32,
    pub(crate) row1: Coord,
    pub(crate) column1: Coord,
    pub(crate) row2: Coord,
    pub(crate) column2: Coord,
}

/// A rectangle of cells a formula reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Area {
    pub(crate) sheet: u32,
    pub(crate) row1: i32,
    pub(crate) column1: i32,
    pub(crate) row2: i32,
    pub(crate) column2: i32,
}

impl Area {
    fn contains(&self, (sheet, row, column): CellKey) -> bool {
        self.sheet == sheet
            && (self.row1..=self.row2).contains(&row)
            && (self.column1..=self.column2).contains(&column)
    }
}

/// What a shared formula reads, relative to the cell it is in.
struct Shape {
    node: Arc<Node>,
    refs: Vec<RelArea>,
    volatile: bool,
}

struct Entry {
    reads: Vec<Area>,
    volatile: bool,
}

#[derive(Default)]
pub(crate) struct DepIndex {
    shapes: HashMap<(u32, i32), Shape>,
    formulas: HashMap<CellKey, Entry, KeyBuild>,
    points: HashMap<CellKey, Vec<CellKey>, KeyBuild>,
    columns: HashMap<(u32, i32), Vec<(i32, i32, CellKey)>>,
    wide: Vec<(Area, CellKey)>,
    pub(crate) volatile: HashSet<CellKey, KeyBuild>,
    pub(crate) arrays: HashSet<CellKey, KeyBuild>,
    /// The parsed formulas the index was built against: the address of the
    /// first one of each sheet (0 for a sheet that had none). New formulas are
    /// only ever appended, so a sheet whose first one changed was re-parsed
    /// behind the index's back and the index no longer holds.
    built_against: Vec<usize>,
}

fn coord(absolute: bool, value: i32) -> Coord {
    Coord { absolute, value }
}

/// Collects the references of a parsed formula. Returns false when the formula
/// reads cells that cannot be known from its text.
pub(crate) fn collect(node: &Node, out: &mut Vec<RelArea>) -> bool {
    use Node::*;
    match node {
        BooleanKind(_) | NumberKind(_) | StringKind(_) | ErrorKind(_) | EmptyArgKind => true,
        WrongReferenceKind { .. } | WrongRangeKind { .. } | ParseErrorKind { .. } => true,
        ArrayKind(_) | NamedVariableKind { .. } => true,
        ReferenceKind {
            sheet_index,
            absolute_row,
            absolute_column,
            row,
            column,
            ..
        } => {
            let r = coord(*absolute_row, *row);
            let c = coord(*absolute_column, *column);
            out.push(RelArea {
                sheet: *sheet_index,
                row1: r,
                column1: c,
                row2: r,
                column2: c,
            });
            true
        }
        RangeKind {
            sheet_index,
            absolute_row1,
            absolute_column1,
            row1,
            column1,
            absolute_row2,
            absolute_column2,
            row2,
            column2,
            ..
        } => {
            out.push(RelArea {
                sheet: *sheet_index,
                row1: coord(*absolute_row1, *row1),
                column1: coord(*absolute_column1, *column1),
                row2: coord(*absolute_row2, *row2),
                column2: coord(*absolute_column2, *column2),
            });
            true
        }
        FunctionKind { kind, args } => {
            let dynamic = matches!(
                kind,
                Function::Indirect
                    | Function::Offset
                    | Function::Now
                    | Function::Today
                    | Function::Rand
                    | Function::Randbetween
                    | Function::Randarray
                    | Function::Cell
                    | Function::Info
                    | Function::Hyperlink
                    | Function::Sheet
                    | Function::Sheets
                    // Skips hidden rows: hiding a row changes it without touching a cell.
                    | Function::Subtotal
            );
            let mut known = !dynamic;
            for a in args {
                known &= collect(a, out);
            }
            known
        }
        OpConcatenateKind { left, right }
        | OpSumKind { left, right, .. }
        | OpProductKind { left, right, .. }
        | OpPowerKind { left, right }
        | CompareKind { left, right, .. } => {
            let a = collect(left, out);
            collect(right, out) && a
        }
        UnaryKind { right, .. } => collect(right, out),
        ImplicitIntersection { child, .. } => collect(child, out),
        LambdaDefKind { body, .. } => collect(body, out),
        LambdaCallKind { lambda, args } => {
            let mut known = collect(lambda, out);
            for a in args {
                known &= collect(a, out);
            }
            known
        }
        // A range built at run time, a spill, a name or a table: unknown.
        OpRangeKind { .. }
        | SpillRangeOperator { .. }
        | DefinedNameKind(_)
        | TableNameKind(_)
        | NamedFunctionKind { .. } => {
            // Still look inside for whatever is static; the formula is volatile anyway.
            if let OpRangeKind { left, right } = node {
                collect(left, out);
                collect(right, out);
            }
            if let SpillRangeOperator { child } = node {
                collect(child, out);
            }
            if let NamedFunctionKind { args, .. } = node {
                for a in args {
                    collect(a, out);
                }
            }
            false
        }
    }
}

impl DepIndex {
    pub(crate) fn formula_count(&self) -> usize {
        self.formulas.len()
    }

    pub(crate) fn is_formula(&self, key: &CellKey) -> bool {
        self.formulas.contains_key(key)
    }

    fn markers(model: &Model) -> Vec<usize> {
        model
            .parsed_formulas
            .iter()
            .map(|p| p.first().map_or(0, |(n, _)| Arc::as_ptr(n) as *const u8 as usize))
            .collect()
    }

    /// Whether the parsed formulas are still the ones the index was built on.
    pub(crate) fn still_valid(&self, model: &Model) -> bool {
        let now = Self::markers(model);
        now.len() == self.built_against.len()
            && now.iter().zip(&self.built_against).all(|(n, b)| *b == 0 || n == b)
    }

    pub(crate) fn build(model: &Model) -> DepIndex {
        let mut index = DepIndex {
            built_against: Self::markers(model),
            ..DepIndex::default()
        };
        for (sheet, worksheet) in model.workbook.worksheets.iter().enumerate() {
            if worksheet.shared_formulas.is_empty() {
                continue;
            }
            for (row, column, cell) in worksheet.sheet_data.cells() {
                if let Some(formula) = FormulaCell::of(&cell) {
                    index.add(model, (sheet as u32, row, column), formula);
                }
            }
        }
        index
    }

    fn shape(&mut self, model: &Model, sheet: u32, f: i32) -> Option<&Shape> {
        let node = model.parsed_formulas.get(sheet as usize)?.get(f as usize)?.0.clone();
        let fresh = match self.shapes.get(&(sheet, f)) {
            Some(s) => !Arc::ptr_eq(&s.node, &node),
            None => true,
        };
        if fresh {
            let mut refs = Vec::new();
            let known = collect(&node, &mut refs);
            self.shapes.insert(
                (sheet, f),
                Shape {
                    node,
                    refs,
                    volatile: !known,
                },
            );
        }
        self.shapes.get(&(sheet, f))
    }

    fn add(&mut self, model: &Model, key: CellKey, formula: FormulaCell) {
        // A dynamic formula that returns one value (IronCalc keeps INDIRECT,
        // LET and the like as 1×1 dynamic arrays) is an ordinary formula here.
        if let Some((dynamic, size)) = formula.array {
            if !dynamic || size != (1, 1) {
                self.arrays.insert(key);
            }
        }
        let (sheet, row, column) = key;
        let Some(shape) = self.shape(model, sheet, formula.f) else {
            // Not expected: a formula without a parsed formula. Evaluate it always.
            self.volatile.insert(key);
            self.formulas.insert(
                key,
                Entry {
                    reads: Vec::new(),
                    volatile: true,
                },
            );
            return;
        };
        let volatile = shape.volatile;
        let reads: Vec<Area> = shape
            .refs
            .iter()
            .map(|r| {
                let (r1, r2) = (r.row1.at(row), r.row2.at(row));
                let (c1, c2) = (r.column1.at(column), r.column2.at(column));
                Area {
                    sheet: r.sheet,
                    row1: r1.min(r2),
                    column1: c1.min(c2),
                    row2: r1.max(r2),
                    column2: c1.max(c2),
                }
            })
            .collect();
        for a in &reads {
            if a.row1 == a.row2 && a.column1 == a.column2 {
                self.points.entry((a.sheet, a.row1, a.column1)).or_default().push(key);
            } else if a.column2 - a.column1 < WIDE {
                for c in a.column1..=a.column2 {
                    self.columns.entry((a.sheet, c)).or_default().push((a.row1, a.row2, key));
                }
            } else {
                self.wide.push((*a, key));
            }
        }
        if volatile {
            self.volatile.insert(key);
        }
        self.formulas.insert(key, Entry { reads, volatile });
    }

    fn remove(&mut self, key: CellKey) {
        self.arrays.remove(&key);
        let Some(entry) = self.formulas.remove(&key) else { return };
        if entry.volatile {
            self.volatile.remove(&key);
        }
        for a in &entry.reads {
            if a.row1 == a.row2 && a.column1 == a.column2 {
                let p = (a.sheet, a.row1, a.column1);
                if let Some(list) = self.points.get_mut(&p) {
                    list.retain(|k| *k != key);
                    if list.is_empty() {
                        self.points.remove(&p);
                    }
                }
            } else if a.column2 - a.column1 < WIDE {
                for c in a.column1..=a.column2 {
                    if let Some(list) = self.columns.get_mut(&(a.sheet, c)) {
                        list.retain(|(_, _, k)| *k != key);
                    }
                }
            } else {
                self.wide.retain(|(_, k)| *k != key);
            }
        }
    }

    /// Brings the entry of one cell in line with what the cell holds now.
    pub(crate) fn update_cell(&mut self, model: &Model, key: CellKey) {
        self.remove(key);
        let (sheet, row, column) = key;
        let formula = model
            .workbook
            .worksheets
            .get(sheet as usize)
            .and_then(|w| w.sheet_data.cell(row, column))
            .and_then(|cell: std::borrow::Cow<'_, Cell>| FormulaCell::of(&cell));
        if let Some(formula) = formula {
            self.add(model, key, formula);
        }
        // A sheet that got its first formula gets its marker.
        if let Some(b) = self.built_against.get_mut(sheet as usize) {
            if *b == 0 {
                *b = Self::markers(model).get(sheet as usize).copied().unwrap_or(0);
            }
        }
    }

    /// Calls `found` for every formula that reads `cell`.
    pub(crate) fn dependents(&self, cell: CellKey, mut found: impl FnMut(CellKey)) {
        if let Some(list) = self.points.get(&cell) {
            for k in list {
                found(*k);
            }
        }
        let (sheet, row, column) = cell;
        if let Some(list) = self.columns.get(&(sheet, column)) {
            for (r1, r2, k) in list {
                if (*r1..=*r2).contains(&row) {
                    found(*k);
                }
            }
        }
        for (area, k) in &self.wide {
            if area.contains(cell) {
                found(*k);
            }
        }
    }
}
