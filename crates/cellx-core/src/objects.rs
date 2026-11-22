//! Memos, charts, conditional formatting, paste special and pivot tables.

use std::cmp::Ordering;
use std::collections::HashMap;

pub use ironcalc_base::cf_types::{CfRuleInput, ConditionalFormattingView};
use ironcalc_base::types::{Comment, Style};
use serde::{Deserialize, Serialize};

use crate::data::quote_ident;
use crate::error::{CellxError, Result};
use crate::extras::{
    chart_data, default_series_in_rows, parse_range, Aggregate, Anchor, Chart, ChartData, ChartKind, PivotDef,
    PivotSource,
};
use crate::ooxml::{self, ChartXml, Marker, MemoXml, SheetParts, EMU_PER_PX};
use crate::refs::{self, column_name, Axis};
use crate::value::Scalar;
use crate::workbook::{quote_sheet, CellRange, RecalcStats, Workbook};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Memo {
    pub row: i32,
    pub col: i32,
    pub text: String,
    pub author: String,
}

/// What "선택하여 붙여넣기" pastes.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
pub enum PasteWhat {
    All,
    Formulas,
    Values,
    Formats,
    ColumnWidths,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
pub enum PasteOp {
    None,
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct PasteSpecial {
    pub what: PasteWhat,
    pub op: PasteOp,
    pub skip_blanks: bool,
    pub transpose: bool,
}

fn parse_cell_ref(s: &str) -> Option<(i32, i32)> {
    let s = s.replace('$', "");
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = s.split_at(split);
    if letters.is_empty() {
        return None;
    }
    let mut col = 0i32;
    for ch in letters.chars() {
        if !ch.is_ascii_alphabetic() {
            return None;
        }
        col = col * 26 + (ch.to_ascii_uppercase() as i32 - 'A' as i32 + 1);
    }
    Some((digits.parse().ok()?, col))
}

/// New position of an index after inserting (`count` > 0) or deleting (`count` < 0) at `at`.
/// `None` when the index was deleted.
fn shift_index(v: i32, at: i32, count: i32) -> Option<i32> {
    if count > 0 {
        Some(if v >= at { v + count } else { v })
    } else if v >= at && v < at - count {
        None
    } else if v >= at - count {
        Some(v + count)
    } else {
        Some(v)
    }
}

pub(crate) fn range_a1(r: CellRange) -> String {
    let r = r.normalized();
    if r.r0 == r.r1 && r.c0 == r.c1 {
        format!("{}{}", column_name(r.c0), r.r0)
    } else {
        format!("{}{}:{}{}", column_name(r.c0), r.r0, column_name(r.c1), r.r1)
    }
}

/// How a value is typed back so that it stays the same value.
fn value_input(v: &Scalar) -> String {
    match v {
        Scalar::Empty => String::new(),
        Scalar::Number(n) => format!("{n}"),
        Scalar::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Scalar::Error(e) => e.clone(),
        Scalar::Text(t) => {
            let upper = t.to_ascii_uppercase();
            let risky = t.starts_with(['=', '+', '-', '\'', '#'])
                || t.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '.')
                || upper == "TRUE"
                || upper == "FALSE"
                || t.ends_with('%');
            if risky {
                format!("'{t}")
            } else {
                t.clone()
            }
        }
    }
}

fn compare_keys(a: &Scalar, b: &Scalar) -> Ordering {
    let rank = |s: &Scalar| match s {
        Scalar::Number(_) => 0,
        Scalar::Text(_) => 1,
        Scalar::Bool(_) => 2,
        Scalar::Error(_) => 3,
        Scalar::Empty => 4,
    };
    match (a, b) {
        (Scalar::Number(x), Scalar::Number(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Scalar::Text(x), Scalar::Text(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
        (Scalar::Bool(x), Scalar::Bool(y)) => x.cmp(y),
        _ => rank(a).cmp(&rank(b)),
    }
}

fn key_text(s: &Scalar) -> String {
    match s {
        Scalar::Empty => "(비어 있음)".into(),
        Scalar::Text(t) => t.clone(),
        Scalar::Number(n) => format!("{n}"),
        Scalar::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Scalar::Error(e) => e.clone(),
    }
}

fn author_name() -> String {
    std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "CellX".into())
}

impl Workbook {
    fn sheet_index(&self, name: Option<&str>, default: u32) -> Result<u32> {
        match name {
            None => Ok(default),
            Some(n) => self
                .sheet_names()
                .iter()
                .position(|s| s.eq_ignore_ascii_case(n))
                .map(|i| i as u32)
                .ok_or_else(|| CellxError::Calc(format!("시트를 찾을 수 없습니다: {n}"))),
        }
    }

    // ------------------------------------------------------------ keeping objects in step

    pub(crate) fn rename_in_objects(&mut self, old_prefix: &str, new_prefix: &str) {
        for c in &mut self.charts {
            if c.range.starts_with(old_prefix) {
                c.range = c.range.replacen(old_prefix, new_prefix, 1);
            }
        }
        for p in &mut self.pivots {
            if let PivotSource::Range { range } = &mut p.source {
                if range.starts_with(old_prefix) {
                    *range = range.replacen(old_prefix, new_prefix, 1);
                }
            }
        }
    }

    pub(crate) fn objects_after_sheet_delete(&mut self, sheet: u32) {
        self.pictures.retain(|p| p.sheet != sheet);
        for p in &mut self.pictures {
            if p.sheet > sheet {
                p.sheet -= 1;
            }
        }
        self.charts.retain(|c| c.sheet != sheet);
        self.pivots.retain(|p| p.sheet != sheet);
        self.filters.retain(|f| f.sheet != sheet);
        self.validations.retain(|v| v.sheet != sheet);
        for f in &mut self.filters {
            if f.sheet > sheet {
                f.sheet -= 1;
            }
        }
        for v in &mut self.validations {
            if v.sheet > sheet {
                v.sheet -= 1;
            }
        }
        for c in &mut self.charts {
            if c.sheet > sheet {
                c.sheet -= 1;
            }
        }
        for p in &mut self.pivots {
            if p.sheet > sheet {
                p.sheet -= 1;
            }
        }
    }

    /// Rows or columns were inserted or deleted: memos, chart ranges and positions,
    /// and pivot sources and positions follow, like cell references do.
    pub(crate) fn shift_objects(&mut self, sheet: u32, axis: Axis, at: i32, count: i32) {
        let names = self.sheet_names();
        let target = names.get(sheet as usize).cloned().unwrap_or_default();
        let shift_range = |range: &str, own: u32| -> String {
            let own = names.get(own as usize).cloned().unwrap_or_default();
            refs::shift_refs(&format!("={range}"), &own, &target, axis, at, count)
                .trim_start_matches('=')
                .to_string()
        };
        for p in &mut self.pictures {
            if p.sheet == sheet {
                let v = if axis == Axis::Row { &mut p.anchor.row } else { &mut p.anchor.col };
                *v = shift_index(*v, at, count).unwrap_or(at).max(1);
            }
        }
        for c in &mut self.charts {
            c.range = shift_range(&c.range, c.sheet);
            if c.sheet == sheet {
                let v = if axis == Axis::Row { &mut c.anchor.row } else { &mut c.anchor.col };
                *v = shift_index(*v, at, count).unwrap_or(at).max(1);
            }
        }
        for p in &mut self.pivots {
            if let PivotSource::Range { range } = &mut p.source {
                // Pivot sources name their sheet, so any sheet works as "own".
                *range = shift_range(range, p.sheet);
            }
            if p.sheet == sheet {
                let v = if axis == Axis::Row { &mut p.row } else { &mut p.col };
                *v = shift_index(*v, at, count).unwrap_or(at).max(1);
            }
        }
        for v in self.validations.iter_mut().filter(|v| v.sheet == sheet) {
            let parts: Vec<String> = v
                .range
                .split_whitespace()
                .map(|p| shift_range(p, sheet))
                .filter(|p| !p.contains("#REF!") && !p.is_empty())
                .collect();
            v.range = parts.join(" ");
        }
        self.validations.retain(|v| !v.range.is_empty());
        for f in self.filters.iter_mut().filter(|f| f.sheet == sheet) {
            let a1 = shift_range(&range_a1(f.range), sheet);
            match parse_range(&a1) {
                Some((_, r0, c0, r1, c1)) => {
                    let old_c0 = f.range.c0;
                    f.range = CellRange { r0, c0, r1, c1 };
                    if axis == Axis::Column {
                        let delta = c0 - old_c0;
                        for cf in &mut f.columns {
                            cf.col = shift_index(cf.col, at, count).unwrap_or(-1);
                        }
                        let _ = delta;
                        f.columns.retain(|cf| cf.col > 0);
                    }
                }
                None => f.columns.clear(),
            }
        }
        if let Some(ws) = self.model_mut().workbook.worksheets.get_mut(sheet as usize) {
            let old = std::mem::take(&mut ws.comments);
            for mut c in old {
                let Some((r, col)) = parse_cell_ref(&c.cell_ref) else { continue };
                let moved = if axis == Axis::Row {
                    shift_index(r, at, count).map(|r| (r, col))
                } else {
                    shift_index(col, at, count).map(|col| (r, col))
                };
                if let Some((r, col)) = moved {
                    c.cell_ref = format!("{}{}", column_name(col), r);
                    ws.comments.push(c);
                }
            }
        }
    }

    // ------------------------------------------------------------ memos

    pub fn memos(&self, sheet: u32) -> Result<Vec<Memo>> {
        let ws = self.model().workbook.worksheet(sheet)?;
        Ok(ws
            .comments
            .iter()
            .filter_map(|c| {
                let (row, col) = parse_cell_ref(&c.cell_ref)?;
                Some(Memo { row, col, text: c.text.clone(), author: c.author_name.clone() })
            })
            .collect())
    }

    /// Adds, changes (`Some`) or deletes (`None`) the memo of a cell.
    pub fn set_memo(&mut self, sheet: u32, row: i32, col: i32, text: Option<&str>) -> Result<RecalcStats> {
        let start = self.begin();
        let cell_ref = format!("{}{}", column_name(col), row);
        let author = author_name();
        let ws = self
            .model_mut()
            .workbook
            .worksheets
            .get_mut(sheet as usize)
            .ok_or_else(|| CellxError::Calc("시트가 없습니다".into()))?;
        let at = ws.comments.iter().position(|c| parse_cell_ref(&c.cell_ref) == Some((row, col)));
        match (text, at) {
            (Some(t), Some(i)) => ws.comments[i].text = t.to_string(),
            (Some(t), None) => ws.comments.push(Comment {
                text: t.to_string(),
                author_name: author,
                author_id: None,
                cell_ref,
            }),
            (None, Some(i)) => {
                ws.comments.remove(i);
            }
            (None, None) => {}
        }
        self.commit(start)
    }

    /// Deletes the memos inside a range.
    pub fn clear_memos(&mut self, sheet: u32, range: CellRange) -> Result<RecalcStats> {
        let start = self.begin();
        if let Some(ws) = self.model_mut().workbook.worksheets.get_mut(sheet as usize) {
            ws.comments
                .retain(|c| !parse_cell_ref(&c.cell_ref).is_some_and(|(r, col)| range.contains(r, col)));
        }
        self.commit(start)
    }

    // ------------------------------------------------------------ charts

    pub fn charts(&self, sheet: u32) -> Vec<Chart> {
        self.charts.iter().filter(|c| c.sheet == sheet).cloned().collect()
    }

    /// Inserts a chart of `range` next to it (삽입 → 차트), 480×288 px like Excel's default.
    pub fn add_chart(&mut self, sheet: u32, kind: ChartKind, range: CellRange) -> Result<Chart> {
        let r = range.normalized();
        let id = self.next_object_id;
        self.next_object_id += 1;
        let chart = Chart {
            id,
            sheet,
            kind,
            title: String::new(),
            range: range_a1(r),
            series_in_rows: default_series_in_rows(r.r0, r.c0, r.r1, r.c1),
            legend: true,
            anchor: Anchor { row: r.r0, col: r.c1 + 2, dx: 0.0, dy: 0.0, width: 480.0, height: 288.0 },
        };
        let start = self.begin();
        self.charts.push(chart.clone());
        self.commit(start)?;
        Ok(chart)
    }

    pub fn update_chart(&mut self, chart: Chart) -> Result<()> {
        if parse_range(&chart.range).is_none() {
            return Err(CellxError::Calc(format!("데이터 범위가 올바르지 않습니다: {}", chart.range)));
        }
        let start = self.begin();
        match self.charts.iter_mut().find(|c| c.id == chart.id) {
            Some(c) => *c = chart,
            None => return Err(CellxError::Calc("차트가 없습니다".into())),
        }
        self.commit(start)?;
        Ok(())
    }

    pub fn delete_chart(&mut self, id: u32) -> Result<()> {
        let start = self.begin();
        self.charts.retain(|c| c.id != id);
        self.commit(start)?;
        Ok(())
    }

    pub fn chart_data(&self, id: u32) -> Result<ChartData> {
        let c = self
            .charts
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| CellxError::Calc("차트가 없습니다".into()))?;
        self.resolve_chart(c)
    }

    fn resolve_chart(&self, c: &Chart) -> Result<ChartData> {
        let (sheet_name, r0, c0, r1, c1) =
            parse_range(&c.range).ok_or_else(|| CellxError::Calc(format!("데이터 범위가 올바르지 않습니다: {}", c.range)))?;
        let sheet = self.sheet_index(sheet_name.as_deref(), c.sheet)?;
        // Whole columns/rows: keep to the used part.
        let (last_row, last_col) = self.dimensions(sheet)?;
        let (r1, c1) = (r1.min(last_row.max(r0)), c1.min(last_col.max(c0)));
        let m = self.model();
        Ok(chart_data(
            c.kind,
            &c.title,
            sheet,
            (r0, c0, r1, c1),
            c.series_in_rows,
            |r, col| self.value(sheet, r, col).unwrap_or(Scalar::Empty),
            |r, col| m.get_formatted_cell_value(sheet, r, col).unwrap_or_default(),
        ))
    }

    pub(crate) fn col_px(&self, sheet: u32, col: i32) -> f64 {
        self.model().get_column_width(sheet, col).unwrap_or(72.0)
    }

    pub(crate) fn row_px(&self, sheet: u32, row: i32) -> f64 {
        self.model().get_row_height(sheet, row).unwrap_or(22.0)
    }

    pub(crate) fn to_marker(&self, sheet: u32, a: &Anchor) -> (Marker, Marker) {
        let from = Marker {
            col: a.col - 1,
            col_off: (a.dx * EMU_PER_PX) as i64,
            row: a.row - 1,
            row_off: (a.dy * EMU_PER_PX) as i64,
        };
        let mut rest = a.dx + a.width;
        let mut c = a.col;
        while c < refs::LAST_COL && rest >= self.col_px(sheet, c) {
            rest -= self.col_px(sheet, c);
            c += 1;
        }
        let mut rest_y = a.dy + a.height;
        let mut r = a.row;
        while r < refs::LAST_ROW && rest_y >= self.row_px(sheet, r) {
            rest_y -= self.row_px(sheet, r);
            r += 1;
        }
        let to = Marker {
            col: c - 1,
            col_off: (rest * EMU_PER_PX) as i64,
            row: r - 1,
            row_off: (rest_y * EMU_PER_PX) as i64,
        };
        (from, to)
    }

    /// Memos and charts of every sheet, for the xlsx writer.
    pub(crate) fn sheet_parts(&self) -> Vec<SheetParts> {
        let names = self.sheet_names();
        let mut parts: Vec<SheetParts> = (0..names.len()).map(|_| SheetParts::default()).collect();
        for (i, p) in parts.iter_mut().enumerate() {
            if let Ok(memos) = self.memos(i as u32) {
                p.memos = memos
                    .into_iter()
                    .map(|m| MemoXml { row: m.row, col: m.col, text: m.text, author: m.author })
                    .collect();
            }
        }
        for (i, p) in parts.iter_mut().enumerate() {
            let sheet = i as u32;
            let outline = self.outline(sheet);
            p.outline_rows = outline.rows;
            p.outline_cols = outline.cols;
            if let Ok(ws) = self.model().workbook.worksheet(sheet) {
                p.hidden_rows = ws.rows.iter().filter(|r| r.hidden).map(|r| r.r).collect();
                p.cols = ws.cols.clone();
            }
            p.protection = self.protections.iter().find(|x| x.sheet == sheet).cloned();
            p.unlocked = self.unlocked.iter().filter(|(s, _)| *s == sheet).map(|(_, r)| *r).collect();
            p.page = self.page_setups.iter().find(|x| x.sheet == sheet).cloned();
            p.tab_color = self
                .model()
                .workbook
                .worksheet(sheet)
                .ok()
                .map(|w| w.color.clone())
                .filter(|c| *c != ironcalc_base::types::Color::None);
            let filter = self.filters.iter().find(|f| f.sheet == sheet).cloned();
            let names = self.sheet_names();
            let mut tables: Vec<ironcalc_base::types::Table> =
                self.model().workbook.tables.values().filter(|t| names.get(i) == Some(&t.sheet_name)).cloned().collect();
            tables.sort_by(|a, b| a.name.cmp(&b.name));
            let mut claimed = false;
            for t in tables {
                // A filter over the table's data belongs to the table in xlsx.
                let owns = filter.as_ref().is_some_and(|f| {
                    parse_range(&t.reference).is_some_and(|(_, r0, c0, _, c1)| f.range.r0 == r0 && f.range.c0 == c0 && f.range.c1 == c1)
                });
                claimed |= owns;
                p.tables.push(ooxml::TableXml { table: t, filter: if owns { filter.clone() } else { None } });
            }
            if !claimed {
                p.filter = filter;
            }
        }
        for v in &self.validations {
            if let Some(p) = parts.get_mut(v.sheet as usize) {
                p.validations.push(v.clone());
            }
        }
        for (sheet, pic) in self.picture_parts() {
            if let Some(p) = parts.get_mut(sheet as usize) {
                p.pictures.push(pic);
            }
        }
        for c in &self.charts {
            let Ok(data) = self.resolve_chart(c) else { continue };
            let Some(p) = parts.get_mut(c.sheet as usize) else { continue };
            let (from, to) = self.to_marker(c.sheet, &c.anchor);
            p.charts.push(ChartXml {
                kind: c.kind,
                legend: c.legend,
                data_sheet: names.get(data.sheet as usize).cloned().unwrap_or_default(),
                data,
                from,
                to,
            });
        }
        parts
    }

    /// Objects IronCalc does not read from xlsx: charts, AutoFilter, data validation.
    pub(crate) fn import_extras(&mut self, xlsx: &[u8]) {
        self.import_charts(xlsx);
        self.import_pictures(xlsx);
        // CellX writes these names itself on save.
        self.model_mut()
            .workbook
            .defined_names
            .retain(|d| {
                !d.name.eq_ignore_ascii_case("_xlnm._FilterDatabase")
                    && !d.name.eq_ignore_ascii_case("_xlnm.Print_Titles")
                    && !d.name.eq_ignore_ascii_case("_xlnm.Print_Area")
            });
        for o in ooxml::read_sheet_options(xlsx) {
            let sheet = o.sheet as u32;
            for (r, l) in o.outline_rows {
                self.outline_rows.insert((sheet, r), l);
            }
            for (c, l) in o.outline_cols {
                self.outline_cols.insert((sheet, c), l);
            }
            if let Some(p) = o.protection {
                self.protections.push(p);
            }
            self.unlocked.extend(o.unlocked.into_iter().map(|r| (sheet, r)));
            if let Some(p) = o.page {
                self.page_setups.push(p);
            }
        }
        // IronCalc reads table parts without their style, filter and totals settings.
        let meta = ooxml::read_table_meta(xlsx);
        for t in self.model_mut().workbook.tables.values_mut() {
            if let Some(m) = meta.get(&t.name) {
                t.style_info = m.style.clone();
                t.has_filters = m.has_filters;
                for (c, (label, func)) in t.columns.iter_mut().zip(m.totals.iter()) {
                    c.totals_row_label = label.clone();
                    c.totals_row_function = func.clone();
                }
            }
        }
        // Table filters (inside the table part) become the sheet's AutoFilter.
        let names = self.sheet_names();
        let tables: Vec<ironcalc_base::types::Table> = self.model().workbook.tables.values().cloned().collect();
        for t in tables {
            let Some(sheet) = names.iter().position(|n| *n == t.sheet_name).map(|i| i as u32) else { continue };
            if !t.has_filters || self.filters.iter().any(|f| f.sheet == sheet) {
                continue;
            }
            if let Some((_, r0, c0, r1, c1)) = parse_range(&t.reference) {
                let range = CellRange { r0, c0, r1: r1 - t.totals_row_count as i32, c1 };
                let columns = ooxml::read_table_filter(xlsx, &t.name).map(|f| f.columns).unwrap_or_default();
                self.filters.push(crate::datatools::AutoFilter { sheet, range, columns });
            }
        }
        self.model_mut().refresh_parser_tables();
        for (_, filter, validations) in ooxml::read_sheet_rules(xlsx) {
            if let Some(f) = filter {
                self.filters.retain(|x| x.sheet != f.sheet);
                self.filters.push(f);
            }
            self.validations.extend(validations.into_iter().filter(|v| v.kind != crate::datatools::ValidationKind::Any));
        }
    }

    /// Charts read from an xlsx become CellX charts over the same cells.
    pub(crate) fn import_charts(&mut self, xlsx: &[u8]) {
        let names = self.sheet_names();
        for ic in ooxml::read_charts(xlsx) {
            let sheet = ic.sheet as u32;
            let mut sheet_name: Option<String> = None;
            let mut bounds: Option<(i32, i32, i32, i32)> = None;
            let mut rows_wise = false;
            for (i, f) in ic.values.iter().chain(ic.categories.iter()).chain(ic.names.iter()).enumerate() {
                let Some((sn, r0, c0, r1, c1)) = parse_range(f) else { continue };
                if i == 0 {
                    rows_wise = r0 == r1 && c1 > c0;
                    sheet_name = sn.clone();
                }
                bounds = Some(match bounds {
                    None => (r0, c0, r1, c1),
                    Some((a, b, c, d)) => (a.min(r0), b.min(c0), c.max(r1), d.max(c1)),
                });
            }
            let Some((r0, c0, r1, c1)) = bounds else { continue };
            let own = names.get(ic.sheet).cloned().unwrap_or_default();
            let a1 = range_a1(CellRange { r0, c0, r1, c1 });
            let range = match &sheet_name {
                Some(n) if !n.eq_ignore_ascii_case(&own) => format!("{}!{a1}", quote_sheet(n)),
                _ => a1,
            };
            let dx = ic.from.col_off as f64 / EMU_PER_PX;
            let dy = ic.from.row_off as f64 / EMU_PER_PX;
            let (width, height) = match (ic.ext, ic.to) {
                (Some((cx, cy)), _) if cx > 0 => (cx as f64 / EMU_PER_PX, cy as f64 / EMU_PER_PX),
                (_, Some(to)) => {
                    let w: f64 = ((ic.from.col + 1)..=to.col).map(|c| self.col_px(sheet, c)).sum::<f64>() - dx
                        + to.col_off as f64 / EMU_PER_PX;
                    let h: f64 = ((ic.from.row + 1)..=to.row).map(|r| self.row_px(sheet, r)).sum::<f64>() - dy
                        + to.row_off as f64 / EMU_PER_PX;
                    (w, h)
                }
                _ => (480.0, 288.0),
            };
            let id = self.next_object_id;
            self.next_object_id += 1;
            self.charts.push(Chart {
                id,
                sheet,
                kind: ic.kind,
                title: ic.title,
                range,
                series_in_rows: rows_wise,
                legend: ic.legend,
                anchor: Anchor {
                    row: ic.from.row + 1,
                    col: ic.from.col + 1,
                    dx,
                    dy,
                    width: width.max(60.0),
                    height: height.max(40.0),
                },
            });
        }
    }

    // ------------------------------------------------------------ conditional formatting

    pub fn conditional_formats(&self, sheet: u32) -> Result<Vec<ConditionalFormattingView>> {
        Ok(self.um.get_conditional_formatting_list(sheet)?)
    }

    pub fn add_conditional_format(&mut self, sheet: u32, range: CellRange, rule: CfRuleInput) -> Result<RecalcStats> {
        self.guard(sheet, range, crate::sheetopts::Guard::FormatCells)?;
        let start = self.begin();
        self.um.add_conditional_formatting(sheet, &range_a1(range), rule)?;
        self.commit(start)
    }

    pub fn delete_conditional_format(&mut self, sheet: u32, index: u32) -> Result<RecalcStats> {
        let start = self.begin();
        self.um.delete_conditional_formatting(sheet, index)?;
        self.commit(start)
    }

    /// 규칙 지우기: rules that lie inside `range` (or the whole sheet when `None`).
    pub fn clear_conditional_formats(&mut self, sheet: u32, range: Option<CellRange>) -> Result<RecalcStats> {
        let start = self.begin();
        let mut doomed: Vec<usize> = self
            .um
            .get_conditional_formatting_list(sheet)?
            .into_iter()
            .filter(|v| match range {
                None => true,
                Some(sel) => v.range.split_whitespace().all(|part| {
                    parse_range(part).is_some_and(|(_, r0, c0, r1, c1)| sel.contains(r0, c0) && sel.contains(r1, c1))
                }),
            })
            .map(|v| v.index)
            .collect();
        doomed.sort_unstable_by(|a, b| b.cmp(a));
        for i in doomed {
            self.um.delete_conditional_formatting(sheet, i as u32)?;
        }
        self.commit(start)
    }

    // ------------------------------------------------------------ paste special

    /// 선택하여 붙여넣기 of what was last copied in CellX.
    pub fn paste_special(&mut self, sheet: u32, at_row: i32, at_col: i32, opts: PasteSpecial) -> Result<RecalcStats> {
        let stored = self
            .clipboard
            .take()
            .ok_or_else(|| CellxError::Calc("먼저 셀을 복사하세요".into()))?;
        let result = self.paste_special_from(&stored, sheet, at_row, at_col, opts);
        if !stored.cut {
            self.clipboard = Some(stored);
        }
        result
    }

    fn paste_special_from(
        &mut self,
        stored: &crate::workbook::StoredClipboard,
        sheet: u32,
        at_row: i32,
        at_col: i32,
        opts: PasteSpecial,
    ) -> Result<RecalcStats> {
        let (sr0, sc0, sr1, sc1) = stored.clip.range();
        let src_sheet = stored.clip.sheet();
        let data = stored.clip.data();
        let (h, w) = (sr1 - sr0 + 1, sc1 - sc0 + 1);
        let place = |dr: i32, dc: i32| if opts.transpose { (at_row + dc, at_col + dr) } else { (at_row + dr, at_col + dc) };
        let (th, tw) = if opts.transpose { (w, h) } else { (h, w) };
        let target = CellRange { r0: at_row, c0: at_col, r1: at_row + th - 1, c1: at_col + tw - 1 };

        struct Src {
            dr: i32,
            dc: i32,
            text: String,
            value: Scalar,
            style: Option<Style>,
        }
        let mut cells = Vec::new();
        for r in sr0..=sr1 {
            for c in sc0..=sc1 {
                let (dr, dc) = (r - sr0, c - sc0);
                let cell = data.get(&r).and_then(|m| m.get(&c));
                let text = stored
                    .bridges
                    .iter()
                    .find(|(br, bc, _)| *br == dr && *bc == dc)
                    .map(|(_, _, f)| f.clone())
                    .or_else(|| cell.map(|c| c.text().to_string()))
                    .unwrap_or_default();
                cells.push(Src {
                    dr,
                    dc,
                    value: self.value(src_sheet, r, c).unwrap_or(Scalar::Empty),
                    style: cell.map(|c| c.style().clone()),
                    text,
                });
            }
        }

        let start = self.begin();
        if opts.what == PasteWhat::ColumnWidths {
            for c in sc0..=sc1 {
                let px = self.col_px(src_sheet, c);
                let tc = at_col + (c - sc0);
                self.um.set_columns_width(sheet, tc, tc, px)?;
            }
            return self.commit(start);
        }
        let contents = matches!(opts.what, PasteWhat::All | PasteWhat::Formulas | PasteWhat::Values);
        let formats = matches!(opts.what, PasteWhat::All | PasteWhat::Formats);
        if contents {
            for s in &cells {
                let blank = s.text.is_empty();
                if blank && (opts.skip_blanks || opts.op != PasteOp::None) {
                    continue;
                }
                let (tr, tc) = place(s.dr, s.dc);
                let src_r = sr0 + s.dr;
                let src_c = sc0 + s.dc;
                let mut input = if opts.what == PasteWhat::Values {
                    value_input(&s.value)
                } else if s.text.starts_with('=') && opts.transpose {
                    refs::transpose_refs(&s.text, (src_r, src_c), (tr, tc))
                } else if s.text.starts_with('=') {
                    refs::offset_refs(&s.text, tr - src_r, tc - src_c)
                } else {
                    s.text.clone()
                };
                if opts.op != PasteOp::None {
                    let sym = match opts.op {
                        PasteOp::Add => "+",
                        PasteOp::Subtract => "-",
                        PasteOp::Multiply => "*",
                        PasteOp::Divide => "/",
                        PasteOp::None => unreachable!(),
                    };
                    let old = self.input(sheet, tr, tc)?;
                    let operand = if input.starts_with('=') { format!("({})", &input[1..]) } else { input.clone() };
                    input = if old.starts_with('=') || input.starts_with('=') {
                        let left = if old.is_empty() { "0".into() } else if let Some(f) = old.strip_prefix('=') { format!("({f})") } else { old };
                        format!("={left}{sym}{operand}")
                    } else {
                        let a = match self.value(sheet, tr, tc)? {
                            Scalar::Number(n) => n,
                            Scalar::Empty => 0.0,
                            _ => continue, // Excel leaves text cells alone
                        };
                        let Scalar::Number(b) = s.value else { continue };
                        let v = match opts.op {
                            PasteOp::Add => a + b,
                            PasteOp::Subtract => a - b,
                            PasteOp::Multiply => a * b,
                            _ if b == 0.0 => {
                                input = "=#DIV/0!".into();
                                self.write_input(sheet, tr, tc, &input, true)?;
                                continue;
                            }
                            _ => a / b,
                        };
                        format!("{v}")
                    };
                }
                self.write_input(sheet, tr, tc, &input, true)?;
            }
        }
        if formats {
            let mut styles: Vec<Vec<Style>> = vec![vec![Style::default(); tw as usize]; th as usize];
            let mut any_blank_skip = false;
            for s in &cells {
                let (tr, tc) = place(s.dr, s.dc);
                let style = match &s.style {
                    Some(st) => st.clone(),
                    None => self.um.get_cell_style(src_sheet, sr0 + s.dr, sc0 + s.dc)?,
                };
                if opts.skip_blanks && s.text.is_empty() {
                    any_blank_skip = true;
                }
                styles[(tr - at_row) as usize][(tc - at_col) as usize] = style;
            }
            if any_blank_skip {
                // Keep the target style where the source cell is blank.
                for s in &cells {
                    if s.text.is_empty() {
                        let (tr, tc) = place(s.dr, s.dc);
                        styles[(tr - at_row) as usize][(tc - at_col) as usize] = self.um.get_cell_style(sheet, tr, tc)?;
                    }
                }
            }
            self.select(sheet, target)?;
            self.um.on_paste_styles(&styles)?;
        }
        self.commit(start)
    }

    // ------------------------------------------------------------ pivot tables

    /// Field names of a pivot source (the header row of a range, or a data sheet's columns).
    pub fn pivot_fields(&mut self, source: &PivotSource, default_sheet: u32) -> Result<Vec<String>> {
        match source {
            PivotSource::Table { name } => Ok(self.data().table_info(name)?.columns.into_iter().map(|c| c.name).collect()),
            PivotSource::Range { range } => {
                let (sn, r0, c0, _, c1) =
                    parse_range(range).ok_or_else(|| CellxError::Calc(format!("범위가 올바르지 않습니다: {range}")))?;
                let sheet = self.sheet_index(sn.as_deref(), default_sheet)?;
                let (_, last_col) = self.dimensions(sheet)?;
                Ok(header_names(self, sheet, r0, c0, c1.min(last_col.max(c0))))
            }
        }
    }

    pub fn pivots(&self) -> Vec<PivotDef> {
        self.pivots.clone()
    }

    pub(crate) fn set_pivots(&mut self, pivots: Vec<PivotDef>) {
        self.next_object_id = self.next_object_id.max(pivots.iter().map(|p| p.id + 1).max().unwrap_or(1));
        self.pivots = pivots;
    }

    /// The pivot table whose output contains a cell.
    pub fn pivot_at(&self, sheet: u32, row: i32, col: i32) -> Option<PivotDef> {
        self.pivots
            .iter()
            .find(|p| {
                p.sheet == sheet
                    && row >= p.row
                    && col >= p.col
                    && row < p.row + p.extent.0.max(1)
                    && col < p.col + p.extent.1.max(1)
            })
            .cloned()
    }

    /// Creates (or with an existing id, changes) a pivot table and writes it out.
    pub fn save_pivot(&mut self, mut def: PivotDef, new_sheet: bool) -> Result<PivotDef> {
        if def.values.is_empty() && def.rows.is_empty() && def.columns.is_none() {
            return Err(CellxError::Calc("행, 열 또는 값에 필드를 하나 이상 넣으세요".into()));
        }
        let start = self.begin();
        if new_sheet {
            self.um.new_sheet()?;
            def.sheet = self.sheet_names().len() as u32 - 1;
            def.row = 3;
            def.col = 1;
            def.extent = (0, 0);
        }
        if let PivotSource::Range { range } = &def.source {
            // Pivot sources always name their sheet so they survive moves.
            if !range.contains('!') {
                let name = self.sheet_names().get(def.sheet as usize).cloned().unwrap_or_default();
                def.source = PivotSource::Range { range: format!("{}!{range}", quote_sheet(&name)) };
            }
        }
        if def.id == 0 {
            def.id = self.next_object_id;
            self.next_object_id += 1;
        } else if let Some(old) = self.pivots.iter().find(|p| p.id == def.id) {
            def.extent = old.extent;
        }
        let out = self.write_pivot(&mut def);
        if out.is_ok() {
            self.pivots.retain(|p| p.id != def.id);
            self.pivots.push(def.clone());
        }
        let stats = self.commit(start);
        out?;
        stats?;
        Ok(def)
    }

    pub fn refresh_pivots(&mut self) -> Result<RecalcStats> {
        let start = self.begin();
        let mut defs = std::mem::take(&mut self.pivots);
        let mut first_err = None;
        for d in &mut defs {
            if let Err(e) = self.write_pivot(d) {
                first_err.get_or_insert(e);
            }
        }
        self.pivots = defs;
        let stats = self.commit(start)?;
        match first_err {
            Some(e) => Err(e),
            None => Ok(stats),
        }
    }

    pub fn delete_pivot(&mut self, id: u32) -> Result<RecalcStats> {
        let start = self.begin();
        if let Some(p) = self.pivots.iter().find(|p| p.id == id).cloned() {
            if p.extent.0 > 0 {
                let area = CellRange { r0: p.row, c0: p.col, r1: p.row + p.extent.0 - 1, c1: p.col + p.extent.1 - 1 };
                self.um.range_clear_all(&area.area(p.sheet))?;
            }
            self.pivots.retain(|x| x.id != id);
        }
        self.commit(start)
    }

    fn write_pivot(&mut self, def: &mut PivotDef) -> Result<()> {
        let grid = self.compute_pivot(def)?;
        if def.extent.0 > 0 {
            let old = CellRange {
                r0: def.row,
                c0: def.col,
                r1: def.row + def.extent.0 - 1,
                c1: def.col + def.extent.1 - 1,
            };
            self.um.range_clear_all(&old.area(def.sheet))?;
        }
        let rows = grid.cells.len() as i32;
        let cols = grid.cells.iter().map(|r| r.len()).max().unwrap_or(0) as i32;
        for (i, row) in grid.cells.iter().enumerate() {
            for (j, v) in row.iter().enumerate() {
                if matches!(v, Scalar::Empty) {
                    continue;
                }
                let (r, c) = (def.row + i as i32, def.col + j as i32);
                self.write_input(def.sheet, r, c, &value_input(v), true)?;
            }
        }
        if rows > 0 && cols > 0 {
            let header = CellRange { r0: def.row, c0: def.col, r1: def.row + grid.header_rows - 1, c1: def.col + cols - 1 };
            self.um.update_range_style(&header.area(def.sheet), "fill.fg_color", "#DDEBF7")?;
            self.um.update_range_style(&header.area(def.sheet), "font.b", "true")?;
            if grid.total_row {
                let t = CellRange { r0: def.row + rows - 1, c0: def.col, r1: def.row + rows - 1, c1: def.col + cols - 1 };
                self.um.update_range_style(&t.area(def.sheet), "fill.fg_color", "#DDEBF7")?;
                self.um.update_range_style(&t.area(def.sheet), "font.b", "true")?;
            }
            let body = CellRange { r0: def.row + grid.header_rows, c0: def.col + grid.label_cols, r1: def.row + rows - 1, c1: def.col + cols - 1 };
            if body.r1 >= body.r0 && body.c1 >= body.c0 {
                let all_whole = grid.cells.iter().skip(grid.header_rows as usize).flat_map(|r| r.iter()).all(|v| match v {
                    Scalar::Number(n) => n.fract() == 0.0,
                    _ => true,
                });
                let fmt = if all_whole { "#,##0" } else { "#,##0.00" };
                self.um.update_range_style(&body.area(def.sheet), "num_fmt", fmt)?;
            }
        }
        def.extent = (rows, cols);
        Ok(())
    }

    /// Runs the pivot in the data engine: GROUPING SETS give the cells, row and column
    /// totals and the grand total in one query.
    fn compute_pivot(&mut self, def: &PivotDef) -> Result<PivotGrid> {
        let (table, temp) = match &def.source {
            PivotSource::Table { name } => (quote_ident(name), false),
            PivotSource::Range { range } => {
                self.load_range_table(range, def.sheet)?;
                (quote_ident(PIVOT_TABLE), true)
            }
        };
        let result = self.pivot_query(def, &table);
        if temp {
            let _ = self.data().execute_internal(&format!("DROP TABLE IF EXISTS {table}"));
        }
        result
    }

    fn load_range_table(&mut self, range: &str, default_sheet: u32) -> Result<()> {
        let (sn, r0, c0, r1, c1) = parse_range(range).ok_or_else(|| CellxError::Calc(format!("범위가 올바르지 않습니다: {range}")))?;
        let sheet = self.sheet_index(sn.as_deref(), default_sheet)?;
        let (last_row, last_col) = self.dimensions(sheet)?;
        let (r1, c1) = (r1.min(last_row.max(r0)), c1.min(last_col.max(c0)));
        let names = header_names(self, sheet, r0, c0, c1);
        let width = names.len();
        let mut rows: Vec<Vec<Scalar>> = Vec::new();
        for r in (r0 + 1)..=r1 {
            let row: Vec<Scalar> = (c0..=c1).map(|c| self.value(sheet, r, c).unwrap_or(Scalar::Empty)).collect();
            if row.iter().all(|v| matches!(v, Scalar::Empty)) {
                continue;
            }
            rows.push(row);
        }
        let numeric: Vec<bool> = (0..width)
            .map(|j| {
                let mut any = false;
                for row in &rows {
                    match &row[j] {
                        Scalar::Number(_) => any = true,
                        Scalar::Empty => {}
                        _ => return false,
                    }
                }
                any
            })
            .collect();
        let cols: Vec<String> = names
            .iter()
            .zip(&numeric)
            .map(|(n, num)| format!("{} {}", quote_ident(n), if *num { "DOUBLE" } else { "VARCHAR" }))
            .collect();
        let t = quote_ident(PIVOT_TABLE);
        let mut sql = format!("CREATE OR REPLACE TABLE {t} ({});", cols.join(", "));
        for chunk in rows.chunks(500) {
            let values: Vec<String> = chunk
                .iter()
                .map(|row| {
                    let vals: Vec<String> = row
                        .iter()
                        .zip(&numeric)
                        .map(|(v, num)| match v {
                            Scalar::Empty => "NULL".into(),
                            Scalar::Number(n) if *num => format!("{n}"),
                            other => format!("'{}'", key_text(other).replace('\'', "''")),
                        })
                        .collect();
                    format!("({})", vals.join(", "))
                })
                .collect();
            sql.push_str(&format!("INSERT INTO {t} VALUES {};", values.join(", ")));
        }
        self.data().execute_internal(&sql)
    }

    fn pivot_query(&mut self, def: &PivotDef, table: &str) -> Result<PivotGrid> {
        let mut fields: Vec<String> = def.rows.clone();
        if let Some(c) = &def.columns {
            fields.push(c.clone());
        }
        let nr = def.rows.len();
        let has_c = def.columns.is_some();
        let q: Vec<String> = fields.iter().map(|f| quote_ident(f)).collect();
        let aggs: Vec<String> = def
            .values
            .iter()
            .map(|v| {
                let f = quote_ident(&v.field);
                match v.agg {
                    Aggregate::Count => format!("COUNT({f})"),
                    Aggregate::Sum => format!("SUM(TRY_CAST({f} AS DOUBLE))"),
                    Aggregate::Average => format!("AVG(TRY_CAST({f} AS DOUBLE))"),
                    Aggregate::Max => format!("MAX(TRY_CAST({f} AS DOUBLE))"),
                    Aggregate::Min => format!("MIN(TRY_CAST({f} AS DOUBLE))"),
                }
            })
            .collect();
        let aggs_sql = if aggs.is_empty() { "COUNT(*)".to_string() } else { aggs.join(", ") };
        let rset = q[..nr].join(", ");
        let mut sets = Vec::new();
        if has_c {
            sets.push(format!("({})", q.join(", ")));
            if nr > 0 {
                sets.push(format!("({rset})"));
            }
            sets.push(format!("({})", q[nr]));
        } else if nr > 0 {
            sets.push(format!("({rset})"));
        }
        sets.push("()".into());
        let grouping = if fields.is_empty() { "0".to_string() } else { format!("GROUPING({})", q.join(", ")) };
        let select_fields = if q.is_empty() { String::new() } else { format!("{}, ", q.join(", ")) };
        let sql = format!(
            "SELECT {select_fields}{grouping} AS _g, {aggs_sql} FROM {table} GROUP BY GROUPING SETS ({})",
            sets.join(", ")
        );
        let (_, rows, _) = self.data().rows(&sql, &[], 5_000_000)?;
        let nf = fields.len();
        let nv = aggs.len().max(1);
        let bit = |i: usize| 1u64 << (nf - 1 - i);
        let all_r: u64 = (0..nr).map(bit).sum();
        let c_bit = if has_c { bit(nr) } else { 0 };

        let mut row_keys: Vec<Vec<Scalar>> = Vec::new();
        let mut col_keys: Vec<Scalar> = Vec::new();
        let mut cells: HashMap<(String, String), Vec<Scalar>> = HashMap::new();
        let mut row_totals: HashMap<String, Vec<Scalar>> = HashMap::new();
        let mut col_totals: HashMap<String, Vec<Scalar>> = HashMap::new();
        let mut grand = vec![Scalar::Empty; nv];
        let id = |vals: &[Scalar]| vals.iter().map(key_text).collect::<Vec<_>>().join("\u{1f}");
        for row in rows {
            let g = match &row[nf] {
                Scalar::Number(n) => *n as u64,
                _ => 0,
            };
            let rk: Vec<Scalar> = row[..nr].to_vec();
            let ck = if has_c { row[nr].clone() } else { Scalar::Empty };
            let vals: Vec<Scalar> = row[nf + 1..].to_vec();
            let r_grouped = g & all_r == 0;
            let c_grouped = has_c && g & c_bit == 0;
            match (nr > 0 && r_grouped, c_grouped) {
                (true, true) => {
                    cells.insert((id(&rk), key_text(&ck)), vals);
                }
                (true, false) => {
                    row_keys.push(rk.clone());
                    row_totals.insert(id(&rk), vals);
                }
                (false, true) => {
                    col_keys.push(ck.clone());
                    col_totals.insert(key_text(&ck), vals);
                }
                (false, false) => grand = vals,
            }
        }
        row_keys.sort_by(|a, b| {
            a.iter().zip(b.iter()).map(|(x, y)| compare_keys(x, y)).find(|o| *o != Ordering::Equal).unwrap_or(Ordering::Equal)
        });
        col_keys.sort_by(compare_keys);

        let labels: Vec<String> = if def.values.is_empty() {
            vec!["개수".into()]
        } else {
            def.values.iter().map(|v| format!("{} : {}", v.agg.label(), v.field)).collect()
        };
        let label_cols = nr.max(1);
        let mut out: Vec<Vec<Scalar>> = Vec::new();
        let text = |s: &str| Scalar::Text(s.to_string());
        let header_rows;
        if has_c {
            // Caption row: value label and "열 레이블".
            let mut cap = vec![Scalar::Empty; label_cols];
            cap[0] = text(if nv == 1 { &labels[0] } else { "값" });
            cap.push(text("열 레이블"));
            out.push(cap);
            let mut head: Vec<Scalar> =
                if nr > 0 { def.rows.iter().map(|r| text(r)).collect() } else { vec![Scalar::Empty] };
            for ck in &col_keys {
                for l in &labels {
                    head.push(if nv == 1 { Scalar::Text(key_text(ck)) } else { text(&format!("{} - {l}", key_text(ck))) });
                }
            }
            for l in &labels {
                head.push(if nv == 1 { text("총합계") } else { text(&format!("전체 {l}")) });
            }
            out.push(head);
            header_rows = 2;
            if nr > 0 {
                for rk in &row_keys {
                    let mut line: Vec<Scalar> = rk.iter().map(|k| if matches!(k, Scalar::Empty) { text("(비어 있음)") } else { k.clone() }).collect();
                    for ck in &col_keys {
                        match cells.get(&(id(rk), key_text(ck))) {
                            Some(v) => line.extend(v.iter().cloned()),
                            None => line.extend(std::iter::repeat_n(Scalar::Empty, nv)),
                        }
                    }
                    line.extend(row_totals.get(&id(rk)).cloned().unwrap_or_else(|| vec![Scalar::Empty; nv]));
                    out.push(line);
                }
            }
            let mut total = vec![Scalar::Empty; label_cols];
            total[0] = text(if nr > 0 { "총합계" } else { &labels[0] });
            for ck in &col_keys {
                total.extend(col_totals.get(&key_text(ck)).cloned().unwrap_or_else(|| vec![Scalar::Empty; nv]));
            }
            total.extend(grand.iter().cloned());
            out.push(total);
        } else {
            let mut head: Vec<Scalar> =
                if nr > 0 { def.rows.iter().map(|r| text(r)).collect() } else { vec![] };
            head.extend(labels.iter().map(|l| text(l)));
            out.push(head);
            header_rows = 1;
            for rk in &row_keys {
                let mut line: Vec<Scalar> = rk.iter().map(|k| if matches!(k, Scalar::Empty) { text("(비어 있음)") } else { k.clone() }).collect();
                line.extend(row_totals.get(&id(rk)).cloned().unwrap_or_else(|| vec![Scalar::Empty; nv]));
                out.push(line);
            }
            let mut total = if nr > 0 { vec![Scalar::Empty; nr] } else { vec![] };
            if nr > 0 {
                total[0] = text("총합계");
            }
            total.extend(grand.iter().cloned());
            out.push(total);
        }
        Ok(PivotGrid { cells: out, header_rows, label_cols: if nr == 0 && !has_c { 0 } else { label_cols as i32 }, total_row: true })
    }
}

const PIVOT_TABLE: &str = "_cellx_pivot_source";

struct PivotGrid {
    cells: Vec<Vec<Scalar>>,
    header_rows: i32,
    label_cols: i32,
    total_row: bool,
}

fn header_names(wb: &Workbook, sheet: u32, row: i32, c0: i32, c1: i32) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for c in c0..=c1 {
        let raw = wb.model().get_formatted_cell_value(sheet, row, c).unwrap_or_default();
        let mut name = if raw.trim().is_empty() { format!("열{}", c - c0 + 1) } else { raw.trim().to_string() };
        let base = name.clone();
        let mut n = 2;
        while names.iter().any(|x| x.eq_ignore_ascii_case(&name)) {
            name = format!("{base}{n}");
            n += 1;
        }
        names.push(name);
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extras::PivotValue;

    fn sample() -> Workbook {
        let mut wb = Workbook::new().unwrap();
        let rows = [
            ["지역", "상품", "금액"],
            ["서울", "A", "100"],
            ["부산", "A", "50"],
            ["서울", "B", "30"],
            ["서울", "A", "20"],
        ];
        let mut cells = Vec::new();
        for (i, r) in rows.iter().enumerate() {
            for (j, v) in r.iter().enumerate() {
                cells.push((i as i32 + 1, j as i32 + 1, v.to_string()));
            }
        }
        wb.set_inputs(0, &cells).unwrap();
        wb
    }

    #[test]
    fn memos_follow_rows_and_undo() {
        let mut wb = Workbook::new().unwrap();
        wb.set_memo(0, 3, 2, Some("확인 필요")).unwrap();
        wb.insert_rows(0, 1, 2).unwrap();
        assert_eq!(wb.memos(0).unwrap()[0].row, 5);
        wb.undo().unwrap();
        assert_eq!(wb.memos(0).unwrap()[0].row, 3);
        wb.undo().unwrap();
        assert!(wb.memos(0).unwrap().is_empty());
    }

    #[test]
    fn paste_values_and_transpose() {
        let mut wb = Workbook::new().unwrap();
        wb.set_input(0, 1, 1, "2").unwrap();
        wb.set_input(0, 1, 2, "=A1*10").unwrap();
        wb.copy(0, CellRange { r0: 1, c0: 1, r1: 1, c1: 2 }, false).unwrap();
        let opts = PasteSpecial { what: PasteWhat::Values, op: PasteOp::None, skip_blanks: false, transpose: true };
        wb.paste_special(0, 3, 1, opts).unwrap();
        assert_eq!(wb.input(0, 3, 1).unwrap(), "2");
        assert_eq!(wb.input(0, 4, 1).unwrap(), "20");
        // Formulas transposed move their references to the new place.
        let opts = PasteSpecial { what: PasteWhat::Formulas, op: PasteOp::None, skip_blanks: false, transpose: true };
        wb.paste_special(0, 3, 3, opts).unwrap();
        assert_eq!(wb.input(0, 4, 3).unwrap(), "=C3*10");
        // Multiply: target 5 × copied 2.
        wb.set_input(0, 6, 1, "5").unwrap();
        wb.copy(0, CellRange { r0: 1, c0: 1, r1: 1, c1: 1 }, false).unwrap();
        let opts = PasteSpecial { what: PasteWhat::Values, op: PasteOp::Multiply, skip_blanks: false, transpose: false };
        wb.paste_special(0, 6, 1, opts).unwrap();
        assert_eq!(wb.value(0, 6, 1).unwrap(), Scalar::Number(10.0));
    }

    #[test]
    fn pivot_with_rows_columns_and_totals() {
        let mut wb = sample();
        let def = PivotDef {
            id: 0,
            source: PivotSource::Range { range: "A1:C5".into() },
            rows: vec!["지역".into()],
            columns: Some("상품".into()),
            values: vec![PivotValue { field: "금액".into(), agg: Aggregate::Sum }],
            sheet: 0,
            row: 1,
            col: 6,
            extent: (0, 0),
        };
        let def = wb.save_pivot(def, false).unwrap();
        // F1 caption, F2 headers: 지역 | A | B | 총합계; rows 부산, 서울; 총합계.
        assert_eq!(wb.input(0, 2, 7).unwrap(), "A");
        assert_eq!(wb.input(0, 3, 6).unwrap(), "부산");
        assert_eq!(wb.value(0, 4, 7).unwrap(), Scalar::Number(120.0)); // 서울 A
        assert_eq!(wb.value(0, 4, 9).unwrap(), Scalar::Number(150.0)); // 서울 total
        assert_eq!(wb.value(0, 5, 9).unwrap(), Scalar::Number(200.0)); // grand total
        assert_eq!(def.extent, (5, 4));
        // Source changes, refresh picks them up.
        wb.set_input(0, 3, 3, "70").unwrap();
        wb.refresh_pivots().unwrap();
        assert_eq!(wb.value(0, 5, 9).unwrap(), Scalar::Number(220.0));
        assert!(wb.data().table_names().unwrap().is_empty());
    }

    #[test]
    fn charts_round_trip_through_xlsx() {
        let mut wb = sample();
        let chart = wb.add_chart(0, ChartKind::Column, CellRange { r0: 1, c0: 2, r1: 5, c1: 3 }).unwrap();
        let data = wb.chart_data(chart.id).unwrap();
        assert_eq!(data.series.len(), 1);
        assert_eq!(data.series[0].name, "금액");
        assert_eq!(data.categories, vec!["A", "A", "B", "A"]);
        wb.set_memo(0, 1, 1, Some("메모")).unwrap();
        let dir = crate::project::new_work_dir().unwrap();
        let path = dir.join("c.xlsx");
        wb.save_xlsx(&path).unwrap();
        let back = Workbook::open_xlsx(&path).unwrap();
        let charts = back.charts(0);
        assert_eq!(charts.len(), 1);
        assert_eq!(charts[0].kind, ChartKind::Column);
        assert_eq!(charts[0].range, "B1:C5");
        assert_eq!(charts[0].anchor.row, 1);
        assert_eq!(charts[0].anchor.col, 5);
        assert!((charts[0].anchor.width - 480.0).abs() < 1.0);
        assert_eq!(back.memos(0).unwrap()[0].text, "메모");
    }

    /// `CELLX_SAMPLE=/tmp/x.xlsx cargo test --workspace -- --ignored write_sample` writes a
    /// workbook with a chart, a memo and a pivot table for checking in Excel or LibreOffice.
    #[test]
    #[ignore]
    fn write_sample() {
        let Ok(path) = std::env::var("CELLX_SAMPLE") else { return };
        let mut wb = sample();
        wb.add_chart(0, ChartKind::Column, CellRange { r0: 1, c0: 2, r1: 5, c1: 3 }).unwrap();
        let mut line = wb.add_chart(0, ChartKind::Pie, CellRange { r0: 1, c0: 2, r1: 5, c1: 3 }).unwrap();
        line.anchor.row = 17;
        wb.update_chart(line).unwrap();
        wb.set_memo(0, 2, 1, Some("본사 포함")).unwrap();
        wb.toggle_auto_filter(0, CellRange { r0: 1, c0: 1, r1: 5, c1: 3 }).unwrap();
        wb.set_filter_column(
            0,
            crate::datatools::ColumnFilter { col: 1, values: Some(vec!["서울".into()]), conditions: vec![], and: true },
        )
        .unwrap();
        let v = crate::datatools::Validation {
            sheet: 0,
            range: String::new(),
            kind: crate::datatools::ValidationKind::List,
            op: crate::datatools::ValidationOp::Between,
            formula1: "\"A,B,C\"".into(),
            formula2: String::new(),
            allow_blank: true,
            in_cell_dropdown: true,
            show_input: false,
            input_title: String::new(),
            input_message: String::new(),
            show_error: true,
            error_style: crate::datatools::ErrorStyle::Stop,
            error_title: String::new(),
            error_message: String::new(),
        };
        wb.set_validation(0, CellRange { r0: 2, c0: 2, r1: 5, c1: 2 }, v).unwrap();
        wb.group(0, crate::refs::Axis::Row, 3, 4).unwrap();
        wb.group(0, crate::refs::Axis::Column, 2, 2).unwrap();
        wb.toggle_group(0, crate::refs::Axis::Row, 3, 1, true).unwrap();
        let mut ps = wb.page_setup(0);
        ps.landscape = true;
        ps.fit = crate::sheetopts::FitMode::Width;
        ps.title_rows = Some((1, 1));
        ps.row_breaks = vec![4];
        ps.page_numbers = true;
        wb.set_page_setup(ps).unwrap();
        wb.set_locked(0, CellRange { r0: 2, c0: 3, r1: 5, c1: 3 }, false).unwrap();
        wb.protect_sheet(0, Some("abc"), crate::sheetopts::ProtectOptions { auto_filter: true, ..Default::default() }).unwrap();
        wb.add_sheet().unwrap();
        wb.set_inputs(1, &[(1, 1, "품목".into()), (1, 2, "수량".into()), (2, 1, "사과".into()), (2, 2, "3".into()), (3, 1, "배".into()), (3, 2, "5".into())]).unwrap();
        wb.create_table(1, CellRange { r0: 1, c0: 1, r1: 3, c1: 2 }, true, "TableStyleMedium3").unwrap();
        wb.save_xlsx(std::path::Path::new(&path)).unwrap();
    }

    #[test]
    fn conditional_format_shows_in_viewport() {
        let mut wb = sample();
        let rule: CfRuleInput = serde_json::from_str(
            r##"{"type":"CellIs","operator":"GreaterThan","formula":"60","formula2":null,"format":{"font":null,"fill":{"color":"#FFC7CE"},"border":null,"num_fmt":null,"alignment":null},"stop_if_true":false}"##,
        )
        .unwrap();
        wb.add_conditional_format(0, CellRange { r0: 2, c0: 3, r1: 5, c1: 3 }, rule).unwrap();
        let vp = wb.viewport(0, 1, 1, 5, 3).unwrap();
        let fill = |r: i32| vp.cells.iter().find(|c| c.row == r && c.col == 3).and_then(|c| c.style.as_ref()).and_then(|s| s.fill.clone());
        assert_eq!(fill(2).map(|f| f.to_uppercase()), Some("#FFC7CE".into()));
        assert_eq!(fill(3), None);
        wb.clear_conditional_formats(0, None).unwrap();
        assert!(wb.conditional_formats(0).unwrap().is_empty());
    }
}
