//! CellX: inserting and deleting rows without writing every formula out as
//! text and parsing it back. IronCalc keeps formulas in a relative form, so a
//! formula that moves with its row is still right as it is; only references
//! that cross the inserted or deleted rows need new coordinates, and those are
//! worked out on the parsed formula. A formula that reads a deleted row (and
//! becomes #REF!) still goes the text way, which decides what it turns into.

use crate::constants::{LAST_COLUMN, LAST_ROW};
use crate::expressions::parser::stringify::{to_string_displaced, DisplaceData};
use crate::expressions::types::CellReferenceRC;
use crate::expressions::parser::static_analysis::run_static_analysis_on_node;
use crate::expressions::parser::Node;
use crate::model::Model;
use crate::types::Cell;

/// `count` rows (or columns) inserted before `row` on `sheet`, or deleted
/// from `row` on when `count` is negative. With `columns`, `row` is a column.
#[derive(Clone, Copy)]
pub(crate) struct RowShift {
    pub(crate) sheet: u32,
    pub(crate) row: i32,
    pub(crate) count: i32,
    pub(crate) columns: bool,
}

impl RowShift {
    /// The row a reference points at after the shift. Rows that are deleted
    /// do not get here (see `reads_deleted`).
    fn target(&self, sheet: u32, row: i32) -> i32 {
        if sheet == self.sheet && row >= self.row {
            row + self.count
        } else {
            row
        }
    }

    /// Whether `row` of `sheet` is one of the deleted rows.
    fn deleted(&self, sheet: u32, row: i32) -> bool {
        self.count < 0 && sheet == self.sheet && row >= self.row && row < self.row - self.count
    }

    /// Whether a formula that was at row (column) `old` reads a deleted one.
    fn reads_deleted(&self, node: &Node, old: i32) -> bool {
        if self.count >= 0 {
            return false;
        }
        let mut refs = Vec::new();
        crate::dependencies::collect(node, &mut refs);
        refs.iter().any(|r| {
            let (a, b, last) = if self.columns {
                (r.column1, r.column2, LAST_COLUMN)
            } else {
                (r.row1, r.row2, LAST_ROW)
            };
            let whole = a.absolute && b.absolute && a.value == 1 && b.value == last;
            let at = |c: crate::dependencies::Coord| if c.absolute { c.value } else { c.value + old };
            !whole && (self.deleted(r.sheet, at(a)) || self.deleted(r.sheet, at(b)))
        })
    }

    /// One row coordinate of a reference in a formula that was at row `old`
    /// and is now at row `new`.
    fn coordinate(&self, sheet: u32, absolute: bool, value: i32, old: i32, new: i32) -> i32 {
        if absolute {
            self.target(sheet, value)
        } else {
            self.target(sheet, value + old) - new
        }
    }
}

