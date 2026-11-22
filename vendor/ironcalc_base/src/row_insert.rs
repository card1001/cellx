//! CellX: inserting and deleting rows without writing every formula out as
//! text and parsing it back. IronCalc keeps formulas in a relative form, so a
//! formula that moves with its row is still right as it is; only references
//! that cross the inserted or deleted rows need new coordinates, and those are
//! worked out on the parsed formula. A formula that reads a deleted row (and
//! becomes #REF!) still goes the text way, which decides what it turns into.

use crate::constants::LAST_ROW;
use crate::expressions::parser::stringify::{to_string_displaced, DisplaceData};
use crate::expressions::types::CellReferenceRC;
use crate::expressions::parser::static_analysis::run_static_analysis_on_node;
use crate::expressions::parser::Node;
use crate::model::Model;
use crate::types::Cell;

/// `count` rows inserted before `row` on `sheet` (or deleted from `row` on,
/// when `count` is negative).
#[derive(Clone, Copy)]
pub(crate) struct RowShift {
    pub(crate) sheet: u32,
    pub(crate) row: i32,
    pub(crate) count: i32,
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

    /// Whether a formula that was at row `old` reads a deleted row.
    fn reads_deleted(&self, node: &Node, old: i32) -> bool {
        if self.count >= 0 {
            return false;
        }
        let mut refs = Vec::new();
        crate::dependencies::collect(node, &mut refs);
        refs.iter().any(|r| {
            let whole = r.row1.absolute && r.row2.absolute && r.row1.value == 1 && r.row2.value == LAST_ROW;
            let at = |c: crate::dependencies::Coord| if c.absolute { c.value } else { c.value + old };
            !whole && (self.deleted(r.sheet, at(r.row1)) || self.deleted(r.sheet, at(r.row2)))
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
            let r = ins.coordinate(*sheet_index, *absolute_row, *row, old, new);
            (r != *row).then(|| ReferenceKind {
                sheet_name: sheet_name.clone(),
                sheet_index: *sheet_index,
                absolute_row: *absolute_row,
                absolute_column: *absolute_column,
                row: r,
                column: *column,
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
            // Whole columns (A:A) do not move.
            if *absolute_row1 && *absolute_row2 && *row1 == 1 && *row2 == LAST_ROW {
                return None;
            }
            let r1 = ins.coordinate(*sheet_index, *absolute_row1, *row1, old, new);
            let r2 = ins.coordinate(*sheet_index, *absolute_row2, *row2, old, new);
            (r1 != *row1 || r2 != *row2).then(|| RangeKind {
                sheet_name: sheet_name.clone(),
                sheet_index: *sheet_index,
                absolute_row1: *absolute_row1,
                absolute_column1: *absolute_column1,
                row1: r1,
                column1: *column1,
                absolute_row2: *absolute_row2,
                absolute_column2: *absolute_column2,
                row2: r2,
                column2: *column2,
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
    /// The formulas outside rows `row..row + count` of `sheet` that read one of
    /// them, with their text: deleting the rows turns them into #REF!.
    pub(crate) fn formulas_reading_rows(&self, sheet: u32, row: i32, count: i32) -> Vec<(u32, i32, i32, String)> {
        let shift = RowShift { sheet, row, count: -count };
        let mut out = Vec::new();
        for (s, worksheet) in self.workbook.worksheets.iter().enumerate() {
            let s = s as u32;
            if worksheet.shared_formulas.is_empty() {
                continue;
            }
            for (r, c, cell) in worksheet.sheet_data.cells() {
                if shift.deleted(s, r) {
                    continue;
                }
                let Cell::CellFormula { f, .. } = &*cell else { continue };
                let Some((node, _)) = self.parsed_formulas[s as usize].get(*f as usize) else { continue };
                if shift.reads_deleted(node, r) {
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

    /// Displaces every formula of the workbook for rows moved with
    /// `move_rows_down` or `move_rows_up`. Array formulas were moved as text
    /// and are displaced as text, the way `displace_cells` does it.
    pub(crate) fn displace_for_row_shift(&mut self, ins: RowShift) -> Result<(), String> {
        let text = DisplaceData::Row {
            sheet: ins.sheet,
            row: ins.row,
            delta: ins.count,
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
                // Where the formula was before the cells moved.
                let moved_here = if ins.count > 0 { r >= ins.row + ins.count } else { r >= ins.row };
                let old = if s == ins.sheet && moved_here { r - ins.count } else { r };
                let Some(node) = self.parsed_formulas[s as usize].get(f as usize).map(|p| p.0.clone()) else {
                    continue;
                };
                if ins.reads_deleted(&node, old) {
                    // Written out from where it was, with the deleted rows
                    // turned into #REF!, and entered where it is now.
                    let context = CellReferenceRC {
                        sheet: self.workbook.worksheet(s)?.get_name(),
                        row: old,
                        column: c,
                    };
                    let formula = to_string_displaced(&node, &context, &text, self.locale, self.language);
                    self.update_cell_with_formula(s, r, c, format!("={formula}"))?;
                    continue;
                }
                if let Some(moved) = displace(&node, &ins, old, r) {
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
