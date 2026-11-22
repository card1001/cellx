//! Excel tables (표 서식), sheet protection (시트 보호), page setup (페이지 설정) and
//! custom-formula validation.

use ironcalc_base::types::{Table, TableColumn, TableStyleInfo};
use serde::{Deserialize, Serialize};

use crate::datatools::{AutoFilter, Validation};
use crate::error::{CellxError, Result};
use crate::extras::parse_range;
use crate::objects::range_a1;
use crate::refs::{self, column_name, Axis};
use crate::value::Scalar;
use crate::workbook::{BorderView, CellRange, CellStyle, RecalcStats, Workbook};

// ---------------------------------------------------------------- tables

/// Colors of a table style: header fill, header text, stripe fill, border.
pub struct TablePalette {
    pub header_fill: Option<&'static str>,
    pub header_font: Option<&'static str>,
    pub stripe: Option<&'static str>,
    pub border: Option<&'static str>,
}

/// The table styles CellX offers (Excel names, so xlsx files keep them).
pub const TABLE_STYLES: [&str; 10] = [
    "TableStyleLight1",
    "TableStyleLight9",
    "TableStyleMedium2",
    "TableStyleMedium3",
    "TableStyleMedium4",
    "TableStyleMedium5",
    "TableStyleMedium6",
    "TableStyleMedium7",
    "TableStyleMedium9",
    "TableStyleDark1",
];