/// The formula with its row references displaced, or `None` when nothing in
/// it changes. Mirrors `stringify_reference` for `DisplaceData::Row`.
fn displace(node: &Node, ins: &RowShift, old: i32, new: i32) -> Option<Node> {
    use Node::*;
    let one = |n: &Node| displace(n, ins, old, new);
    let boxed = |n: &Node| one(n).map(Box::new);
    let pair = |l: &Node, r: &Node| -> Option<(Box<Node>, Box<Node>)> {
        let (a, b) = (one(l), one(r));
        if a.is_none() && b.is_none() {
            return None;
        }
        Some((
            Box::new(a.unwrap_or_else(|| l.clone())),
            Box::new(b.unwrap_or_else(|| r.clone())),
        ))
    };
    let list = |args: &[Node]| -> Option<Vec<Node>> {
        let changed: Vec<Option<Node>> = args.iter().map(one).collect();
        if changed.iter().all(Option::is_none) {
            return None;
        }
        Some(
            changed
                .into_iter()
                .zip(args)
                .map(|(c, a)| c.unwrap_or_else(|| a.clone()))
                .collect(),
        )
    };
    match node {
        ReferenceKind {
            sheet_name,
            sheet_index,
            absolute_row,
            absolute_column,
            row,
            column,
        } => {
            let (r, c) = if ins.columns {
                (*row, ins.coordinate(*sheet_index, *absolute_column, *column, old, new))
            } else {
                (ins.coordinate(*sheet_index, *absolute_row, *row, old, new), *column)
            };
            (r != *row || c != *column).then(|| ReferenceKind {
                sheet_name: sheet_name.clone(),
                sheet_index: *sheet_index,
                absolute_row: *absolute_row,
                absolute_column: *absolute_column,
                row: r,
                column: c,
            })
        }
        RangeKind {
            sheet_name,
            sheet_index,
            absolute_row1,
            absolute_column1,
            row1,
            column1,
            absolute_row2,
            absolute_column2,
            row2,
            column2,
        } => {
            // Whole columns (A:A) do not move with rows, whole rows (1:1) with columns.
            let (r1, r2, c1, c2) = if ins.columns {
                if *absolute_column1 && *absolute_column2 && *column1 == 1 && *column2 == LAST_COLUMN {
                    return None;
                }
                (
                    *row1,
                    *row2,
                    ins.coordinate(*sheet_index, *absolute_column1, *column1, old, new),
                    ins.coordinate(*sheet_index, *absolute_column2, *column2, old, new),
                )
            } else {
                if *absolute_row1 && *absolute_row2 && *row1 == 1 && *row2 == LAST_ROW {
                    return None;
                }
                (
                    ins.coordinate(*sheet_index, *absolute_row1, *row1, old, new),
                    ins.coordinate(*sheet_index, *absolute_row2, *row2, old, new),
                    *column1,
                    *column2,
                )
            };
            (r1 != *row1 || r2 != *row2 || c1 != *column1 || c2 != *column2).then(|| RangeKind {
                sheet_name: sheet_name.clone(),
                sheet_index: *sheet_index,
                absolute_row1: *absolute_row1,
                absolute_column1: *absolute_column1,
                row1: r1,
                column1: c1,
                absolute_row2: *absolute_row2,
                absolute_column2: *absolute_column2,
                row2: r2,
                column2: c2,
            })
        }
        // Wrong references are written out unchanged by `stringify`; relative
        // ones still keep pointing at the same place.
        WrongReferenceKind { .. } | WrongRangeKind { .. } => None,
        OpRangeKind { left, right } => pair(left, right).map(|(left, right)| OpRangeKind { left, right }),
        OpConcatenateKind { left, right } => {
            pair(left, right).map(|(left, right)| OpConcatenateKind { left, right })
        }
        OpSumKind { kind, left, right } => pair(left, right).map(|(left, right)| OpSumKind {
            kind: kind.clone(),
            left,
            right,
        }),
        OpProductKind { kind, left, right } => {
            pair(left, right).map(|(left, right)| OpProductKind {
                kind: kind.clone(),
                left,
                right,
            })
        }
        OpPowerKind { left, right } => pair(left, right).map(|(left, right)| OpPowerKind { left, right }),
        CompareKind { kind, left, right } => pair(left, right).map(|(left, right)| CompareKind {
            kind: kind.clone(),
            left,
            right,
        }),
        UnaryKind { kind, right } => boxed(right).map(|right| UnaryKind {
            kind: kind.clone(),
            right,
        }),
        FunctionKind { kind, args } => list(args).map(|args| FunctionKind {
            kind: kind.clone(),
            args,
        }),
        NamedFunctionKind { id, name, args } => list(args).map(|args| NamedFunctionKind {
            id: *id,
            name: name.clone(),
            args,
        }),
        LambdaDefKind { parameters, body } => boxed(body).map(|body| LambdaDefKind {
            parameters: parameters.clone(),
            body,
        }),
        LambdaCallKind { lambda, args } => {
            let l = one(lambda);
            let a = list(args);
            if l.is_none() && a.is_none() {
                return None;
            }
            Some(LambdaCallKind {
                lambda: Box::new(l.unwrap_or_else(|| (**lambda).clone())),
                args: a.unwrap_or_else(|| args.clone()),
            })
        }
        ImplicitIntersection { automatic, child } => boxed(child).map(|child| ImplicitIntersection {
            automatic: *automatic,
            child,
        }),
        SpillRangeOperator { child } => boxed(child).map(|child| SpillRangeOperator { child }),
        BooleanKind(_) | NumberKind(_) | StringKind(_) | ArrayKind(_) | DefinedNameKind(_)
        | TableNameKind(_) | NamedVariableKind { .. } | ErrorKind(_) | ParseErrorKind { .. }
        | EmptyArgKind => None,
    }
}

