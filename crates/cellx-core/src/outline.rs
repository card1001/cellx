//! Outlines (행/열 그룹) and subtotals (부분합).
//!
//! Outline levels live beside IronCalc's rows and columns (IronCalc has none) and are
//! written to xlsx as `outlineLevel` attributes. Collapsing a group hides its rows.

use serde::{Deserialize, Serialize};

use crate::error::{CellxError, Result};
use crate::refs::{column_name, Axis};
use crate::value::Scalar;
use crate::workbook::{CellRange, RecalcStats, Workbook};

pub const MAX_LEVEL: u8 = 7;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct OutlineView {
    pub rows: Vec<(i32, u8)>,
    pub cols: Vec<(i32, u8)>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubtotalFn {
    Sum,
    Count,
    Average,
    Max,
    Min,
}

impl SubtotalFn {
    fn code(self) -> u8 {
        match self {
            SubtotalFn::Average => 1,
            SubtotalFn::Count => 3,
            SubtotalFn::Max => 4,
            SubtotalFn::Min => 5,
            SubtotalFn::Sum => 9,
        }
    }
    fn label(self) -> &'static str {
        match self {
            SubtotalFn::Sum => "요약",
            SubtotalFn::Count => "개수",
            SubtotalFn::Average => "평균",
            SubtotalFn::Max => "최대",
            SubtotalFn::Min => "최소",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SubtotalOptions {
    /// Column whose changes start a new group.
    pub group_col: i32,
    pub func: SubtotalFn,
    /// Columns that get a SUBTOTAL formula.
    pub value_cols: Vec<i32>,
    /// Remove earlier subtotals first (새로운 값으로 대치).
    pub replace: bool,
}

fn key_text(v: &Scalar) -> String {
    match v {
        Scalar::Text(t) => t.trim().to_lowercase(),
        Scalar::Number(n) => format!("{n}"),
        Scalar::Bool(b) => b.to_string(),
        Scalar::Error(e) => e.clone(),
        Scalar::Empty => String::new(),
    }
}

impl Workbook {
    fn levels(&self, axis: Axis) -> &std::collections::BTreeMap<(u32, i32), u8> {
        match axis {
            Axis::Row => &self.outline_rows,
            Axis::Column => &self.outline_cols,
        }
    }

    fn levels_mut(&mut self, axis: Axis) -> &mut std::collections::BTreeMap<(u32, i32), u8> {
        match axis {
            Axis::Row => &mut self.outline_rows,
            Axis::Column => &mut self.outline_cols,
        }
    }

    pub fn outline(&self, sheet: u32) -> OutlineView {
        let pick = |m: &std::collections::BTreeMap<(u32, i32), u8>| {
            m.iter().filter(|((s, _), l)| *s == sheet && **l > 0).map(|((_, i), l)| (*i, *l)).collect()
        };
        OutlineView { rows: pick(&self.outline_rows), cols: pick(&self.outline_cols) }
    }

    pub fn level(&self, sheet: u32, axis: Axis, index: i32) -> u8 {
        self.levels(axis).get(&(sheet, index)).copied().unwrap_or(0)
    }

    /// 그룹 (Shift+Alt+→): one level deeper.
    pub fn group(&mut self, sheet: u32, axis: Axis, from: i32, to: i32) -> Result<RecalcStats> {
        let (a, b) = (from.min(to), from.max(to));
        if (a..=b).any(|i| self.level(sheet, axis, i) >= MAX_LEVEL) {
            return Err(CellxError::Calc("윤곽은 8단계까지만 만들 수 있습니다".into()));
        }
        let start = self.begin();
        for i in a..=b {
            *self.levels_mut(axis).entry((sheet, i)).or_insert(0) += 1;
        }
        self.commit(start)
    }

    /// 그룹 해제 (Shift+Alt+←): one level up; rows that leave every group are shown.
    pub fn ungroup(&mut self, sheet: u32, axis: Axis, from: i32, to: i32) -> Result<RecalcStats> {
        let (a, b) = (from.min(to), from.max(to));
        let start = self.begin();
        for i in a..=b {
            let l = self.level(sheet, axis, i);
            if l == 0 {
                continue;
            }
            if l == 1 {
                self.levels_mut(axis).remove(&(sheet, i));
                self.set_axis_hidden(sheet, axis, i, i, false)?;
            } else {
                self.levels_mut(axis).insert((sheet, i), l - 1);
            }
        }
        self.commit(start)
    }

    pub fn clear_outline(&mut self, sheet: u32) -> Result<RecalcStats> {
        let start = self.begin();
        for axis in [Axis::Row, Axis::Column] {
            let idx: Vec<i32> = self.levels(axis).keys().filter(|(s, _)| *s == sheet).map(|(_, i)| *i).collect();
            for i in idx {
                self.levels_mut(axis).remove(&(sheet, i));
                self.set_axis_hidden(sheet, axis, i, i, false)?;
            }
        }
        self.commit(start)
    }

    fn set_axis_hidden(&mut self, sheet: u32, axis: Axis, a: i32, b: i32, hidden: bool) -> Result<()> {
        match axis {
            Axis::Row => self.um.set_rows_hidden(sheet, a, b, hidden)?,
            Axis::Column => self.um.set_columns_hidden(sheet, a, b, hidden)?,
        }
        Ok(())
    }

    fn is_hidden(&self, sheet: u32, axis: Axis, i: i32) -> bool {
        let ws = match self.model().workbook.worksheet(sheet) {
            Ok(ws) => ws,
            Err(_) => return false,
        };
        match axis {
            Axis::Row => ws.is_row_hidden(i).unwrap_or(false),
            Axis::Column => ws.is_column_hidden(i).unwrap_or(false),
        }
    }

    /// The group at `level` that contains `index`: the run of indexes with level ≥ `level`.
    fn group_run(&self, sheet: u32, axis: Axis, index: i32, level: u8) -> Option<(i32, i32)> {
        if self.level(sheet, axis, index) < level {
            return None;
        }
        let mut a = index;
        while a > 1 && self.level(sheet, axis, a - 1) >= level {
            a -= 1;
        }
        let mut b = index;
        while self.level(sheet, axis, b + 1) >= level {
            b += 1;
        }
        Some((a, b))
    }

    /// The +/- button of a group: hides it (`collapse`) or shows it again. Showing keeps
    /// inner groups that were collapsed, as Excel does.
    pub fn toggle_group(&mut self, sheet: u32, axis: Axis, index: i32, level: u8, collapse: bool) -> Result<RecalcStats> {
        let Some((a, b)) = self.group_run(sheet, axis, index, level) else { return Ok(RecalcStats::default()) };
        let start = self.begin();
        if collapse {
            self.set_axis_hidden(sheet, axis, a, b, true)?;
        } else {
            let mut i = a;
            while i <= b {
                let l = self.level(sheet, axis, i);
                // An inner group stays collapsed when its summary line (after it) is shown
                // but its own lines were hidden before.
                if l > level && self.is_hidden(sheet, axis, i) {
                    if let Some((ia, ib)) = self.group_run(sheet, axis, i, level + 1) {
                        let inner_collapsed = (ia..=ib).all(|k| self.is_hidden(sheet, axis, k));
                        let summary = ib + 1;
                        if inner_collapsed && summary <= b {
                            self.set_axis_hidden(sheet, axis, summary, summary, false)?;
                            i = ib + 2;
                            continue;
                        }
                    }
                }
                self.set_axis_hidden(sheet, axis, i, i, false)?;
                i += 1;
            }
        }
        self.commit(start)
    }

    /// The level buttons (1, 2, 3 …): shows lines up to `level`, hides deeper ones.
    pub fn show_outline_level(&mut self, sheet: u32, axis: Axis, level: u8) -> Result<RecalcStats> {
        let start = self.begin();
        let items: Vec<(i32, u8)> = self.levels(axis).iter().filter(|((s, _), _)| *s == sheet).map(|((_, i), l)| (*i, *l)).collect();
        for (i, l) in items {
            // Level n shows lines whose level is below n.
            let hide = l >= level;
            if self.is_hidden(sheet, axis, i) != hide {
                self.set_axis_hidden(sheet, axis, i, i, hide)?;
            }
        }
        self.commit(start)
    }

    pub(crate) fn shift_outline(&mut self, sheet: u32, axis: Axis, at: i32, count: i32) {
        let map = self.levels_mut(axis);
        let old = std::mem::take(map);
        for ((s, i), l) in old {
            if s != sheet {
                map.insert((s, i), l);
                continue;
            }
            let ni = if count > 0 {
                Some(if i >= at { i + count } else { i })
            } else if i >= at && i < at - count {
                None
            } else if i >= at - count {
                Some(i + count)
            } else {
                Some(i)
            };
            if let Some(ni) = ni {
                map.insert((s, ni), l);
            }
        }
    }

    pub(crate) fn outline_after_sheet_delete(&mut self, sheet: u32) {
        for axis in [Axis::Row, Axis::Column] {
            let map = self.levels_mut(axis);
            let old = std::mem::take(map);
            for ((s, i), l) in old {
                if s < sheet {
                    map.insert((s, i), l);
                } else if s > sheet {
                    map.insert((s - 1, i), l);
                }
            }
        }
    }

    // ------------------------------------------------------------ subtotals

    /// 부분합: inserts a SUBTOTAL row after each group of `group_col` and a grand total,
    /// and outlines the result (details at level 2, subtotals at level 1).
    pub fn subtotal(&mut self, sheet: u32, range: CellRange, opts: &SubtotalOptions) -> Result<RecalcStats> {
        let mut r = range.normalized();
        if opts.value_cols.is_empty() {
            return Err(CellxError::Calc("부분합 계산 항목을 하나 이상 고르세요".into()));
        }
        let start = self.begin();
        if opts.replace {
            r = self.remove_subtotal_rows(sheet, r)?;
        }
        let (last_row, _) = self.dimensions(sheet)?;
        r.r1 = r.r1.min(last_row.max(r.r0 + 1));
        let first = r.r0 + 1; // header row above
        if r.r1 < first {
            return Err(CellxError::Calc("머리글 아래에 데이터가 없습니다".into()));
        }
        // Group boundaries, bottom-up so inserted rows do not move the ones still to do.
        let mut groups: Vec<(i32, i32, String)> = Vec::new();
        let mut g0 = first;
        let mut key = key_text(&self.value(sheet, first, opts.group_col)?);
        let mut label = self.model().get_formatted_cell_value(sheet, first, opts.group_col)?;
        for row in (first + 1)..=(r.r1 + 1) {
            let k = if row > r.r1 { "\u{0}end".to_string() } else { key_text(&self.value(sheet, row, opts.group_col)?) };
            if k != key {
                groups.push((g0, row - 1, label.clone()));
                g0 = row;
                key = k;
                if row <= r.r1 {
                    label = self.model().get_formatted_cell_value(sheet, row, opts.group_col)?;
                }
            }
        }
        let code = opts.func.code();
        let mut offset = 0;
        let mut subtotal_rows = Vec::new();
        // Top-down with a running offset: each insertion pushes the next group down by one.
        for (a, b, label) in &groups {
            let (a, b) = (a + offset, b + offset);
            let at = b + 1;
            self.um.insert_rows(sheet, at, 1)?;
            self.shift_all_pub(sheet, Axis::Row, at, 1);
            self.write_input(sheet, at, opts.group_col, &format!("{label} {}", opts.func.label()), true)?;
            for c in &opts.value_cols {
                let col = column_name(*c);
                self.write_input(sheet, at, *c, &format!("=SUBTOTAL({code},{col}{a}:{col}{b})"), true)?;
            }
            for row in a..=b {
                self.outline_rows.insert((sheet, row), 2);
            }
            self.outline_rows.insert((sheet, at), 1);
            subtotal_rows.push(at);
            offset += 1;
        }
        let last = r.r1 + offset;
        let total = last + 1;
        self.um.insert_rows(sheet, total, 1)?;
        self.shift_all_pub(sheet, Axis::Row, total, 1);
        self.write_input(sheet, total, opts.group_col, &format!("총 {}", opts.func.label()), true)?;
        for c in &opts.value_cols {
            let col = column_name(*c);
            // SUBTOTAL skips the SUBTOTAL rows inside its range, so this is the grand total.
            self.write_input(sheet, total, *c, &format!("=SUBTOTAL({code},{col}{first}:{col}{last})"), true)?;
        }
        subtotal_rows.push(total);
        let c0 = r.c0;
        let c1 = r.c1;
        for row in subtotal_rows {
            let area = CellRange { r0: row, c0, r1: row, c1 }.area(sheet);
            self.um.update_range_style(&area, "font.b", "true")?;
        }
        self.commit(start)
    }

    /// 모두 제거: deletes the rows holding SUBTOTAL formulas and the outline.
    pub fn remove_subtotals(&mut self, sheet: u32, range: CellRange) -> Result<RecalcStats> {
        let start = self.begin();
        self.remove_subtotal_rows(sheet, range.normalized())?;
        self.commit(start)
    }

    fn remove_subtotal_rows(&mut self, sheet: u32, mut r: CellRange) -> Result<CellRange> {
        let (last_row, _) = self.dimensions(sheet)?;
        r.r1 = r.r1.max(last_row.min(r.r1 + 1000));
        let mut row = r.r1;
        while row > r.r0 {
            let has = (r.c0..=r.c1).any(|c| self.input(sheet, row, c).unwrap_or_default().to_ascii_uppercase().starts_with("=SUBTOTAL("));
            if has {
                self.um.delete_rows(sheet, row, 1)?;
                self.shift_all_pub(sheet, Axis::Row, row, -1);
                r.r1 -= 1;
            }
            row -= 1;
        }
        for i in r.r0..=r.r1 {
            if self.outline_rows.remove(&(sheet, i)).is_some() {
                self.um.set_rows_hidden(sheet, i, i, false)?;
            }
        }
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book() -> Workbook {
        let mut wb = Workbook::new().unwrap();
        let rows = [["지역", "금액"], ["부산", "50"], ["부산", "60"], ["서울", "100"], ["서울", "30"], ["서울", "20"]];
        let mut cells = Vec::new();
        for (i, r) in rows.iter().enumerate() {
            for (j, v) in r.iter().enumerate() {
                cells.push((i as i32 + 1, j as i32 + 1, v.to_string()));
            }
        }
        wb.set_inputs(0, &cells).unwrap();
        wb
    }

    fn hidden(wb: &Workbook, rows: std::ops::RangeInclusive<i32>) -> Vec<i32> {
        rows.filter(|r| wb.model().workbook.worksheet(0).unwrap().is_row_hidden(*r).unwrap()).collect()
    }

    #[test]
    fn groups_collapse_and_levels() {
        let mut wb = book();
        wb.group(0, Axis::Row, 2, 5).unwrap();
        wb.group(0, Axis::Row, 3, 4).unwrap();
        assert_eq!(wb.outline(0).rows, vec![(2, 1), (3, 2), (4, 2), (5, 1)]);
        wb.toggle_group(0, Axis::Row, 3, 2, true).unwrap();
        assert_eq!(hidden(&wb, 1..=6), vec![3, 4]);
        wb.toggle_group(0, Axis::Row, 2, 1, true).unwrap();
        assert_eq!(hidden(&wb, 1..=6), vec![2, 3, 4, 5]);
        // Expanding the outer group keeps the inner one collapsed.
        wb.toggle_group(0, Axis::Row, 2, 1, false).unwrap();
        assert_eq!(hidden(&wb, 1..=6), vec![3, 4]);
        wb.show_outline_level(0, Axis::Row, 1).unwrap();
        assert_eq!(hidden(&wb, 1..=6), vec![2, 3, 4, 5]);
        wb.show_outline_level(0, Axis::Row, 3).unwrap();
        assert!(hidden(&wb, 1..=6).is_empty());
        wb.insert_rows(0, 1, 1).unwrap();
        assert_eq!(wb.outline(0).rows[0], (3, 1));
        wb.undo().unwrap();
        wb.ungroup(0, Axis::Row, 2, 5).unwrap();
        assert_eq!(wb.outline(0).rows, vec![(3, 1), (4, 1)]);
    }

    #[test]
    fn subtotals_like_excel() {
        let mut wb = book();
        let opts = SubtotalOptions { group_col: 1, func: SubtotalFn::Sum, value_cols: vec![2], replace: true };
        wb.subtotal(0, CellRange { r0: 1, c0: 1, r1: 6, c1: 2 }, &opts).unwrap();
        let shown = |r: i32, c: i32| wb.model().get_formatted_cell_value(0, r, c).unwrap();
        // 부산 50, 60, 부산 요약 110, 서울 100, 30, 20, 서울 요약 150, 총 요약 260.
        assert_eq!(shown(4, 1), "부산 요약");
        assert_eq!(shown(4, 2), "110");
        assert_eq!(shown(8, 1), "서울 요약");
        assert_eq!(shown(8, 2), "150");
        assert_eq!(shown(9, 1), "총 요약");
        assert_eq!(shown(9, 2), "260");
        assert_eq!(wb.level(0, Axis::Row, 2), 2);
        assert_eq!(wb.level(0, Axis::Row, 4), 1);
        // Again with replace: same result, not doubled.
        wb.subtotal(0, CellRange { r0: 1, c0: 1, r1: 9, c1: 2 }, &opts).unwrap();
        assert_eq!(wb.model().get_formatted_cell_value(0, 9, 2).unwrap(), "260");
        assert_eq!(wb.model().get_formatted_cell_value(0, 10, 1).unwrap(), "");
        wb.remove_subtotals(0, CellRange { r0: 1, c0: 1, r1: 9, c1: 2 }).unwrap();
        assert_eq!(wb.model().get_formatted_cell_value(0, 4, 1).unwrap(), "서울");
        assert!(wb.outline(0).rows.is_empty());
    }
}