pub fn table_palette(name: &str) -> TablePalette {
    let p = |h: Option<&'static str>, f: Option<&'static str>, s: Option<&'static str>, b: Option<&'static str>| TablePalette {
        header_fill: h,
        header_font: f,
        stripe: s,
        border: b,
    };
    match name {
        "TableStyleLight1" => p(None, None, Some("#D9D9D9"), Some("#000000")),
        "TableStyleLight9" => p(Some("#4472C4"), Some("#FFFFFF"), None, Some("#8EA9DB")),
        "TableStyleMedium3" => p(Some("#ED7D31"), Some("#FFFFFF"), Some("#FCE4D6"), None),
        "TableStyleMedium4" => p(Some("#A5A5A5"), Some("#FFFFFF"), Some("#EDEDED"), None),
        "TableStyleMedium5" => p(Some("#FFC000"), Some("#FFFFFF"), Some("#FFF2CC"), None),
        "TableStyleMedium6" => p(Some("#5B9BD5"), Some("#FFFFFF"), Some("#DDEBF7"), None),
        "TableStyleMedium7" => p(Some("#70AD47"), Some("#FFFFFF"), Some("#E2EFDA"), None),
        "TableStyleMedium9" => p(Some("#4472C4"), Some("#FFFFFF"), Some("#B4C6E7"), None),
        "TableStyleDark1" => p(Some("#000000"), Some("#FFFFFF"), Some("#737373"), None),
        _ => p(Some("#4472C4"), Some("#FFFFFF"), Some("#D9E1F2"), None),
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct TableView {
    pub name: String,
    pub sheet: u32,
    pub range: CellRange,
    pub style: String,
    pub stripes: bool,
    pub header: bool,
    pub totals: bool,
    pub columns: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TableUpdate {
    pub name: String,
    pub new_name: String,
    pub style: String,
    pub stripes: bool,
    pub totals: bool,
}

// ---------------------------------------------------------------- protection

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct ProtectOptions {
    pub format_cells: bool,
    pub format_columns: bool,
    pub format_rows: bool,
    pub insert_rows: bool,
    pub insert_columns: bool,
    pub delete_rows: bool,
    pub delete_columns: bool,
    pub sort: bool,
    pub auto_filter: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SheetProtection {
    pub sheet: u32,
    /// Excel's legacy 16-bit password hash, hex (xlsx `password`).
    pub password_hash: Option<String>,
    pub options: ProtectOptions,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ProtectionView {
    pub protected: bool,
    pub has_password: bool,
    pub options: ProtectOptions,
    /// Cells left editable (셀 서식 → 보호 → 잠금 해제).
    pub unlocked: Vec<CellRange>,
}

/// What an edit needs when the sheet is protected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guard {
    Content,
    FormatCells,
    FormatColumns,
    FormatRows,
    InsertRows,
    InsertColumns,
    DeleteRows,
    DeleteColumns,
    Sort,
    AutoFilter,
}

const PROTECTED: &str = "변경하려는 셀이 보호된 시트에 있습니다. 변경하려면 검토 탭에서 시트 보호를 해제하세요.";

/// Excel's sheet password hash (the one in xlsx `sheetProtection/@password`).
pub fn excel_password_hash(password: &str) -> String {
    let chars: Vec<u32> = password.encode_utf16().map(u32::from).collect();
    let mut hash: u32 = 0;
    for (i, c) in chars.iter().enumerate() {
        let value = c << (i + 1);
        let rotated = value >> 15;
        hash ^= (value & 0x7fff) | rotated;
    }
    hash ^= chars.len() as u32;
    hash ^= 0xCE4B;
    format!("{:X}", hash & 0xffff)
}

// ---------------------------------------------------------------- page setup

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FitMode {
    #[default]
    None,
    Width,
    Page,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PageSetup {
    pub sheet: u32,
    pub landscape: bool,
    pub fit: FitMode,
    pub gridlines: bool,
    pub headings: bool,
    /// Rows repeated at the top of every page (인쇄 제목).
    pub title_rows: Option<(i32, i32)>,
    /// Manual page breaks: a new page starts at these rows / columns.
    pub row_breaks: Vec<i32>,
    pub col_breaks: Vec<i32>,
    pub page_numbers: bool,
}

impl PageSetup {
    pub fn new(sheet: u32) -> Self {
        PageSetup {
            sheet,
            landscape: false,
            fit: FitMode::None,
            gridlines: false,
            headings: false,
            title_rows: None,
            row_breaks: Vec::new(),
            col_breaks: Vec::new(),
            page_numbers: false,
        }
    }

    pub fn is_default(&self) -> bool {
        *self == PageSetup::new(self.sheet)
    }
}

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

impl Workbook {
    // ------------------------------------------------------------ tables

    fn table_view(&self, t: &Table) -> Option<TableView> {
        let sheet = self.sheet_names().iter().position(|n| *n == t.sheet_name)? as u32;
        let (_, r0, c0, r1, c1) = parse_range(&t.reference)?;
        Some(TableView {
            name: t.name.clone(),
            sheet,
            range: CellRange { r0, c0, r1, c1 },
            style: t.style_info.name.clone().unwrap_or_else(|| "TableStyleMedium2".into()),
            stripes: t.style_info.show_row_stripes,
            header: t.header_row_count > 0,
            totals: t.totals_row_count > 0,
            columns: t.columns.iter().map(|c| c.name.clone()).collect(),
        })
    }

    pub fn tables_on(&self, sheet: u32) -> Vec<TableView> {
        let mut v: Vec<TableView> = self.model().workbook.tables.values().filter_map(|t| self.table_view(t)).filter(|t| t.sheet == sheet).collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    pub fn table_at(&self, sheet: u32, row: i32, col: i32) -> Option<TableView> {
        self.tables_on(sheet).into_iter().find(|t| t.range.contains(row, col))
    }

    fn next_table_name(&self) -> String {
        let tables = &self.model().workbook.tables;
        (1..).map(|i| format!("표{i}")).find(|n| !tables.keys().any(|k| k.eq_ignore_ascii_case(n))).unwrap_or_else(|| "표".into())
    }

    /// 표 서식 (Ctrl+T): makes `range` an Excel table with filter buttons.
    pub fn create_table(&mut self, sheet: u32, range: CellRange, has_header: bool, style: &str) -> Result<TableView> {
        self.guard(sheet, range, Guard::FormatCells)?;
        let mut r = range.normalized();
        if r.r0 == r.r1 && r.c0 == r.c1 {
            r = self.current_region(sheet, r.r0, r.c0)?;
        }
        if self.tables_on(sheet).iter().any(|t| !(t.range.r1 < r.r0 || t.range.r0 > r.r1 || t.range.c1 < r.c0 || t.range.c0 > r.c1)) {
            return Err(CellxError::Calc("표는 다른 표와 겹칠 수 없습니다".into()));
        }
        let start = self.begin();
        if !has_header {
            // Excel adds a header row of 열1, 열2 … above the data.
            self.um.insert_rows(sheet, r.r0, 1)?;
            self.shift_all_pub(sheet, Axis::Row, r.r0, 1);
            for (i, c) in (r.c0..=r.c1).enumerate() {
                self.write_input(sheet, r.r0, c, &format!("열{}", i + 1), true)?;
            }
            r.r1 += 1;
        }
        let mut names: Vec<String> = Vec::new();
        for (i, c) in (r.c0..=r.c1).enumerate() {
            let raw = self.model().get_formatted_cell_value(sheet, r.r0, c)?;
            let mut n = if raw.trim().is_empty() { format!("열{}", i + 1) } else { raw.trim().to_string() };
            let base = n.clone();
            let mut k = 2;
            while names.iter().any(|x| x.eq_ignore_ascii_case(&n)) {
                n = format!("{base}{k}");
                k += 1;
            }
            if raw.trim() != n {
                self.write_input(sheet, r.r0, c, &n, true)?;
            }
            names.push(n);
        }
        let name = self.next_table_name();
        if self.data().has_table(&name) {
            return Err(CellxError::Calc(format!("데이터 시트와 같은 이름은 쓸 수 없습니다: {name}")));
        }
        let sheet_name = self.sheet_names()[sheet as usize].clone();
        let table = Table {
            name: name.clone(),
            display_name: name.clone(),
            sheet_name,
            reference: range_a1(r),
            totals_row_count: 0,
            header_row_count: 1,
            header_row_dxf_id: None,
            data_dxf_id: None,
            totals_row_dxf_id: None,
            columns: names
                .iter()
                .enumerate()
                .map(|(i, n)| TableColumn { id: i as u32 + 1, name: n.clone(), ..TableColumn::default() })
                .collect(),
            style_info: TableStyleInfo {
                name: Some(style.to_string()),
                show_first_column: false,
                show_last_column: false,
                show_row_stripes: true,
                show_column_stripes: false,
            },
            has_filters: true,
        };
        self.model_mut().workbook.tables.insert(name.clone(), table);
        self.model_mut().refresh_parser_tables();
        if !self.filters.iter().any(|f| f.sheet == sheet) {
            self.filters.push(AutoFilter { sheet, range: r, columns: Vec::new() });
        }
        self.commit(start)?;
        self.table_at(sheet, r.r0, r.c0).ok_or_else(|| CellxError::Calc("표를 만들지 못했습니다".into()))
    }

    /// 표 디자인: name, style, banded rows, total row.
    pub fn update_table(&mut self, u: &TableUpdate) -> Result<RecalcStats> {
        let t = self
            .model()
            .workbook
            .tables
            .get(&u.name)
            .cloned()
            .ok_or_else(|| CellxError::Calc(format!("표가 없습니다: {}", u.name)))?;
        let view = self.table_view(&t).ok_or_else(|| CellxError::Calc("표의 시트를 찾을 수 없습니다".into()))?;
        let sheet = view.sheet;
        let new_name = u.new_name.trim();
        if new_name.is_empty() {
            return Err(CellxError::Calc("표 이름을 입력하세요".into()));
        }
        if !new_name.eq_ignore_ascii_case(&u.name) && self.model().workbook.tables.keys().any(|k| k.eq_ignore_ascii_case(new_name)) {
            return Err(CellxError::Calc(format!("이미 있는 표 이름입니다: {new_name}")));
        }
        let start = self.begin();
        let mut t2 = t.clone();
        t2.style_info.name = Some(u.style.clone());
        t2.style_info.show_row_stripes = u.stripes;
        let mut r = view.range;
        if u.totals && !view.totals {
            // 요약 행: a label and a SUBTOTAL of the last column, like Excel's default.
            r.r1 += 1;
            if !(r.c0..=r.c1).all(|c| self.input(sheet, r.r1, c).unwrap_or_default().is_empty()) {
                self.um.insert_rows(sheet, r.r1, 1)?;
                self.shift_all_pub(sheet, Axis::Row, r.r1, 1);
            }
            self.write_input(sheet, r.r1, r.c0, "요약", true)?;
            let last = t.columns.last().map(|c| c.name.clone()).unwrap_or_default();
            if r.c1 > r.c0 {
                self.write_input(sheet, r.r1, r.c1, &format!("=SUBTOTAL(109,{}[{}])", t.name, last), true)?;
            }
            t2.totals_row_count = 1;
            if let Some(c) = t2.columns.first_mut() {
                c.totals_row_label = Some("요약".into());
            }
            if let Some(c) = t2.columns.last_mut() {
                if r.c1 > r.c0 {
                    c.totals_row_function = Some("sum".into());
                }
            }
        } else if !u.totals && view.totals {
            self.um.range_clear_contents(&CellRange { r0: r.r1, c0: r.c0, r1: r.r1, c1: r.c1 }.area(sheet))?;
            r.r1 -= 1;
            t2.totals_row_count = 0;
            for c in &mut t2.columns {
                c.totals_row_label = None;
                c.totals_row_function = None;
            }
        }
        t2.reference = range_a1(r);
        if new_name != u.name {
            t2.name = new_name.to_string();
            t2.display_name = new_name.to_string();
            // Formulas that use the old name follow, as in Excel.
            let cells: Vec<(i32, i32)> = (0..self.sheet_names().len() as u32)
                .flat_map(|s| self.model().workbook.worksheet(s).map(|w| w.sheet_data.cells().map(|(r, c, _)| (r, c)).collect::<Vec<_>>()).unwrap_or_default().into_iter().map(move |(r, c)| (s, r, c)))
                .filter_map(|(s, r, c)| (s == sheet).then_some((r, c)))
                .collect();
            for (rr, cc) in cells {
                let inp = self.input(sheet, rr, cc)?;
                if inp.starts_with('=') && inp.contains(&format!("{}[", u.name)) {
                    let out = inp.replace(&format!("{}[", u.name), &format!("{new_name}["));
                    self.model_mut().workbook.tables.insert(t2.name.clone(), t2.clone());
                    self.model_mut().refresh_parser_tables();
                    self.write_input(sheet, rr, cc, &out, true)?;
                }
            }
        }
        let wb = &mut self.model_mut().workbook;
        wb.tables.remove(&u.name);
        wb.tables.insert(t2.name.clone(), t2);
        self.model_mut().refresh_parser_tables();
        if let Some(f) = self.filters.iter_mut().find(|f| f.sheet == sheet && f.range.r0 == r.r0 && f.range.c0 == r.c0) {
            // Filters cover the data, not the total row.
            f.range = CellRange { r1: if u.totals { r.r1 - 1 } else { r.r1 }, ..r };
        }
        self.commit(start)
    }

    /// 범위로 변환: the table goes, its look stays as ordinary formatting.
    pub fn convert_table_to_range(&mut self, name: &str) -> Result<RecalcStats> {
        let t = self.model().workbook.tables.get(name).cloned().ok_or_else(|| CellxError::Calc(format!("표가 없습니다: {name}")))?;
        let view = self.table_view(&t).ok_or_else(|| CellxError::Calc("표의 시트를 찾을 수 없습니다".into()))?;
        let start = self.begin();
        let pal = table_palette(&view.style);
        let r = view.range;
        let sheet = view.sheet;
        let header = CellRange { r0: r.r0, c0: r.c0, r1: r.r0, c1: r.c1 };
        if let Some(f) = pal.header_fill {
            self.um.update_range_style(&header.area(sheet), "fill.fg_color", f)?;
        }
        if let Some(f) = pal.header_font {
            self.um.update_range_style(&header.area(sheet), "font.color", f)?;
        }
        self.um.update_range_style(&header.area(sheet), "font.b", "true")?;
        if let (Some(s), true) = (pal.stripe, view.stripes) {
            let mut row = r.r0 + 1;
            while row <= r.r1 - view.totals as i32 {
                let band = CellRange { r0: row, c0: r.c0, r1: row, c1: r.c1 };
                self.um.update_range_style(&band.area(sheet), "fill.fg_color", s)?;
                row += 2;
            }
        }
        self.model_mut().workbook.tables.remove(name);
        self.model_mut().refresh_parser_tables();
        self.filters.retain(|f| !(f.sheet == sheet && f.range.r0 == r.r0 && f.range.c0 == r.c0));
        self.um.set_rows_hidden(sheet, r.r0, r.r1, false)?;
        self.commit(start)
    }

    /// The table look of a cell, under its own formatting.
    pub(crate) fn table_style_for(&self, tables: &[TableView], row: i32, col: i32, cs: &mut CellStyle) {
        let Some(t) = tables.iter().find(|t| t.range.contains(row, col)) else { return };
        let pal = table_palette(&t.style);
        let header = t.header && row == t.range.r0;
        let total = t.totals && row == t.range.r1;
        if header {
            if cs.fill.is_none() {
                cs.fill = pal.header_fill.map(str::to_string);
            }
            if cs.color.is_none() {
                cs.color = pal.header_font.map(str::to_string);
            }
            cs.bold = true;
            if pal.header_fill.is_none() && cs.border_bottom.is_none() {
                cs.border_bottom = Some(BorderView { style: "thin".into(), color: "#000000".into() });
            }
        } else if total {
            cs.bold = true;
            if cs.border_top.is_none() {
                cs.border_top = Some(BorderView { style: "double".into(), color: pal.header_fill.unwrap_or("#000000").into() });
            }
        } else {
            let band = (row - t.range.r0 - t.header as i32) % 2 == 0;
            if t.stripes && band && cs.fill.is_none() {
                cs.fill = pal.stripe.map(str::to_string);
            }
        }
        if let Some(b) = pal.border {
            for side in [&mut cs.border_top, &mut cs.border_bottom] {
                if side.is_none() {
                    *side = Some(BorderView { style: "thin".into(), color: b.into() });
                }
            }
        }
    }

    pub(crate) fn shift_tables(&mut self, sheet: u32, axis: Axis, at: i32, count: i32) {
        let name = self.sheet_names().get(sheet as usize).cloned().unwrap_or_default();
        let mut changed = false;
        for t in self.model_mut().workbook.tables.values_mut() {
            if t.sheet_name != name {
                continue;
            }
            let moved = refs::shift_refs(&format!("={}", t.reference), &name, &name, axis, at, count);
            let moved = moved.trim_start_matches('=').to_string();
            if moved != t.reference && !moved.contains("#REF!") {
                t.reference = moved;
                changed = true;
            }
        }
        if changed {
            self.model_mut().refresh_parser_tables();
        }
    }

    // ------------------------------------------------------------ protection

    pub fn protection(&self, sheet: u32) -> ProtectionView {
        let p = self.protections.iter().find(|p| p.sheet == sheet);
        ProtectionView {
            protected: p.is_some(),
            has_password: p.is_some_and(|p| p.password_hash.is_some()),
            options: p.map(|p| p.options.clone()).unwrap_or_default(),
            unlocked: self.unlocked.iter().filter(|(s, _)| *s == sheet).map(|(_, r)| *r).collect(),
        }
    }

    pub fn protect_sheet(&mut self, sheet: u32, password: Option<&str>, options: ProtectOptions) -> Result<()> {
        let start = self.begin();
        self.protections.retain(|p| p.sheet != sheet);
        self.protections.push(SheetProtection {
            sheet,
            password_hash: password.filter(|p| !p.is_empty()).map(excel_password_hash),
            options,
        });
        self.commit(start)?;
        Ok(())
    }

    pub fn unprotect_sheet(&mut self, sheet: u32, password: Option<&str>) -> Result<()> {
        if let Some(p) = self.protections.iter().find(|p| p.sheet == sheet) {
            if let Some(h) = &p.password_hash {
                let given = password.map(excel_password_hash);
                let same = |a: &str, b: &str| u32::from_str_radix(a, 16).ok() == u32::from_str_radix(b, 16).ok();
                if given.as_deref().map(|g| same(g, h)) != Some(true) {
                    return Err(CellxError::Calc("암호가 올바르지 않습니다.".into()));
                }
            }
        }
        let start = self.begin();
        self.protections.retain(|p| p.sheet != sheet);
        self.commit(start)?;
        Ok(())
    }

    /// 셀 서식 → 보호 → 잠금: locked cells cannot change once the sheet is protected.
    pub fn set_locked(&mut self, sheet: u32, range: CellRange, locked: bool) -> Result<RecalcStats> {
        self.guard(sheet, range, Guard::FormatCells)?;
        let r = range.normalized();
        let start = self.begin();
        let mut out: Vec<(u32, CellRange)> = Vec::new();
        for (s, u) in std::mem::take(&mut self.unlocked) {
            if s != sheet {
                out.push((s, u));
                continue;
            }
            for part in crate::datatools::subtract_range(u, r) {
                out.push((s, part));
            }
        }
        if !locked {
            out.push((sheet, r));
        }
        self.unlocked = out;
        self.commit(start)
    }

    pub fn is_locked(&self, sheet: u32, row: i32, col: i32) -> bool {
        !self.unlocked.iter().any(|(s, r)| *s == sheet && r.contains(row, col))
    }

    /// Refuses an edit the protection of `sheet` does not allow.
    pub(crate) fn guard(&self, sheet: u32, range: CellRange, what: Guard) -> Result<()> {
        let Some(p) = self.protections.iter().find(|p| p.sheet == sheet) else { return Ok(()) };
        let o = &p.options;
        let allowed = match what {
            Guard::Content => {
                let r = range.normalized();
                let area = (r.r1 - r.r0 + 1) as i64 * (r.c1 - r.c0 + 1) as i64;
                if area <= 200_000 {
                    (r.r0..=r.r1).all(|row| (r.c0..=r.c1).all(|c| !self.is_locked(sheet, row, c)))
                } else {
                    self.unlocked.iter().any(|(s, u)| *s == sheet && u.contains(r.r0, r.c0) && u.contains(r.r1, r.c1))
                }
            }
            Guard::FormatCells => o.format_cells,
            Guard::FormatColumns => o.format_columns,
            Guard::FormatRows => o.format_rows,
            Guard::InsertRows => o.insert_rows,
            Guard::InsertColumns => o.insert_columns,
            Guard::DeleteRows => o.delete_rows,
            Guard::DeleteColumns => o.delete_columns,
            Guard::Sort => o.sort,
            Guard::AutoFilter => o.auto_filter,
        };
        if allowed {
            Ok(())
        } else {
            Err(CellxError::Calc(PROTECTED.into()))
        }
    }

    pub(crate) fn shift_sheet_options(&mut self, sheet: u32, axis: Axis, at: i32, count: i32) {
        for (s, r) in &mut self.unlocked {
            if *s != sheet {
                continue;
            }
            let (a, b) = if axis == Axis::Row { (&mut r.r0, &mut r.r1) } else { (&mut r.c0, &mut r.c1) };
            let na = shift_index(*a, at, count).unwrap_or(at);
            let nb = shift_index(*b, at, count).unwrap_or(at + count.min(0) + 0).max(na.min(*b));
            *a = na;
            *b = nb.max(na);
        }
        for p in self.page_setups.iter_mut().filter(|p| p.sheet == sheet) {
            let list = if axis == Axis::Row { &mut p.row_breaks } else { &mut p.col_breaks };
            *list = list.iter().filter_map(|b| shift_index(*b, at, count)).collect();
            if axis == Axis::Row {
                if let Some((a, b)) = p.title_rows {
                    p.title_rows = match (shift_index(a, at, count), shift_index(b, at, count)) {
                        (Some(a), Some(b)) => Some((a, b)),
                        _ => None,
                    };
                }
            }
        }
    }

    pub(crate) fn sheet_options_after_delete(&mut self, sheet: u32) {
        self.protections.retain(|p| p.sheet != sheet);
        self.unlocked.retain(|(s, _)| *s != sheet);
        self.page_setups.retain(|p| p.sheet != sheet);
        for p in &mut self.protections {
            if p.sheet > sheet {
                p.sheet -= 1;
            }
        }
        for (s, _) in &mut self.unlocked {
            if *s > sheet {
                *s -= 1;
            }
        }
        for p in &mut self.page_setups {
            if p.sheet > sheet {
                p.sheet -= 1;
            }
        }
    }

    // ------------------------------------------------------------ page setup

    pub fn page_setup(&self, sheet: u32) -> PageSetup {
        self.page_setups.iter().find(|p| p.sheet == sheet).cloned().unwrap_or_else(|| PageSetup::new(sheet))
    }

    pub fn set_page_setup(&mut self, setup: PageSetup) -> Result<()> {
        let start = self.begin();
        let sheet = setup.sheet;
        self.page_setups.retain(|p| p.sheet != sheet);
        let mut s = setup;
        s.row_breaks.sort_unstable();
        s.row_breaks.dedup();
        s.col_breaks.sort_unstable();
        s.col_breaks.dedup();
        if !s.is_default() {
            self.page_setups.push(s);
        }
        self.commit(start)?;
        Ok(())
    }

    /// 나누기 → 페이지 나누기 삽입: before the active row (and column), as in Excel.
    pub fn insert_page_break(&mut self, sheet: u32, row: i32, col: i32) -> Result<()> {
        let mut s = self.page_setup(sheet);
        if row > 1 {
            s.row_breaks.push(row);
        }
        if col > 1 {
            s.col_breaks.push(col);
        }
        self.set_page_setup(s)
    }

    pub fn remove_page_break(&mut self, sheet: u32, row: i32, col: i32) -> Result<()> {
        let mut s = self.page_setup(sheet);
        s.row_breaks.retain(|b| *b != row);
        s.col_breaks.retain(|b| *b != col);
        self.set_page_setup(s)
    }

    pub fn reset_page_breaks(&mut self, sheet: u32) -> Result<()> {
        let mut s = self.page_setup(sheet);
        s.row_breaks.clear();
        s.col_breaks.clear();
        self.set_page_setup(s)
    }

    // ------------------------------------------------------------ custom validation

    /// Evaluates a custom validation formula for an entry, as if typed in the cell:
    /// the entry goes into the cell and the formula into a scratch cell, both without
    /// history, and both are put back afterwards.
    pub(crate) fn custom_rule_holds(&mut self, sheet: u32, row: i32, col: i32, input: &str, v: &Validation) -> bool {
        let Some((_, r0, c0, _, _)) = v.range.split_whitespace().next().and_then(parse_range) else { return true };
        let formula = refs::offset_refs(&format!("={}", v.formula1.trim_start_matches('=')), row - r0, col - c0);
        let original = self.input(sheet, row, col).unwrap_or_default();
        let (sr, sc) = (refs::LAST_ROW, refs::LAST_COL);
        let scratch_original = self.input(sheet, sr, sc).unwrap_or_default();
        let mut ok = true;
        if self.model_mut().set_user_input(sheet, row, col, input.to_string()).is_ok()
            && self.model_mut().set_user_input(sheet, sr, sc, formula).is_ok()
        {
            self.model_mut().evaluate();
            ok = match self.value(sheet, sr, sc) {
                Ok(Scalar::Bool(b)) => b,
                Ok(Scalar::Number(n)) => n != 0.0,
                _ => false,
            };
        }
        let restore = |wb: &mut Workbook, r: i32, c: i32, text: &str| {
            if text.is_empty() {
                let _ = wb.model_mut().range_clear_contents(&CellRange { r0: r, c0: c, r1: r, c1: c }.area(sheet));
            } else {
                let _ = wb.model_mut().set_user_input(sheet, r, c, text.to_string());
            }
        };
        restore(self, sr, sc, &scratch_original);
        // Bridge cells hold their result formula in the model, the rest their input.
        let model_original = if self.bridges.contains_key(&(sheet, row, col)) {
            self.model().get_cell_formula(sheet, row, col).ok().flatten().unwrap_or(original)
        } else {
            original
        };
        restore(self, row, col, &model_original);
        self.model_mut().evaluate();
        ok
    }
}

/// `A1` names for xlsx.
pub fn a1(r: CellRange) -> String {
    format!("{}{}:{}{}", column_name(r.c0), r.r0, column_name(r.c1), r.r1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datatools::{ErrorStyle, ValidationKind, ValidationOp};

    fn book() -> Workbook {
        let mut wb = Workbook::new().unwrap();
        let rows = [["지역", "금액"], ["서울", "100"], ["부산", "50"], ["대구", "70"]];
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
    fn tables_with_structured_refs_and_totals() {
        let mut wb = book();
        let t = wb.create_table(0, CellRange { r0: 1, c0: 1, r1: 1, c1: 1 }, true, "TableStyleMedium2").unwrap();
        assert_eq!(t.name, "표1");
        assert_eq!(t.range, CellRange { r0: 1, c0: 1, r1: 4, c1: 2 });
        wb.set_input(0, 6, 1, "=SUM(표1[금액])").unwrap();
        assert_eq!(wb.value(0, 6, 1).unwrap(), Scalar::Number(220.0));
        // Table look: header fill, banded rows.
        let vp = wb.viewport(0, 1, 1, 4, 2).unwrap();
        let fill = |r: i32| vp.cells.iter().find(|c| c.row == r && c.col == 2).and_then(|c| c.style.as_ref()).and_then(|s| s.fill.clone());
        assert_eq!(fill(1).as_deref(), Some("#4472C4"));
        assert_eq!(fill(2).as_deref(), Some("#D9E1F2"));
        assert_eq!(fill(3), None);
        wb.update_table(&TableUpdate { name: "표1".into(), new_name: "매출".into(), style: "TableStyleMedium7".into(), stripes: true, totals: true })
            .unwrap();
        // IronCalc keeps structured references as the ranges they stand for.
        assert_eq!(wb.value(0, 6, 1).unwrap(), Scalar::Number(220.0));
        assert_eq!(wb.model().get_formatted_cell_value(0, 5, 2).unwrap(), "220");
        // Rows inserted above move the table.
        wb.insert_rows(0, 1, 2).unwrap();
        assert_eq!(wb.table_at(0, 3, 1).unwrap().range, CellRange { r0: 3, c0: 1, r1: 7, c1: 2 });
        // xlsx keeps the table.
        let dir = crate::project::new_work_dir().unwrap();
        let path = dir.join("t.xlsx");
        wb.save_xlsx(&path).unwrap();
        let back = Workbook::open_xlsx(&path).unwrap();
        let bt = back.table_at(0, 3, 1).unwrap();
        assert_eq!((bt.name.as_str(), bt.style.as_str(), bt.totals), ("매출", "TableStyleMedium7", true));
        assert!(back.auto_filter(0).is_some());
        wb.convert_table_to_range("매출").unwrap();
        assert!(wb.tables_on(0).is_empty());
    }

    #[test]
    fn protection_blocks_locked_cells() {
        let mut wb = book();
        wb.set_locked(0, CellRange { r0: 2, c0: 2, r1: 4, c1: 2 }, false).unwrap();
        wb.protect_sheet(0, Some("abc"), ProtectOptions { format_columns: true, ..Default::default() }).unwrap();
        assert!(wb.set_input(0, 2, 1, "x").is_err());
        assert!(wb.set_input(0, 2, 2, "5").is_ok());
        assert!(wb.insert_rows(0, 1, 1).is_err());
        assert!(wb.set_style(0, CellRange { r0: 2, c0: 2, r1: 2, c1: 2 }, "font.b", "true").is_err());
        assert!(wb.set_column_width(0, 1, 1, 100.0).is_ok());
        assert!(wb.unprotect_sheet(0, Some("nope")).is_err());
        wb.unprotect_sheet(0, Some("abc")).unwrap();
        assert!(wb.set_input(0, 2, 1, "x").is_ok());
        // Excel's hash of "abc".
        assert_eq!(excel_password_hash("abc"), "CC1A");
    }

    #[test]
    fn page_breaks_shift_and_save() {
        let mut wb = book();
        wb.insert_page_break(0, 3, 1).unwrap();
        let mut s = wb.page_setup(0);
        s.landscape = true;
        s.title_rows = Some((1, 1));
        s.fit = FitMode::Width;
        s.gridlines = true;
        wb.set_page_setup(s).unwrap();
        wb.insert_rows(0, 2, 1).unwrap();
        assert_eq!(wb.page_setup(0).row_breaks, vec![4]);
        let dir = crate::project::new_work_dir().unwrap();
        let path = dir.join("p.xlsx");
        wb.save_xlsx(&path).unwrap();
        let back = Workbook::open_xlsx(&path).unwrap();
        let p = back.page_setup(0);
        assert_eq!((p.landscape, p.fit, p.gridlines, p.title_rows, p.row_breaks.clone()), (true, FitMode::Width, true, Some((1, 1)), vec![4]));
        assert!(back.names().is_empty());
    }

    #[test]
    fn custom_validation_formula() {
        let mut wb = book();
        let v = Validation {
            sheet: 0,
            range: String::new(),
            kind: ValidationKind::Custom,
            op: ValidationOp::Between,
            // Unique names in column A.
            formula1: "COUNTIF($A$2:$A$10,A2)=1".into(),
            formula2: String::new(),
            allow_blank: true,
            in_cell_dropdown: false,
            show_input: false,
            input_title: String::new(),
            input_message: String::new(),
            show_error: true,
            error_style: ErrorStyle::Stop,
            error_title: String::new(),
            error_message: String::new(),
        };
        wb.set_validation(0, CellRange { r0: 2, c0: 1, r1: 10, c1: 1 }, v).unwrap();
        assert!(wb.validate_input(0, 5, 1, "서울").is_some());
        assert!(wb.validate_input(0, 5, 1, "광주").is_none());
        assert_eq!(wb.input(0, 5, 1).unwrap(), "");
        assert_eq!(wb.input(0, 2, 1).unwrap(), "서울");
    }
}