impl Model<'_> {
    /// CellX: puts whole cells in place (`None` removes the cell). See
    /// `UserModel::set_cells`.
    pub(crate) fn put_cells(
        &mut self,
        sheet: u32,
        cells: impl Iterator<Item = (i32, i32, Option<Cell>)>,
    ) -> Result<(), String> {
        let data = &mut self.workbook.worksheet_mut(sheet)?.sheet_data;
        for (row, column, cell) in cells {
            match cell {
                Some(cell) => {
                    data.set_cell(row, column, cell);
                }
                None => {
                    data.remove_cell(row, column);
                }
            }
        }
        Ok(())
    }

    /// The formulas outside rows `row..row + count` of `sheet` that read one of
    /// them, with their text: deleting the rows turns them into #REF!.
    pub(crate) fn formulas_reading_rows(&self, sheet: u32, row: i32, count: i32, columns: bool) -> Vec<(u32, i32, i32, String)> {
        let shift = RowShift { sheet, row, count: -count, columns };
        let mut out = Vec::new();
        for (s, worksheet) in self.workbook.worksheets.iter().enumerate() {
            let s = s as u32;
            if worksheet.shared_formulas.is_empty() {
                continue;
            }
            for (r, c, cell) in worksheet.sheet_data.cells() {
                let along = if columns { c } else { r };
                if shift.deleted(s, along) {
                    continue;
                }
                let Cell::CellFormula { f, .. } = &*cell else { continue };
                let Some((node, _)) = self.parsed_formulas[s as usize].get(*f as usize) else { continue };
                if shift.reads_deleted(node, along) {
                    if let Ok(Some(text)) = self.get_cell_formula(s, r, c) {
                        out.push((s, r, c, text));
                    }
                }
            }
        }
        out
    }

    /// Moves the cells of rows `row..` down by `count`. Plain cells and formulas
    /// move as they are; array formulas go through `move_cell`.
    pub(crate) fn move_rows_down(&mut self, sheet: u32, row: i32, count: i32) -> Result<(), String> {
        let mut all_rows = self.workbook.worksheet(sheet)?.sheet_data.rows();
        all_rows.reverse();
        for r in all_rows {
            if r < row {
                break;
            }
            let cells: Vec<(i32, Cell)> = self
                .workbook
                .worksheet(sheet)?
                .sheet_data
                .cells_in_row(r)
                .into_iter()
                .map(|(c, cell)| (c, cell.into_owned()))
                .collect();
            for (column, cell) in cells {
                match cell {
                    Cell::ArrayFormula { .. } | Cell::SpillCell { .. } => {
                        self.move_cell(sheet, r, column, r + count, column)?;
                    }
                    other => {
                        let data = &mut self.workbook.worksheet_mut(sheet)?.sheet_data;
                        data.remove_cell(r, column);
                        data.set_cell(r + count, column, other);
                    }
                }
            }
        }
        Ok(())
    }

    /// Moves the cells of rows `row + count..` up by `count`, dropping the
    /// rows in between. Plain cells and formulas move as they are; array
    /// formulas go through `move_cell`.
    pub(crate) fn move_rows_up(&mut self, sheet: u32, row: i32, count: i32) -> Result<(), String> {
        let all_rows = self.workbook.worksheet(sheet)?.sheet_data.rows();
        for r in all_rows {
            if r < row {
                continue;
            }
            if r < row + count {
                self.workbook.worksheet_mut(sheet)?.sheet_data.remove_row(r);
                continue;
            }
            let cells: Vec<(i32, Cell)> = self
                .workbook
                .worksheet(sheet)?
                .sheet_data
                .cells_in_row(r)
                .into_iter()
                .map(|(c, cell)| (c, cell.into_owned()))
                .collect();
            for (column, cell) in cells {
                match cell {
                    Cell::ArrayFormula { .. } | Cell::SpillCell { .. } => {
                        self.move_cell(sheet, r, column, r - count, column)?;
                    }
                    other => {
                        let data = &mut self.workbook.worksheet_mut(sheet)?.sheet_data;
                        data.remove_cell(r, column);
                        data.set_cell(r - count, column, other);
                    }
                }
            }
        }
        Ok(())
    }

    /// Moves the cells of columns `column..` right by `count`, row by row.
    pub(crate) fn move_columns_right(&mut self, sheet: u32, column: i32, count: i32) -> Result<(), String> {
        for r in self.workbook.worksheet(sheet)?.sheet_data.rows() {
            let mut cells: Vec<(i32, Cell)> = self
                .workbook
                .worksheet(sheet)?
                .sheet_data
                .cells_in_row(r)
                .into_iter()
                .filter(|(c, _)| *c >= column)
                .map(|(c, cell)| (c, cell.into_owned()))
                .collect();
            cells.sort_by(|a, b| b.0.cmp(&a.0));
            for (c, cell) in cells {
                self.move_one(sheet, r, c, r, c + count, cell)?;
            }
        }
        Ok(())
    }

    /// Moves the cells of columns `column + count..` left by `count`, dropping
    /// the columns in between, row by row.
    pub(crate) fn move_columns_left(&mut self, sheet: u32, column: i32, count: i32) -> Result<(), String> {
        for r in self.workbook.worksheet(sheet)?.sheet_data.rows() {
            let mut cells: Vec<(i32, Cell)> = self
                .workbook
                .worksheet(sheet)?
                .sheet_data
                .cells_in_row(r)
                .into_iter()
                .filter(|(c, _)| *c >= column)
                .map(|(c, cell)| (c, cell.into_owned()))
                .collect();
            cells.sort_by_key(|(c, _)| *c);
            for (c, cell) in cells {
                if c < column + count {
                    self.workbook.worksheet_mut(sheet)?.sheet_data.remove_cell(r, c);
                } else {
                    self.move_one(sheet, r, c, r, c - count, cell)?;
                }
            }
        }
        Ok(())
    }

    /// One cell moved as it is, or through `move_cell` if it is part of an array.
    fn move_one(&mut self, sheet: u32, r: i32, c: i32, to_r: i32, to_c: i32, cell: Cell) -> Result<(), String> {
        match cell {
            Cell::ArrayFormula { .. } | Cell::SpillCell { .. } => self.move_cell(sheet, r, c, to_r, to_c),
            other => {
                let data = &mut self.workbook.worksheet_mut(sheet)?.sheet_data;
                data.remove_cell(r, c);
                data.set_cell(to_r, to_c, other);
                Ok(())
            }
        }
    }

    /// Displaces every formula of the workbook for rows moved with
    /// `move_rows_down` or `move_rows_up`. Array formulas were moved as text
    /// and are displaced as text, the way `displace_cells` does it.
    pub(crate) fn displace_for_row_shift(&mut self, ins: RowShift) -> Result<(), String> {
        let text = if ins.columns {
            DisplaceData::Column {
                sheet: ins.sheet,
                column: ins.row,
                delta: ins.count,
            }
        } else {
            DisplaceData::Row {
                sheet: ins.sheet,
                row: ins.row,
                delta: ins.count,
            }
        };
        for s in 0..self.workbook.worksheets.len() as u32 {
            let worksheet = self.workbook.worksheet(s)?;
            if worksheet.shared_formulas.is_empty() {
                continue;
            }
            let found: Vec<(i32, i32, Option<i32>)> = worksheet
                .sheet_data
                .cells()
                .filter_map(|(r, c, cell)| match &*cell {
                    Cell::CellFormula { f, .. } => Some((r, c, Some(*f))),
                    Cell::ArrayFormula { .. } => Some((r, c, None)),
                    _ => None,
                })
                .collect();
            for (r, c, f) in found {
                let Some(f) = f else {
                    self.shift_cell_formula(s, r, c, &text)?;
                    continue;
                };
                // Where the formula was before the cells moved (along the shift).
                let now = if ins.columns { c } else { r };
                let moved_here = if ins.count > 0 { now >= ins.row + ins.count } else { now >= ins.row };
                let old = if s == ins.sheet && moved_here { now - ins.count } else { now };
                let Some(node) = self.parsed_formulas[s as usize].get(f as usize).map(|p| p.0.clone()) else {
                    continue;
                };
                if ins.reads_deleted(&node, old) {
                    // Written out from where it was, with the deleted rows
                    // turned into #REF!, and entered where it is now.
                    let (row, column) = if ins.columns { (r, old) } else { (old, c) };
                    let context = CellReferenceRC {
                        sheet: self.workbook.worksheet(s)?.get_name(),
                        row,
                        column,
                    };
                    let formula = to_string_displaced(&node, &context, &text, self.locale, self.language);
                    self.update_cell_with_formula(s, r, c, format!("={formula}"))?;
                    continue;
                }
                if let Some(moved) = displace(&node, &ins, old, now) {
                    // A range that grew has new dimensions.
                    let static_result = run_static_analysis_on_node(&moved);
                    let index = self.shared_formula_index(s, moved, static_result)?;
                    if let Some(Cell::CellFormula { f, .. }) =
                        self.workbook.worksheet_mut(s)?.sheet_data.cell_mut(r, c)
                    {
                        *f = index;
                    }
                }
            }
        }
        Ok(())
    }
}
