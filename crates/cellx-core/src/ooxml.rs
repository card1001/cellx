//! xlsx parts IronCalc does not write: memos (legacy comments with their VML) and charts.
//!
//! [`add_extras`] rewrites an xlsx produced by IronCalc, adding the parts and the
//! relationships to them. [`read_charts`] reads charts back from any xlsx.

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use zip::write::SimpleFileOptions;

use crate::datatools::{AutoFilter, ColumnFilter, ErrorStyle, FilterCondition, FilterOp, Validation, ValidationKind, ValidationOp};
use crate::error::{CellxError, Result};
use crate::extras::{ChartData, ChartKind};
use crate::sheetopts::{FitMode, PageSetup, SheetProtection};
use crate::workbook::CellRange;
use crate::refs::column_name;

/// EMU per pixel at 96 dpi.
pub const EMU_PER_PX: f64 = 9525.0;

pub struct MemoXml {
    pub row: i32,
    pub col: i32,
    pub text: String,
    pub author: String,
}

/// A cell position with an offset in EMU, 0-based as in DrawingML.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marker {
    pub col: i32,
    pub col_off: i64,
    pub row: i32,
    pub row_off: i64,
}

pub struct ChartXml {
    /// Written as it is instead of `chart_xml`, with its parts.
    pub original: Option<std::sync::Arc<crate::extras::OriginalChart>>,
    pub trendline: bool,
    pub kind: ChartKind,
    pub legend: bool,
    pub data: ChartData,
    /// Name of the sheet the data comes from (for references).
    pub data_sheet: String,
    pub from: Marker,
    pub to: Marker,
}

/// A picture for the drawing part of a sheet.
pub struct PictureXml {
    pub name: String,
    /// Extension of its media part: png, jpeg, gif or bmp.
    pub ext: &'static str,
    pub data: std::sync::Arc<Vec<u8>>,
    pub from: Marker,
    pub to: Marker,
    /// Size in EMU.
    pub cx: i64,
    pub cy: i64,
}

/// A shape or text box for the drawing part of a sheet.
pub struct ShapeXml {
    pub from: Marker,
    pub to: Marker,
    /// The `<xdr:sp>` element; its `cNvPr id="0"` is numbered when written.
    pub sp: String,
}

/// A table slicer for the drawing, slicer and slicer cache parts.
pub struct SlicerXml {
    pub name: String,
    /// The cache's name, also a defined name of the workbook.
    pub cache: String,
    pub caption: String,
    pub column: String,
    pub table: String,
    /// The `tableColumn` id of its column.
    pub column_id: u32,
    pub columns: u32,
    pub from: Marker,
    pub to: Marker,
    pub cx: i64,
    pub cy: i64,
}

#[derive(Default)]
pub struct SheetParts {
    pub memos: Vec<MemoXml>,
    pub charts: Vec<ChartXml>,
    pub pictures: Vec<PictureXml>,
    pub shapes: Vec<ShapeXml>,
    pub slicers: Vec<SlicerXml>,
    /// `<scenarios>` of the sheet.
    pub scenarios: String,
    /// (first result cell, `<f t="dataTable" …/>`) of each data table.
    pub data_tables: Vec<(String, String)>,
    /// `<customSheetViews>` of the sheet.
    pub views: String,
    /// (cache index, pivot table definition) of the sheet's pivot tables.
    pub pivots: Vec<(usize, String)>,
    /// `<ext>` elements for the worksheet's `<extLst>` (sparklines).
    pub ext: String,
    pub filter: Option<AutoFilter>,
    pub validations: Vec<Validation>,
    pub outline_rows: Vec<(i32, u8)>,
    pub outline_cols: Vec<(i32, u8)>,
    /// Hidden rows (written even when they hold no cells).
    pub hidden_rows: Vec<i32>,
    /// IronCalc's column records, to rewrite `<cols>` with outline levels.
    pub cols: Vec<ironcalc_base::types::Col>,
    pub tables: Vec<TableXml>,
    pub protection: Option<SheetProtection>,
    pub unlocked: Vec<CellRange>,
    pub page: Option<PageSetup>,
    /// The sheet tab's color, as IronCalc keeps it.
    pub tab_color: Option<ironcalc_base::types::Color>,
}

pub struct TableXml {
    pub table: ironcalc_base::types::Table,
    /// The AutoFilter of the table's data, when the sheet's filter is the table's.
    pub filter: Option<AutoFilter>,
}

impl SheetParts {
    fn is_empty(&self) -> bool {
        self.memos.is_empty()
            && self.charts.is_empty()
            && self.pictures.is_empty()
            && self.shapes.is_empty()
            && self.ext.is_empty()
            && self.slicers.is_empty()
            && self.scenarios.is_empty()
            && self.data_tables.is_empty()
            && self.views.is_empty()
            && self.pivots.is_empty()
            && self.filter.is_none()
            && self.validations.is_empty()
            && self.outline_rows.is_empty()
            && self.outline_cols.is_empty()
            && self.tables.is_empty()
            && self.protection.is_none()
            && self.unlocked.is_empty()
            && self.page.is_none()
            && self.tab_color.is_none()
    }
}

/// Adds `outlineLevel` to rows, writing the rows that hold no cells.
fn patch_rows(xml: &str, levels: &[(i32, u8)], hidden: &[i32]) -> String {
    use std::collections::BTreeMap;
    let mut want: BTreeMap<i32, (u8, bool)> = BTreeMap::new();
    for (r, l) in levels {
        want.entry(*r).or_insert((0, false)).0 = *l;
    }
    for r in hidden {
        want.entry(*r).or_insert((0, false)).1 = true;
    }
    if want.is_empty() {
        return xml.to_string();
    }
    let (Some(start), Some(end)) = (xml.find("<sheetData"), xml.find("</sheetData>").or_else(|| xml.find("<sheetData/>"))) else {
        return xml.to_string();
    };
    let self_closing = xml[start..].starts_with("<sheetData/>");
    let body_start = if self_closing { start + "<sheetData/>".len() } else { xml[start..].find('>').map(|i| start + i + 1).unwrap_or(start) };
    let body = if self_closing { "" } else { &xml[body_start..end] };
    // Existing rows: split the body at each `<row `.
    let mut pieces: Vec<(i32, String)> = Vec::new();
    let mut rest = body;
    while let Some(i) = rest.find("<row ") {
        let after = &rest[i..];
        let close = after.find("</row>").map(|k| k + 6).or_else(|| after.find("/>").map(|k| k + 2)).unwrap_or(after.len());
        let elem = &after[..close];
        let r = elem
            .split("r=\"")
            .nth(1)
            .and_then(|x| x.split('"').next())
            .and_then(|x| x.parse::<i32>().ok())
            .unwrap_or(0);
        pieces.push((r, elem.to_string()));
        rest = &after[close..];
    }
    for (r, (level, hide)) in &want {
        let extra = format!(
            "{}{}",
            if *level > 0 { format!(r#" outlineLevel="{level}""#) } else { String::new() },
            if *hide { r#" hidden="1""# } else { "" }
        );
        match pieces.iter_mut().find(|(pr, _)| pr == r) {
            Some((_, elem)) => {
                let head_end = elem.find('>').unwrap_or(elem.len());
                let head = &elem[..head_end];
                let mut add = String::new();
                if *level > 0 && !head.contains("outlineLevel") {
                    add.push_str(&format!(r#" outlineLevel="{level}""#));
                }
                if *hide && !head.contains("hidden=") {
                    add.push_str(r#" hidden="1""#);
                }
                let at = if head.ends_with('/') { head_end - 1 } else { head_end };
                elem.insert_str(at, &add);
            }
            None => pieces.push((*r, format!(r#"<row r="{r}"{extra}/>"#))),
        }
    }
    pieces.sort_by_key(|(r, _)| *r);
    let new_body: String = pieces.into_iter().map(|(_, e)| e).collect();
    let tail_start = if self_closing { start + "<sheetData/>".len() } else { end + "</sheetData>".len() };
    format!("{}<sheetData>{new_body}</sheetData>{}", &xml[..start], &xml[tail_start..])
}

/// Rewrites `<cols>` with outline levels on the grouped columns.
fn patch_cols(xml: &str, cols: &[ironcalc_base::types::Col], levels: &[(i32, u8)]) -> String {
    if levels.is_empty() {
        return xml.to_string();
    }
    #[derive(Clone)]
    struct C {
        min: i32,
        max: i32,
        width: f64,
        custom: bool,
        style: Option<i32>,
        hidden: bool,
        level: u8,
    }
    let mut list: Vec<C> = cols
        .iter()
        .map(|c| C { min: c.min, max: c.max, width: c.width, custom: c.custom_width, style: c.style, hidden: c.hidden, level: 0 })
        .collect();
    for (col, level) in levels {
        match list.iter().position(|c| c.min <= *col && *col <= c.max) {
            Some(i) => {
                let c = list.remove(i);
                if c.min < *col {
                    list.push(C { max: col - 1, ..c.clone() });
                }
                if c.max > *col {
                    list.push(C { min: col + 1, ..c.clone() });
                }
                list.push(C { min: *col, max: *col, level: *level, ..c });
            }
            None => list.push(C { min: *col, max: *col, width: 8.43, custom: false, style: None, hidden: false, level: *level }),
        }
    }
    list.sort_by_key(|c| c.min);
    let body: String = list
        .iter()
        .map(|c| {
            format!(
                r#"<col min="{}" max="{}" width="{}" customWidth="{}"{}{}{}/>"#,
                c.min,
                c.max,
                c.width,
                c.custom as u8,
                c.style.map(|s| format!(r#" style="{s}""#)).unwrap_or_default(),
                if c.hidden { r#" hidden="1""# } else { "" },
                if c.level > 0 { format!(r#" outlineLevel="{}""#, c.level) } else { String::new() }
            )
        })
        .collect();
    let new_cols = format!("<cols>{body}</cols>");
    if let (Some(a), Some(b)) = (xml.find("<cols>"), xml.find("</cols>")) {
        format!("{}{new_cols}{}", &xml[..a], &xml[b + "</cols>".len()..])
    } else if let Some(a) = xml.find("<sheetData") {
        format!("{}{new_cols}{}", &xml[..a], &xml[a..])
    } else {
        xml.to_string()
    }
}

fn protection_xml(p: &SheetProtection, unlocked: &[CellRange]) -> String {
    let o = &p.options;
    // In xlsx a "1" means the action is *protected*.
    let flag = |allowed: bool| if allowed { "0" } else { "1" };
    let mut xml = format!(
        r#"<sheetProtection{} sheet="1" objects="1" scenarios="1" formatCells="{}" formatColumns="{}" formatRows="{}" insertColumns="{}" insertRows="{}" deleteColumns="{}" deleteRows="{}" sort="{}" autoFilter="{}"/>"#,
        p.password_hash.as_ref().map(|h| format!(r#" password="{h}""#)).unwrap_or_default(),
        flag(o.format_cells),
        flag(o.format_columns),
        flag(o.format_rows),
        flag(o.insert_columns),
        flag(o.insert_rows),
        flag(o.delete_columns),
        flag(o.delete_rows),
        flag(o.sort),
        flag(o.auto_filter),
    );
    if !unlocked.is_empty() {
        let items: String = unlocked
            .iter()
            .enumerate()
            .map(|(i, r)| format!(r#"<protectedRange sqref="{}" name="편집 가능 {}"/>"#, a1(*r), i + 1))
            .collect();
        xml.push_str(&format!("<protectedRanges>{items}</protectedRanges>"));
    }
    xml
}

fn page_xml(p: &PageSetup) -> String {
    let mut xml = String::new();
    if p.gridlines || p.headings {
        xml.push_str(&format!(
            "<printOptions{}{}/>",
            if p.gridlines { r#" gridLines="1""# } else { "" },
            if p.headings { r#" headings="1""# } else { "" }
        ));
    }
    xml.push_str(r#"<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/>"#);
    let fit = match p.fit {
        FitMode::None => String::new(),
        FitMode::Width => r#" fitToWidth="1" fitToHeight="0""#.into(),
        FitMode::Page => r#" fitToWidth="1" fitToHeight="1""#.into(),
    };
    xml.push_str(&format!(r#"<pageSetup paperSize="9" orientation="{}"{fit}/>"#, if p.landscape { "landscape" } else { "portrait" }));
    let header = crate::sheetopts::header_footer_code(&p.header);
    let footer = crate::sheetopts::header_footer_code(&p.effective_footer());
    if !header.is_empty() || !footer.is_empty() {
        xml.push_str("<headerFooter>");
        if !header.is_empty() {
            xml.push_str(&format!("<oddHeader>{}</oddHeader>", esc(&header)));
        }
        if !footer.is_empty() {
            xml.push_str(&format!("<oddFooter>{}</oddFooter>", esc(&footer)));
        }
        xml.push_str("</headerFooter>");
    }
    if !p.row_breaks.is_empty() {
        let b: String = p.row_breaks.iter().map(|r| format!(r#"<brk id="{}" max="16383" man="1"/>"#, r - 1)).collect();
        xml.push_str(&format!(r#"<rowBreaks count="{0}" manualBreakCount="{0}">{b}</rowBreaks>"#, p.row_breaks.len()));
    }
    if !p.col_breaks.is_empty() {
        let b: String = p.col_breaks.iter().map(|c| format!(r#"<brk id="{}" max="1048575" man="1"/>"#, c - 1)).collect();
        xml.push_str(&format!(r#"<colBreaks count="{0}" manualBreakCount="{0}">{b}</colBreaks>"#, p.col_breaks.len()));
    }
    xml
}

fn table_xml(id: usize, t: &TableXml) -> String {
    let tb = &t.table;
    let filter_ref = crate::extras::parse_range(&tb.reference).map(|(_, r0, c0, r1, c1)| CellRange {
        r0,
        c0,
        r1: r1 - tb.totals_row_count as i32,
        c1,
    });
    let auto = match (&t.filter, filter_ref) {
        (Some(f), Some(r)) => {
            let mut f = f.clone();
            f.range = r;
            auto_filter_xml(&f)
        }
        (None, Some(r)) if tb.has_filters => format!(r#"<autoFilter ref="{}"/>"#, a1(r)),
        _ => String::new(),
    };
    let cols: String = tb
        .columns
        .iter()
        .map(|c| {
            format!(
                r#"<tableColumn id="{}" name="{}"{}{}/>"#,
                c.id,
                esc(&c.name),
                c.totals_row_label.as_ref().map(|l| format!(r#" totalsRowLabel="{}""#, esc(l))).unwrap_or_default(),
                c.totals_row_function.as_ref().map(|f| format!(r#" totalsRowFunction="{}""#, esc(f))).unwrap_or_default(),
            )
        })
        .collect();
    let si = &tb.style_info;
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="{id}" name="{}" displayName="{}" ref="{}"{}{}<tableColumns count="{}">{cols}</tableColumns><tableStyleInfo name="{}" showFirstColumn="{}" showLastColumn="{}" showRowStripes="{}" showColumnStripes="{}"/></table>"#,
        esc(&tb.name),
        esc(&tb.display_name),
        esc(&tb.reference),
        if tb.totals_row_count > 0 { format!(r#" totalsRowCount="{}""#, tb.totals_row_count) } else { String::new() },
        format!(">{auto}"),
        tb.columns.len(),
        esc(si.name.as_deref().unwrap_or("TableStyleMedium2")),
        si.show_first_column as u8,
        si.show_last_column as u8,
        si.show_row_stripes as u8,
        si.show_column_stripes as u8,
    )
}

fn a1(r: CellRange) -> String {
    format!("{}{}:{}{}", column_name(r.c0), r.r0, column_name(r.c1), r.r1)
}

pub(crate) fn auto_filter_xml(f: &AutoFilter) -> String {
    let mut cols = String::new();
    for cf in &f.columns {
        let id = cf.col - f.range.c0;
        if id < 0 {
            continue;
        }
        let mut inner = String::new();
        if let Some(vals) = &cf.values {
            let blank = vals.iter().any(|v| v.trim().is_empty());
            let items: String = vals.iter().filter(|v| !v.trim().is_empty()).map(|v| format!(r#"<filter val="{}"/>"#, esc(v))).collect();
            inner.push_str(&format!("<filters{}>{items}</filters>", if blank { r#" blank="1""# } else { "" }));
        } else if !cf.conditions.is_empty() {
            let items: String = cf
                .conditions
                .iter()
                .map(|c| {
                    let (op, val) = match c.op {
                        FilterOp::Equal => ("equal", c.value.clone()),
                        FilterOp::NotEqual => ("notEqual", c.value.clone()),
                        FilterOp::GreaterThan => ("greaterThan", c.value.clone()),
                        FilterOp::GreaterThanOrEqual => ("greaterThanOrEqual", c.value.clone()),
                        FilterOp::LessThan => ("lessThan", c.value.clone()),
                        FilterOp::LessThanOrEqual => ("lessThanOrEqual", c.value.clone()),
                        FilterOp::Contains => ("equal", format!("*{}*", c.value)),
                        FilterOp::NotContains => ("notEqual", format!("*{}*", c.value)),
                        FilterOp::BeginsWith => ("equal", format!("{}*", c.value)),
                        FilterOp::EndsWith => ("equal", format!("*{}", c.value)),
                    };
                    format!(r#"<customFilter operator="{op}" val="{}"/>"#, esc(&val))
                })
                .collect();
            inner.push_str(&format!("<customFilters{}>{items}</customFilters>", if cf.and && cf.conditions.len() > 1 { r#" and="1""# } else { "" }));
        }
        if !inner.is_empty() {
            cols.push_str(&format!(r#"<filterColumn colId="{id}">{inner}</filterColumn>"#));
        }
    }
    format!(r#"<autoFilter ref="{}">{cols}</autoFilter>"#, a1(f.range))
}

fn validations_xml(vs: &[Validation]) -> String {
    let items: String = vs
        .iter()
        .map(|v| {
            let kind = match v.kind {
                ValidationKind::Any => "none",
                ValidationKind::WholeNumber => "whole",
                ValidationKind::Decimal => "decimal",
                ValidationKind::List => "list",
                ValidationKind::Date => "date",
                ValidationKind::Time => "time",
                ValidationKind::TextLength => "textLength",
                ValidationKind::Custom => "custom",
            };
            let op = match v.op {
                ValidationOp::Between => "between",
                ValidationOp::NotBetween => "notBetween",
                ValidationOp::Equal => "equal",
                ValidationOp::NotEqual => "notEqual",
                ValidationOp::GreaterThan => "greaterThan",
                ValidationOp::LessThan => "lessThan",
                ValidationOp::GreaterThanOrEqual => "greaterThanOrEqual",
                ValidationOp::LessThanOrEqual => "lessThanOrEqual",
            };
            let style = match v.error_style {
                ErrorStyle::Stop => "stop",
                ErrorStyle::Warning => "warning",
                ErrorStyle::Information => "information",
            };
            let uses_op = !matches!(v.kind, ValidationKind::List | ValidationKind::Custom | ValidationKind::Any);
            let f2 = if uses_op && matches!(v.op, ValidationOp::Between | ValidationOp::NotBetween) && !v.formula2.is_empty() {
                format!("<formula2>{}</formula2>", esc(&v.formula2))
            } else {
                String::new()
            };
            let f1 = if v.formula1.is_empty() { String::new() } else { format!("<formula1>{}</formula1>", esc(&v.formula1)) };
            format!(
                r#"<dataValidation type="{kind}" errorStyle="{style}"{op_attr} allowBlank="{}" showDropDown="{}" showInputMessage="{}" showErrorMessage="{}" errorTitle="{}" error="{}" promptTitle="{}" prompt="{}" sqref="{}">{f1}{f2}</dataValidation>"#,
                v.allow_blank as u8,
                // In xlsx showDropDown="1" *hides* the in-cell arrow.
                (!v.in_cell_dropdown) as u8,
                v.show_input as u8,
                v.show_error as u8,
                esc(&v.error_title),
                esc(&v.error_message),
                esc(&v.input_title),
                esc(&v.input_message),
                esc(&v.range),
                op_attr = if uses_op { format!(r#" operator="{op}""#) } else { String::new() },
            )
        })
        .collect();
    format!(r#"<dataValidations count="{}">{items}</dataValidations>"#, vs.len())
}

fn zerr<T>(r: std::result::Result<T, zip::result::ZipError>) -> Result<T> {
    r.map_err(|e| CellxError::Xlsx(e.to_string()))
}

/// Where the worksheet's own `<extLst>` starts (its last child), if it has one —
/// not an extLst inside a rule or a custom view.
fn top_ext_start(xml: &str) -> Option<usize> {
    let end = xml.rfind("</worksheet>")?;
    if !xml[..end].trim_end().ends_with("</extLst>") {
        return None;
    }
    let close = xml[..end].rfind("</extLst>")?;
    let mut depth = 1;
    let mut at = close;
    while depth > 0 {
        let open = xml[..at].rfind("<extLst")?;
        let inner_close = xml[..at].rfind("</extLst>");
        match inner_close {
            Some(c) if c > open => {
                depth += 1;
                at = c;
            }
            _ => {
                depth -= 1;
                at = open;
            }
        }
    }
    Some(at)
}

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn sheet_ref(sheet: &str) -> String {
    if sheet.chars().all(|c| c.is_alphanumeric() || c == '_') && !sheet.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        sheet.to_string()
    } else {
        format!("'{}'", sheet.replace('\'', "''"))
    }
}

fn abs_range(sheet: &str, (a, b): ((i32, i32), (i32, i32))) -> String {
    let cell = |(r, c): (i32, i32)| format!("${}${}", column_name(c), r);
    if a == b {
        format!("{}!{}", sheet_ref(sheet), cell(a))
    } else {
        format!("{}!{}:{}", sheet_ref(sheet), cell(a), cell(b))
    }
}

/// What the workbook part gets besides its sheets.
#[derive(Default)]
pub struct BookParts {
    /// `<workbookProtection>`.
    pub protection: String,
    /// `<customWorkbookViews>`.
    pub views: String,
    /// Pivot cache definitions, in the order the tables refer to them.
    pub pivot_caches: Vec<String>,
}

pub(crate) fn workbook_xml(xlsx: &[u8]) -> Option<String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(xlsx)).ok()?;
    read_part(&mut zip, "xl/workbook.xml")
}

/// Adds memos and charts (`parts[i]` belongs to sheet i+1) to an xlsx made by IronCalc.
pub fn add_extras(xlsx: &[u8], parts: &[SheetParts], sheet_names: &[String]) -> Result<Vec<u8>> {
    add_extras_with(xlsx, parts, sheet_names, &BookParts::default())
}

pub fn add_extras_with(xlsx: &[u8], parts: &[SheetParts], sheet_names: &[String], book: &BookParts) -> Result<Vec<u8>> {
    if parts.iter().all(SheetParts::is_empty) && book.protection.is_empty() && book.views.is_empty() && book.pivot_caches.is_empty() {
        return Ok(xlsx.to_vec());
    }
    let mut zin = zerr(zip::ZipArchive::new(Cursor::new(xlsx)))?;
    // Only the parts that may change are read; the others are copied as they
    // are, still compressed, when the package is written again.
    let mut order: Vec<String> = Vec::new();
    for i in 0..zin.len() {
        let f = zerr(zin.by_index_raw(i))?;
        if !f.is_dir() {
            order.push(zerr(f.name())?.to_string());
        }
    }
    let sheets: std::collections::HashSet<String> = parts
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.is_empty())
        .flat_map(|(i, _)| {
            [format!("xl/worksheets/sheet{}.xml", i + 1), format!("xl/worksheets/_rels/sheet{}.xml.rels", i + 1)]
        })
        .collect();
    let wanted = |name: &str| {
        name == "[Content_Types].xml" || name == "xl/workbook.xml" || name == "xl/_rels/workbook.xml.rels" || sheets.contains(name)
    };
    let mut map: HashMap<String, Vec<u8>> = HashMap::new();
    for name in order.iter().filter(|n| wanted(n)) {
        let mut f = zerr(zin.by_name(name))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        map.insert(name.clone(), buf);
    }
    let loaded: std::collections::HashSet<String> = map.keys().cloned().collect();
    let mut changed: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut put = |map: &mut HashMap<String, Vec<u8>>, order: &mut Vec<String>, name: String, data: Vec<u8>| {
        if !order.contains(&name) {
            order.push(name.clone());
        }
        changed.insert(name.clone());
        map.insert(name, data);
    };

    let mut overrides = String::new();
    let mut chart_no = 0;
    let mut drawing_no = 0;
    let mut memo_no = 0;
    let mut table_no = 0;
    let mut image_no = 0;
    let mut media_types: Vec<&'static str> = Vec::new();
    // Table ids as written, for slicer caches; and the caches to write.
    let mut table_ids: HashMap<String, usize> = HashMap::new();
    let mut caches: Vec<&SlicerXml> = Vec::new();
    let mut slicer_no = 0;
    let mut pivot_no = 0;
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() {
            continue;
        }
        let sheet_path = format!("xl/worksheets/sheet{}.xml", i + 1);
        let rels_path = format!("xl/worksheets/_rels/sheet{}.xml.rels", i + 1);
        let Some(sheet_xml) = map.get(&sheet_path).map(|b| String::from_utf8_lossy(b).to_string()) else {
            continue;
        };
        let mut rels = Vec::new();
        let mut tags = String::new();

        for (cache, table) in &p.pivots {
            pivot_no += 1;
            put(&mut map, &mut order, format!("xl/pivotTables/pivotTable{pivot_no}.xml"), table.clone().into_bytes());
            put(
                &mut map,
                &mut order,
                format!("xl/pivotTables/_rels/pivotTable{pivot_no}.xml.rels"),
                relationships(&[format!(
                    r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition" Target="../pivotCache/pivotCacheDefinition{}.xml"/>"#,
                    cache + 1
                )])
                .into_bytes(),
            );
            overrides.push_str(&format!(
                r#"<Override PartName="/xl/pivotTables/pivotTable{pivot_no}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.pivotTable+xml"/>"#
            ));
            rels.push(format!(
                r#"<Relationship Id="rIdCxPivot{pivot_no}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotTable" Target="../pivotTables/pivotTable{pivot_no}.xml"/>"#
            ));
        }
        let mut ext = p.ext.clone();
        if !p.slicers.is_empty() {
            slicer_no += 1;
            let items: String = p
                .slicers
                .iter()
                .map(|x| {
                    format!(
                        r#"<slicer name="{}" cache="{}" caption="{}"{} rowHeight="241300"/>"#,
                        esc(&x.name),
                        esc(&x.cache),
                        esc(&x.caption),
                        if x.columns > 1 { format!(r#" columnCount="{}""#, x.columns) } else { String::new() }
                    )
                })
                .collect();
            put(
                &mut map,
                &mut order,
                format!("xl/slicers/slicer{slicer_no}.xml"),
                format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><slicers xmlns="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="x" xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main">{items}</slicers>"#).into_bytes(),
            );
            overrides.push_str(&format!(r#"<Override PartName="/xl/slicers/slicer{slicer_no}.xml" ContentType="application/vnd.ms-excel.slicer+xml"/>"#));
            rels.push(format!(
                r#"<Relationship Id="rIdCxSlicer" Type="http://schemas.microsoft.com/office/2007/relationships/slicer" Target="../slicers/slicer{slicer_no}.xml"/>"#
            ));
            ext.push_str(r#"<ext uri="{3A4CF648-6AED-40f4-86FF-DC5316D8AED3}" xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main"><x14:slicerList xmlns:x15="http://schemas.microsoft.com/office/spreadsheetml/2010/11/main"><x14:slicer r:id="rIdCxSlicer"/></x14:slicerList></ext>"#);
            caches.extend(p.slicers.iter());
        }

        if !p.charts.is_empty() || !p.pictures.is_empty() || !p.shapes.is_empty() || !p.slicers.is_empty() {
            drawing_no += 1;
            let mut anchors = String::new();
            let mut drels = Vec::new();
            for (k, pic) in p.pictures.iter().enumerate() {
                image_no += 1;
                let rid = format!("rIdPic{}", k + 1);
                let media = format!("xl/media/cellx_image{image_no}.{}", pic.ext);
                drels.push(format!(
                    r#"<Relationship Id="{rid}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/cellx_image{image_no}.{}"/>"#,
                    pic.ext
                ));
                anchors.push_str(&picture_anchor_xml(pic, 1000 + k as u32, &rid));
                put(&mut map, &mut order, media, pic.data.as_ref().clone());
                if !media_types.contains(&pic.ext) {
                    media_types.push(pic.ext);
                }
            }
            for (k, x) in p.slicers.iter().enumerate() {
                let m = |tag: &str, k: &Marker| {
                    format!(
                        "<xdr:{tag}><xdr:col>{}</xdr:col><xdr:colOff>{}</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>{}</xdr:rowOff></xdr:{tag}>",
                        k.col, k.col_off, k.row, k.row_off
                    )
                };
                anchors.push_str(&format!(
                    r#"<xdr:twoCellAnchor editAs="oneCell">{}{}<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice xmlns:sle15="http://schemas.microsoft.com/office/drawing/2012/slicer" Requires="sle15"><xdr:graphicFrame macro=""><xdr:nvGraphicFramePr><xdr:cNvPr id="{}" name="{}"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><xdr:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/></xdr:xfrm><a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/drawing/2010/slicer"><sle:slicer xmlns:sle="http://schemas.microsoft.com/office/drawing/2010/slicer" name="{}"/></a:graphicData></a:graphic></xdr:graphicFrame></mc:Choice><mc:Fallback><xdr:sp macro="" textlink=""><xdr:nvSpPr><xdr:cNvPr id="0" name=""/><xdr:cNvSpPr><a:spLocks noTextEdit="1"/></xdr:cNvSpPr></xdr:nvSpPr><xdr:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{}" cy="{}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:prstClr val="white"/></a:solidFill><a:ln w="1"><a:solidFill><a:prstClr val="green"/></a:solidFill></a:ln></xdr:spPr><xdr:txBody><a:bodyPr vertOverflow="clip" horzOverflow="clip"/><a:lstStyle/><a:p><a:r><a:rPr lang="ko-KR" sz="1100"/><a:t>이 도형은 테이블 슬라이서를 나타냅니다. 테이블 슬라이서는 Excel 2013 이상에서 지원됩니다.</a:t></a:r></a:p></xdr:txBody></xdr:sp></mc:Fallback></mc:AlternateContent><xdr:clientData/></xdr:twoCellAnchor>"#,
                    m("from", &x.from),
                    m("to", &x.to),
                    3000 + k,
                    esc(&x.name),
                    esc(&x.name),
                    x.cx,
                    x.cy
                ));
            }
            for (k, sh) in p.shapes.iter().enumerate() {
                let m = |tag: &str, k: &Marker| {
                    format!(
                        "<xdr:{tag}><xdr:col>{}</xdr:col><xdr:colOff>{}</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>{}</xdr:rowOff></xdr:{tag}>",
                        k.col, k.col_off, k.row, k.row_off
                    )
                };
                let sp = sh.sp.replacen(r#"cNvPr id="0""#, &format!(r#"cNvPr id="{}""#, 2000 + k), 1);
                anchors.push_str(&format!(r#"<xdr:twoCellAnchor>{}{}{sp}<xdr:clientData/></xdr:twoCellAnchor>"#, m("from", &sh.from), m("to", &sh.to)));
            }
            for (k, ch) in p.charts.iter().enumerate() {
                chart_no += 1;
                let rid = format!("rId{}", k + 1);
                drels.push(format!(
                    r#"<Relationship Id="{rid}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="../charts/chart{chart_no}.xml"/>"#
                ));
                anchors.push_str(&anchor_xml(ch, k as u32 + 2, &rid));
                match &ch.original {
                    Some(o) => {
                        // As the file had it, with its style and color parts.
                        put(&mut map, &mut order, format!("xl/charts/chart{chart_no}.xml"), o.xml.clone().into_bytes());
                        let mut crels = Vec::new();
                        for (j, (id, kind, name, bytes, ctype)) in o.parts.iter().enumerate() {
                            let file = format!("cellx_c{chart_no}_{j}_{name}");
                            put(&mut map, &mut order, format!("xl/charts/{file}"), bytes.clone());
                            crels.push(format!(r#"<Relationship Id="{}" Type="{}" Target="{}"/>"#, esc(id), esc(kind), esc(&file)));
                            match ctype {
                                Some(t) => overrides.push_str(&format!(r#"<Override PartName="/xl/charts/{file}" ContentType="{}"/>"#, esc(t))),
                                None => {
                                    // A default by extension (an embedded workbook, an image).
                                    if let Some(ext) = file.rsplit('.').next() {
                                        let known = ["png", "jpeg", "jpg", "gif", "bmp", "xlsx", "bin", "emf", "wmf"];
                                        if known.contains(&ext) && !media_types.iter().any(|m| *m == ext) {
                                            let ext: &'static str = known.iter().find(|k| **k == ext).copied().unwrap_or("bin");
                                            media_types.push(ext);
                                        }
                                    }
                                }
                            }
                        }
                        if !crels.is_empty() {
                            put(&mut map, &mut order, format!("xl/charts/_rels/chart{chart_no}.xml.rels"), relationships(&crels).into_bytes());
                        }
                    }
                    None => put(&mut map, &mut order, format!("xl/charts/chart{chart_no}.xml"), chart_xml(ch).into_bytes()),
                }
                overrides.push_str(&format!(
                    r#"<Override PartName="/xl/charts/chart{chart_no}.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/>"#
                ));
            }
            let drawing = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">{anchors}</xdr:wsDr>"#
            );
            put(&mut map, &mut order, format!("xl/drawings/drawing{drawing_no}.xml"), drawing.into_bytes());
            put(
                &mut map,
                &mut order,
                format!("xl/drawings/_rels/drawing{drawing_no}.xml.rels"),
                relationships(&drels).into_bytes(),
            );
            overrides.push_str(&format!(
                r#"<Override PartName="/xl/drawings/drawing{drawing_no}.xml" ContentType="application/vnd.openxmlformats-officedocument.drawing+xml"/>"#
            ));
            rels.push(format!(
                r#"<Relationship Id="rIdCxDrawing" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing{drawing_no}.xml"/>"#
            ));
            tags.push_str(r#"<drawing r:id="rIdCxDrawing"/>"#);
        }

        if !p.memos.is_empty() {
            memo_no += 1;
            put(&mut map, &mut order, format!("xl/comments{memo_no}.xml"), comments_xml(&p.memos).into_bytes());
            put(
                &mut map,
                &mut order,
                format!("xl/drawings/vmlDrawing{memo_no}.vml"),
                vml_xml(&p.memos, memo_no).into_bytes(),
            );
            overrides.push_str(&format!(
                r#"<Override PartName="/xl/comments{memo_no}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.comments+xml"/>"#
            ));
            rels.push(format!(
                r#"<Relationship Id="rIdCxComments" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="../comments{memo_no}.xml"/>"#
            ));
            rels.push(format!(
                r#"<Relationship Id="rIdCxVml" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/vmlDrawing" Target="../drawings/vmlDrawing{memo_no}.vml"/>"#
            ));
            tags.push_str(r#"<legacyDrawing r:id="rIdCxVml"/>"#);
        }

        let mut sheet_xml = patch_rows(&sheet_xml, &p.outline_rows, &p.hidden_rows);
        // 데이터 표: the formula goes in the first result cell, before its value.
        for (cell, f) in &p.data_tables {
            let key = format!("<c r=\"{cell}\"");
            let Some(i) = sheet_xml.find(&key) else { continue };
            let Some(e) = sheet_xml[i..].find('>') else { continue };
            let end = i + e;
            if sheet_xml[..end].ends_with('/') {
                sheet_xml.replace_range(end - 1..end + 1, &format!(">{f}</c>"));
            } else {
                sheet_xml.insert_str(end + 1, f);
            }
        }
        if !p.scenarios.is_empty() {
            // After sheetData and its protection, before autoFilter and the rest.
            let from = sheet_xml.find("</sheetData>").or_else(|| sheet_xml.find("<sheetData/>")).unwrap_or(0);
            let at = ["<autoFilter", "<sortState", "<dataConsolidate", "<customSheetViews", "<mergeCells", "<phoneticPr", "<conditionalFormatting", "<dataValidations", "<hyperlinks", "<printOptions", "<pageMargins", "<pageSetup", "<headerFooter", "<rowBreaks", "<colBreaks", "<drawing", "<legacyDrawing", "<tableParts", "<extLst", "</worksheet>"]
                .iter()
                .filter_map(|t| sheet_xml[from..].find(t).map(|i| from + i))
                .min()
                .unwrap_or(sheet_xml.len());
            let at = match sheet_xml[from..at].rfind("</protectedRanges>") {
                Some(k) => from + k + "</protectedRanges>".len(),
                None => at,
            };
            sheet_xml.insert_str(at, &p.scenarios);
        }
        if !p.views.is_empty() {
            let from = sheet_xml.find("</sheetData>").or_else(|| sheet_xml.find("<sheetData/>")).unwrap_or(0);
            let at = ["<mergeCells", "<phoneticPr", "<conditionalFormatting", "<dataValidations", "<hyperlinks", "<printOptions", "<pageMargins", "<pageSetup", "<headerFooter", "<rowBreaks", "<colBreaks", "<drawing", "<legacyDrawing", "<tableParts", "<extLst", "</worksheet>"]
                .iter()
                .filter_map(|t| sheet_xml[from..].find(t).map(|i| from + i))
                .min()
                .unwrap_or(sheet_xml.len());
            sheet_xml.insert_str(at, &p.views);
        }
        sheet_xml = patch_cols(&sheet_xml, &p.cols, &p.outline_cols);
        if !p.outline_rows.is_empty() || !p.outline_cols.is_empty() {
            let lr = p.outline_rows.iter().map(|x| x.1).max().unwrap_or(0);
            let lc = p.outline_cols.iter().map(|x| x.1).max().unwrap_or(0);
            if !sheet_xml.contains("<sheetFormatPr") {
                if let Some(at) = sheet_xml.find("<cols>").or_else(|| sheet_xml.find("<sheetData")) {
                    sheet_xml.insert_str(at, &format!(r#"<sheetFormatPr defaultRowHeight="16.5" outlineLevelRow="{lr}" outlineLevelCol="{lc}"/>"#));
                }
            }
        }
        let fit = p.page.as_ref().is_some_and(|pg| pg.fit != FitMode::None);
        let tab = match &p.tab_color {
            Some(ironcalc_base::types::Color::Rgb(rgb)) => format!(r#"<tabColor rgb="FF{}"/>"#, rgb.trim_start_matches('#').to_uppercase()),
            Some(ironcalc_base::types::Color::Theme(t, tint)) => format!(r#"<tabColor theme="{t}" tint="{tint}"/>"#),
            _ => String::new(),
        };
        if (fit || !tab.is_empty()) && !sheet_xml.contains("<sheetPr") {
            if let Some(at) = sheet_xml.find("<dimension").or_else(|| sheet_xml.find("<sheetViews")) {
                let fit_xml = if fit { r#"<pageSetUpPr fitToPage="1"/>"# } else { "" };
                sheet_xml.insert_str(at, &format!("<sheetPr>{tab}{fit_xml}</sheetPr>"));
            }
        }
        let after_data = |xml: &String| {
            xml.find("</sheetData>")
                .map(|i| i + "</sheetData>".len())
                .or_else(|| xml.find("<sheetData/>").map(|i| i + "<sheetData/>".len()))
        };
        if let Some(pr) = &p.protection {
            let xml = protection_xml(pr, &p.unlocked);
            if let Some(at) = after_data(&sheet_xml) {
                sheet_xml.insert_str(at, &xml);
            }
        }
        let mut table_tags = String::new();
        for (k, t) in p.tables.iter().enumerate() {
            table_no += 1;
            table_ids.insert(t.table.name.to_lowercase(), table_no);
            put(&mut map, &mut order, format!("xl/tables/table{table_no}.xml"), table_xml(table_no, t).into_bytes());
            overrides.push_str(&format!(
                r#"<Override PartName="/xl/tables/table{table_no}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml"/>"#
            ));
            rels.push(format!(
                r#"<Relationship Id="rIdCxTable{k}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/table" Target="../tables/table{table_no}.xml"/>"#
            ));
            table_tags.push_str(&format!(r#"<tablePart r:id="rIdCxTable{k}"/>"#));
        }
        let page_tags = p.page.as_ref().map(page_xml).unwrap_or_default();
        let tags = format!(
            "{page_tags}{tags}{}",
            if table_tags.is_empty() { String::new() } else { format!(r#"<tableParts count="{}">{table_tags}</tableParts>"#, p.tables.len()) }
        );
        // autoFilter comes right after sheetData (and its protection), dataValidations before hyperlinks.
        if let Some(f) = &p.filter {
            let xml = auto_filter_xml(f);
            let at = sheet_xml
                .find("<mergeCells")
                .or_else(|| after_data(&sheet_xml).map(|i| {
                    // After sheetProtection/protectedRanges when present.
                    let rest = &sheet_xml[i..];
                    let mut j = i;
                    for tag in ["</protectedRanges>"] {
                        if let Some(k) = rest.find(tag) {
                            j = i + k + tag.len();
                        }
                    }
                    if j == i {
                        if let Some(k) = rest.find("<sheetProtection") {
                            j = i + k + rest[k..].find("/>").map(|x| x + 2).unwrap_or(0);
                        }
                    }
                    j
                }));
            if let Some(at) = at {
                sheet_xml.insert_str(at, &xml);
            }
        }
        if !p.validations.is_empty() {
            let xml = validations_xml(&p.validations);
            let at = sheet_xml
                .find("<hyperlinks")
                .or_else(|| top_ext_start(&sheet_xml))
                .or_else(|| sheet_xml.rfind("</worksheet>"))
                .unwrap_or(sheet_xml.len());
            sheet_xml.insert_str(at, &xml);
        }
        // drawing and legacyDrawing come before extLst in a worksheet.
        let at = top_ext_start(&sheet_xml)
            .or_else(|| sheet_xml.rfind("</worksheet>"))
            .unwrap_or(sheet_xml.len());
        let mut new_sheet = format!("{}{}{}", &sheet_xml[..at], tags, &sheet_xml[at..]);
        if !ext.is_empty() {
            // The worksheet's own extLst is its last element.
            let end = new_sheet.rfind("</worksheet>").unwrap_or(new_sheet.len());
            if new_sheet[..end].trim_end().ends_with("</extLst>") {
                let at = new_sheet[..end].rfind("</extLst>").unwrap_or(end);
                new_sheet.insert_str(at, &ext);
            } else {
                new_sheet.insert_str(end, &format!("<extLst>{ext}</extLst>"));
            }
        }
        put(&mut map, &mut order, sheet_path, new_sheet.into_bytes());

        if rels.is_empty() {
            continue;
        }
        let new_rels = match map.get(&rels_path) {
            Some(old) => {
                let old = String::from_utf8_lossy(old).to_string();
                let at = old.rfind("</Relationships>").unwrap_or(old.len());
                format!("{}{}{}", &old[..at], rels.join(""), &old[at..])
            }
            None => relationships(&rels),
        };
        put(&mut map, &mut order, rels_path, new_rels.into_bytes());
    }

    // Excel and LibreOffice keep the AutoFilter range in a hidden name as well.
    let filter_names: String = parts
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.filter.as_ref().map(|f| (i, f)))
        .filter_map(|(i, f)| {
            let names = sheet_names.get(i)?;
            let r = f.range;
            Some(format!(
                r#"<definedName name="_xlnm._FilterDatabase" localSheetId="{i}" hidden="1">{}!${}${}:${}${}</definedName>"#,
                esc(&sheet_ref(names)),
                column_name(r.c0),
                r.r0,
                column_name(r.c1),
                r.r1
            ))
        })
        .collect();
    let title_names: String = parts
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.page.as_ref().and_then(|pg| pg.title_rows).map(|t| (i, t)))
        .filter_map(|(i, (a, b))| {
            let n = sheet_names.get(i)?;
            Some(format!(r#"<definedName name="_xlnm.Print_Titles" localSheetId="{i}">{}!${a}:${b}</definedName>"#, esc(&sheet_ref(n))))
        })
        .collect();
    let area_names: String = parts
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.page.as_ref().and_then(|pg| pg.print_area).map(|a| (i, a)))
        .filter_map(|(i, r)| {
            let n = sheet_names.get(i)?;
            Some(format!(
                r#"<definedName name="_xlnm.Print_Area" localSheetId="{i}">{}!${}${}:${}${}</definedName>"#,
                esc(&sheet_ref(n)),
                column_name(r.c0),
                r.r0,
                column_name(r.c1),
                r.r1
            ))
        })
        .collect();
    // Slicer caches: one part each, named in the workbook, listed in its extLst.
    let mut slicer_names = String::new();
    let mut cache_refs = String::new();
    let mut wb_rels_new = String::new();
    for (k, x) in caches.iter().enumerate() {
        let Some(table_id) = table_ids.get(&x.table.to_lowercase()) else { continue };
        let n = k + 1;
        put(
            &mut map,
            &mut order,
            format!("xl/slicerCaches/slicerCache{n}.xml"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><slicerCacheDefinition xmlns="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="x" xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="{}" sourceName="{}"><extLst><x:ext uri="{{2F2917AC-EB37-4324-AD4E-5DD8C200BD13}}" xmlns:x15="http://schemas.microsoft.com/office/spreadsheetml/2010/11/main"><x15:tableSlicerCache tableId="{table_id}" column="{}"/></x:ext></extLst></slicerCacheDefinition>"#,
                esc(&x.cache),
                esc(&x.column),
                x.column_id
            )
            .into_bytes(),
        );
        overrides.push_str(&format!(r#"<Override PartName="/xl/slicerCaches/slicerCache{n}.xml" ContentType="application/vnd.ms-excel.slicerCache+xml"/>"#));
        wb_rels_new.push_str(&format!(
            r#"<Relationship Id="rIdCxSlicerCache{n}" Type="http://schemas.microsoft.com/office/2007/relationships/slicerCache" Target="slicerCaches/slicerCache{n}.xml"/>"#
        ));
        cache_refs.push_str(&format!(r#"<x14:slicerCache r:id="rIdCxSlicerCache{n}"/>"#));
        slicer_names.push_str(&format!(r#"<definedName name="{}">#N/A</definedName>"#, esc(&x.cache)));
    }
    let mut pivot_caches = String::new();
    for (k, cache) in book.pivot_caches.iter().enumerate() {
        let n = k + 1;
        put(&mut map, &mut order, format!("xl/pivotCache/pivotCacheDefinition{n}.xml"), cache.clone().into_bytes());
        overrides.push_str(&format!(
            r#"<Override PartName="/xl/pivotCache/pivotCacheDefinition{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheDefinition+xml"/>"#
        ));
        wb_rels_new.push_str(&format!(
            r#"<Relationship Id="rIdCxPc{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition" Target="pivotCache/pivotCacheDefinition{n}.xml"/>"#
        ));
        pivot_caches.push_str(&format!(r#"<pivotCache cacheId="{}" r:id="rIdCxPc{n}"/>"#, crate::pivotxml::CACHE_ID_BASE + k as u32));
    }
    if !wb_rels_new.is_empty() {
        if let Some(r) = map.get("xl/_rels/workbook.xml.rels") {
            let mut r = String::from_utf8_lossy(r).to_string();
            let at = r.rfind("</Relationships>").unwrap_or(r.len());
            r.insert_str(at, &wb_rels_new);
            put(&mut map, &mut order, "xl/_rels/workbook.xml.rels".into(), r.into_bytes());
        }
    }
    let area_names = format!("{area_names}{slicer_names}");
    let title_names = format!("{title_names}{area_names}");
    let filter_names = format!("{filter_names}{title_names}");
    if !filter_names.is_empty() {
        if let Some(wb) = map.get("xl/workbook.xml") {
            let mut wb = String::from_utf8_lossy(wb).to_string();
            if let Some(at) = wb.find("</definedNames>") {
                wb.insert_str(at, &filter_names);
            } else if let Some(at) = wb.find("<calcPr").or_else(|| wb.find("</workbook>")) {
                wb.insert_str(at, &format!("<definedNames>{filter_names}</definedNames>"));
            }
            put(&mut map, &mut order, "xl/workbook.xml".into(), wb.into_bytes());
        }
    }
    if !book.protection.is_empty() || !book.views.is_empty() || !pivot_caches.is_empty() {
        if let Some(wb) = map.get("xl/workbook.xml") {
            let mut wb = String::from_utf8_lossy(wb).to_string();
            if !book.protection.is_empty() {
                // After fileVersion, fileSharing and workbookPr, before bookViews.
                if let Some(at) = wb.find("<bookViews").or_else(|| wb.find("<sheets")) {
                    wb.insert_str(at, &book.protection);
                }
            }
            if !book.views.is_empty() {
                // After calcPr and oleSize, before pivotCaches and the rest.
                let from = wb.find("<sheets").unwrap_or(0);
                let at = ["<pivotCaches", "<smartTagPr", "<smartTagTypes", "<webPublishing", "<fileRecoveryPr", "<webPublishObjects", "<extLst", "</workbook>"]
                    .iter()
                    .filter_map(|t| wb[from..].find(t).map(|i| from + i))
                    .min()
                    .unwrap_or(wb.len());
                wb.insert_str(at, &book.views);
            }
            if !pivot_caches.is_empty() {
                // pivotCaches come after customWorkbookViews.
                let from = wb.find("<sheets").unwrap_or(0);
                let at = ["<smartTagPr", "<smartTagTypes", "<webPublishing", "<fileRecoveryPr", "<webPublishObjects", "<extLst", "</workbook>"]
                    .iter()
                    .filter_map(|t| wb[from..].find(t).map(|i| from + i))
                    .min()
                    .unwrap_or(wb.len());
                wb.insert_str(at, &format!("<pivotCaches>{pivot_caches}</pivotCaches>"));
                if !wb.contains("xmlns:r=") {
                    wb = wb.replacen("<workbook ", r#"<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#, 1);
                }
            }
            put(&mut map, &mut order, "xl/workbook.xml".into(), wb.into_bytes());
        }
    }
    if !cache_refs.is_empty() {
        if let Some(wb) = map.get("xl/workbook.xml") {
            let mut wb = String::from_utf8_lossy(wb).to_string();
            let ext = format!(
                r#"<ext uri="{{46BE6895-7355-4a93-B00E-2C351335B9C9}}" xmlns:x15="http://schemas.microsoft.com/office/spreadsheetml/2010/11/main"><x15:slicerCaches xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main">{cache_refs}</x15:slicerCaches></ext>"#
            );
            let end = wb.rfind("</workbook>").unwrap_or(wb.len());
            if wb[..end].trim_end().ends_with("</extLst>") {
                let at = wb[..end].rfind("</extLst>").unwrap_or(end);
                wb.insert_str(at, &ext);
            } else {
                wb.insert_str(end, &format!("<extLst>{ext}</extLst>"));
            }
            if !wb.contains("xmlns:r=") {
                wb = wb.replacen("<workbook ", r#"<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#, 1);
            }
            put(&mut map, &mut order, "xl/workbook.xml".into(), wb.into_bytes());
        }
    }

    if let Some(ct) = map.get("[Content_Types].xml") {
        let mut ct = String::from_utf8_lossy(ct).to_string();
        for ext in &media_types {
            let mime = if *ext == "jpeg" { "image/jpeg".to_string() } else { format!("image/{ext}") };
            if !ct.contains(&format!("Extension=\"{ext}\"")) {
                overrides.insert_str(0, &format!(r#"<Default Extension="{ext}" ContentType="{mime}"/>"#));
            }
        }
        if memo_no > 0 && !ct.contains("Extension=\"vml\"") {
            overrides.insert_str(
                0,
                r#"<Default Extension="vml" ContentType="application/vnd.openxmlformats-officedocument.vmlDrawing"/>"#,
            );
        }
        let at = ct.rfind("</Types>").unwrap_or(ct.len());
        ct.insert_str(at, &overrides);
        put(&mut map, &mut order, "[Content_Types].xml".into(), ct.into_bytes());
    }

    let mut out = Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut out);
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for name in &order {
            if changed.contains(name) || !loaded.contains(name) && map.contains_key(name) {
                zerr(zw.start_file(name.as_str(), opts))?;
                zw.write_all(&map[name])?;
            } else {
                // Unchanged: the compressed bytes go across untouched.
                zerr(zw.raw_copy_file(zerr(zin.by_name(name))?))?;
            }
        }
        zerr(zw.finish())?;
    }
    Ok(out.into_inner())
}

fn relationships(rels: &[String]) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{}</Relationships>"#,
        rels.join("")
    )
}

fn comments_xml(memos: &[MemoXml]) -> String {
    let mut authors: Vec<&str> = Vec::new();
    for m in memos {
        if !authors.contains(&m.author.as_str()) {
            authors.push(&m.author);
        }
    }
    let authors_xml: String = authors.iter().map(|a| format!("<author>{}</author>", esc(a))).collect();
    let list: String = memos
        .iter()
        .map(|m| {
            let id = authors.iter().position(|a| *a == m.author).unwrap_or(0);
            format!(
                r#"<comment ref="{}{}" authorId="{id}"><text><r><rPr><sz val="9"/><rFont val="맑은 고딕"/></rPr><t xml:space="preserve">{}</t></r></text></comment>"#,
                column_name(m.col),
                m.row,
                esc(&m.text)
            )
        })
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><comments xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><authors>{authors_xml}</authors><commentList>{list}</commentList></comments>"#
    )
}

fn vml_xml(memos: &[MemoXml], n: usize) -> String {
    let mut shapes = String::new();
    for (i, m) in memos.iter().enumerate() {
        let (r, c) = (m.row - 1, m.col - 1);
        shapes.push_str(&format!(
            r##"<v:shape id="_x0000_s{}" type="#_x0000_t202" style="position:absolute;margin-left:80pt;margin-top:2pt;width:108pt;height:60pt;z-index:{};visibility:hidden" fillcolor="#ffffe1" o:insetmode="auto"><v:fill color2="#ffffe1"/><v:shadow on="t" color="black" obscured="t"/><v:path o:connecttype="none"/><v:textbox style="mso-direction-alt:auto"><div style="text-align:left"></div></v:textbox><x:ClientData ObjectType="Note"><x:MoveWithCells/><x:SizeWithCells/><x:Anchor>{}, 15, {}, 2, {}, 15, {}, 16</x:Anchor><x:AutoFill>False</x:AutoFill><x:Row>{r}</x:Row><x:Column>{c}</x:Column></x:ClientData></v:shape>"##,
            1024 * n + i + 1,
            i + 1,
            c + 1,
            r.max(0),
            c + 3,
            r + 4,
        ));
    }
    format!(
        r##"<xml xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:x="urn:schemas-microsoft-com:office:excel"><o:shapelayout v:ext="edit"><o:idmap v:ext="edit" data="{n}"/></o:shapelayout><v:shapetype id="_x0000_t202" coordsize="21600,21600" o:spt="202" path="m,l,21600r21600,l21600,xe"><v:stroke joinstyle="miter"/><v:path gradientshapeok="t" o:connecttype="rect"/></v:shapetype>{shapes}</xml>"##
    )
}

fn picture_anchor_xml(pic: &PictureXml, id: u32, rid: &str) -> String {
    let m = |tag: &str, k: &Marker| {
        format!(
            "<xdr:{tag}><xdr:col>{}</xdr:col><xdr:colOff>{}</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>{}</xdr:rowOff></xdr:{tag}>",
            k.col, k.col_off, k.row, k.row_off
        )
    };
    format!(
        r#"<xdr:twoCellAnchor editAs="oneCell">{}{}<xdr:pic><xdr:nvPicPr><xdr:cNvPr id="{id}" name="{}"/><xdr:cNvPicPr><a:picLocks noChangeAspect="1"/></xdr:cNvPicPr></xdr:nvPicPr><xdr:blipFill><a:blip xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></xdr:blipFill><xdr:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{}" cy="{}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></xdr:spPr></xdr:pic><xdr:clientData/></xdr:twoCellAnchor>"#,
        m("from", &pic.from),
        m("to", &pic.to),
        esc(&pic.name),
        pic.cx,
        pic.cy
    )
}

fn anchor_xml(ch: &ChartXml, id: u32, rid: &str) -> String {
    let m = |tag: &str, k: &Marker| {
        format!(
            "<xdr:{tag}><xdr:col>{}</xdr:col><xdr:colOff>{}</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>{}</xdr:rowOff></xdr:{tag}>",
            k.col, k.col_off, k.row, k.row_off
        )
    };
    format!(
        r#"<xdr:twoCellAnchor editAs="oneCell">{}{}<xdr:graphicFrame macro=""><xdr:nvGraphicFramePr><xdr:cNvPr id="{id}" name="차트 {}"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><xdr:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/></xdr:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" r:id="{rid}"/></a:graphicData></a:graphic></xdr:graphicFrame><xdr:clientData/></xdr:twoCellAnchor>"#,
        m("from", &ch.from),
        m("to", &ch.to),
        id - 1
    )
}

fn num_cache(values: &[Option<f64>]) -> String {
    let pts: String = values
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|v| format!(r#"<c:pt idx="{i}"><c:v>{v}</c:v></c:pt>"#)))
        .collect();
    format!(r#"<c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="{}"/>{pts}</c:numCache>"#, values.len())
}

fn str_cache(values: &[String]) -> String {
    let pts: String = values
        .iter()
        .enumerate()
        .map(|(i, v)| format!(r#"<c:pt idx="{i}"><c:v>{}</c:v></c:pt>"#, esc(v)))
        .collect();
    format!(r#"<c:strCache><c:ptCount val="{}"/>{pts}</c:strCache>"#, values.len())
}

/// Excel's default palette for series (Office theme accents).
const PALETTE: [&str; 6] = ["4472C4", "ED7D31", "A5A5A5", "FFC000", "5B9BD5", "70AD47"];

fn chart_xml(ch: &ChartXml) -> String {
    let d = &ch.data;
    let sheet = &ch.data_sheet;
    let mut series = String::new();
    for (i, s) in d.series.iter().enumerate() {
        let tx = match s.name_ref {
            Some(cell) => format!(
                r#"<c:tx><c:strRef><c:f>{}</c:f>{}</c:strRef></c:tx>"#,
                esc(&abs_range(sheet, (cell, cell))),
                str_cache(std::slice::from_ref(&s.name))
            ),
            None => format!("<c:tx><c:v>{}</c:v></c:tx>", esc(&s.name)),
        };
        let color = PALETTE[i % PALETTE.len()];
        let sppr = match ch.kind {
            ChartKind::Line | ChartKind::Scatter => format!(
                r#"<c:spPr><a:ln w="28575" cap="rnd"><a:solidFill><a:srgbClr val="{color}"/></a:solidFill></a:ln></c:spPr>"#
            ),
            ChartKind::Pie | ChartKind::Doughnut => String::new(),
            _ => format!(r#"<c:spPr><a:solidFill><a:srgbClr val="{color}"/></a:solidFill></c:spPr>"#),
        };
        let vals_f = esc(&abs_range(sheet, s.values_ref));
        let (cat, val) = if ch.kind == ChartKind::Scatter {
            let x = match (&d.categories_ref, &d.x_values) {
                (Some(r), Some(xs)) => format!(
                    r#"<c:xVal><c:numRef><c:f>{}</c:f>{}</c:numRef></c:xVal>"#,
                    esc(&abs_range(sheet, *r)),
                    num_cache(xs)
                ),
                _ => String::new(),
            };
            (x, format!(r#"<c:yVal><c:numRef><c:f>{vals_f}</c:f>{}</c:numRef></c:yVal>"#, num_cache(&s.values)))
        } else {
            let cat = match &d.categories_ref {
                Some(r) => format!(
                    r#"<c:cat><c:strRef><c:f>{}</c:f>{}</c:strRef></c:cat>"#,
                    esc(&abs_range(sheet, *r)),
                    str_cache(&d.categories)
                ),
                None => String::new(),
            };
            (cat, format!(r#"<c:val><c:numRef><c:f>{vals_f}</c:f>{}</c:numRef></c:val>"#, num_cache(&s.values)))
        };
        let marker = match ch.kind {
            ChartKind::Line => r#"<c:marker><c:symbol val="none"/></c:marker>"#,
            ChartKind::Scatter => r#"<c:marker><c:symbol val="circle"/><c:size val="5"/></c:marker>"#,
            _ => "",
        };
        let smooth = if matches!(ch.kind, ChartKind::Line | ChartKind::Scatter) { r#"<c:smooth val="0"/>"# } else { "" };
        let trend = if ch.trendline && !matches!(ch.kind, ChartKind::Pie | ChartKind::Doughnut) {
            format!(r#"<c:trendline><c:spPr><a:ln w="19050" cap="rnd"><a:solidFill><a:srgbClr val="{color}"/></a:solidFill><a:prstDash val="sysDot"/></a:ln></c:spPr><c:trendlineType val="linear"/><c:dispRSqr val="0"/><c:dispEq val="0"/></c:trendline>"#)
        } else {
            String::new()
        };
        series.push_str(&format!(
            r#"<c:ser><c:idx val="{i}"/><c:order val="{i}"/>{tx}{sppr}{marker}{trend}{cat}{val}{smooth}</c:ser>"#
        ));
    }
    let axes = r#"<c:axId val="111111111"/><c:axId val="222222222"/>"#;
    let plot = match ch.kind {
        ChartKind::Column | ChartKind::Bar => format!(
            r#"<c:barChart><c:barDir val="{}"/><c:grouping val="clustered"/><c:varyColors val="0"/>{series}<c:gapWidth val="219"/><c:overlap val="-27"/>{axes}</c:barChart>"#,
            if ch.kind == ChartKind::Bar { "bar" } else { "col" }
        ),
        ChartKind::Line => format!(
            r#"<c:lineChart><c:grouping val="standard"/><c:varyColors val="0"/>{series}<c:marker val="1"/>{axes}</c:lineChart>"#
        ),
        ChartKind::Area => format!(r#"<c:areaChart><c:grouping val="standard"/><c:varyColors val="0"/>{series}{axes}</c:areaChart>"#),
        ChartKind::Pie => format!(r#"<c:pieChart><c:varyColors val="1"/>{series}<c:firstSliceAng val="0"/></c:pieChart>"#),
        ChartKind::Doughnut => format!(
            r#"<c:doughnutChart><c:varyColors val="1"/>{series}<c:firstSliceAng val="0"/><c:holeSize val="50"/></c:doughnutChart>"#
        ),
        ChartKind::Scatter => format!(r#"<c:scatterChart><c:scatterStyle val="lineMarker"/><c:varyColors val="0"/>{series}{axes}</c:scatterChart>"#),
    };
    let grid = r#"<c:majorGridlines><c:spPr><a:ln w="9525"><a:solidFill><a:srgbClr val="D9D9D9"/></a:solidFill></a:ln></c:spPr></c:majorGridlines>"#;
    let axis_line = r#"<c:spPr><a:ln w="9525"><a:solidFill><a:srgbClr val="BFBFBF"/></a:solidFill></a:ln></c:spPr>"#;
    let (bottom_pos, left_pos) = if ch.kind == ChartKind::Bar { ("l", "b") } else { ("b", "l") };
    let axes_xml = match ch.kind {
        ChartKind::Pie | ChartKind::Doughnut => String::new(),
        ChartKind::Scatter => format!(
            r#"<c:valAx><c:axId val="111111111"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="b"/><c:numFmt formatCode="General" sourceLinked="1"/><c:majorTickMark val="none"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/>{axis_line}<c:crossAx val="222222222"/><c:crosses val="autoZero"/><c:crossBetween val="midCat"/></c:valAx><c:valAx><c:axId val="222222222"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="l"/>{grid}<c:numFmt formatCode="General" sourceLinked="1"/><c:majorTickMark val="none"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/><c:crossAx val="111111111"/><c:crosses val="autoZero"/><c:crossBetween val="midCat"/></c:valAx>"#
        ),
        _ => format!(
            r#"<c:catAx><c:axId val="111111111"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="{bottom_pos}"/><c:numFmt formatCode="General" sourceLinked="1"/><c:majorTickMark val="none"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/>{axis_line}<c:crossAx val="222222222"/><c:crosses val="autoZero"/><c:auto val="1"/><c:lblAlgn val="ctr"/><c:lblOffset val="100"/><c:noMultiLvlLbl val="0"/></c:catAx><c:valAx><c:axId val="222222222"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="{left_pos}"/>{grid}<c:numFmt formatCode="General" sourceLinked="1"/><c:majorTickMark val="none"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/><c:spPr><a:ln><a:noFill/></a:ln></c:spPr><c:crossAx val="111111111"/><c:crosses val="autoZero"/><c:crossBetween val="between"/></c:valAx>"#
        ),
    };
    let title = if d.title.is_empty() {
        r#"<c:autoTitleDeleted val="1"/>"#.to_string()
    } else {
        format!(
            r#"<c:title><c:tx><c:rich><a:bodyPr/><a:p><a:pPr><a:defRPr sz="1400" b="0"/></a:pPr><a:r><a:rPr lang="ko-KR" sz="1400" b="0"/><a:t>{}</a:t></a:r></a:p></c:rich></c:tx><c:overlay val="0"/></c:title><c:autoTitleDeleted val="0"/>"#,
            esc(&d.title)
        )
    };
    let legend = if ch.legend {
        r#"<c:legend><c:legendPos val="b"/><c:overlay val="0"/></c:legend>"#
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><c:roundedCorners val="0"/><c:chart>{title}<c:plotArea><c:layout/>{plot}{axes_xml}</c:plotArea>{legend}<c:plotVisOnly val="1"/><c:dispBlanksAs val="gap"/></c:chart><c:txPr><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr sz="900"><a:latin typeface="맑은 고딕"/><a:ea typeface="맑은 고딕"/></a:defRPr></a:pPr><a:endParaRPr lang="ko-KR"/></a:p></c:txPr></c:chartSpace>"#
    )
}

// ---------------------------------------------------------------- reading charts

/// A chart found in an xlsx, before its references are turned into a range.
#[derive(Debug, Clone)]
pub struct ImportedChart {
    /// 0-based sheet index.
    pub sheet: usize,
    pub kind: ChartKind,
    pub title: String,
    pub legend: bool,
    /// Formulas of the series parts: (names, categories, values).
    pub names: Vec<String>,
    pub categories: Vec<String>,
    pub values: Vec<String>,
    pub from: Marker,
    pub to: Option<Marker>,
    /// Size in EMU for one-cell anchors.
    pub ext: Option<(i64, i64)>,
    pub trendline: bool,
    /// The chart part, and the parts it refers to.
    pub raw: Option<String>,
    pub parts: Vec<(String, String, String, Vec<u8>, Option<String>)>,
}

fn read_part<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Option<String> {
    let mut f = zip.by_name(name).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

/// Resolves a relationship target relative to the part that holds it.
fn resolve(base_dir: &str, target: &str) -> String {
    if let Some(t) = target.strip_prefix('/') {
        return t.to_string();
    }
    let mut parts: Vec<&str> = base_dir.split('/').filter(|p| !p.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn rels_of<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, part: &str) -> HashMap<String, (String, String)> {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    let path = format!("{dir}/_rels/{file}.rels");
    let mut out = HashMap::new();
    if let Some(xml) = read_part(zip, &path) {
        if let Ok(doc) = roxmltree::Document::parse(&xml) {
            for n in doc.descendants().filter(|n| n.has_tag_name("Relationship")) {
                if let (Some(id), Some(t)) = (n.attribute("Id"), n.attribute("Target")) {
                    let kind = n.attribute("Type").unwrap_or("").rsplit('/').next().unwrap_or("").to_string();
                    out.insert(id.to_string(), (kind, resolve(dir, t)));
                }
            }
        }
    }
    out
}

const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn marker(node: roxmltree::Node) -> Marker {
    let get = |tag: &str| {
        node.children()
            .find(|c| c.has_tag_name(tag))
            .and_then(|c| c.text())
            .and_then(|t| t.trim().parse::<i64>().ok())
            .unwrap_or(0)
    };
    Marker { col: get("col") as i32, col_off: get("colOff"), row: get("row") as i32, row_off: get("rowOff") }
}

/// Reads the charts of every sheet. Unknown chart types are skipped.
/// Relationships of a part as written: (id, full type, target).
fn rels_raw<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, part: &str) -> Vec<(String, String, String)> {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    let Some(xml) = read_part(zip, &format!("{dir}/_rels/{file}.rels")) else { return Vec::new() };
    let Ok(doc) = roxmltree::Document::parse(&xml) else { return Vec::new() };
    doc.descendants()
        .filter(|n| n.has_tag_name("Relationship"))
        .filter(|n| n.attribute("TargetMode") != Some("External"))
        .filter_map(|n| Some((n.attribute("Id")?.to_string(), n.attribute("Type")?.to_string(), n.attribute("Target")?.to_string())))
        .collect()
}

/// `[Content_Types].xml` overrides: part name → content type.
fn content_types<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>) -> HashMap<String, String> {
    let Some(xml) = read_part(zip, "[Content_Types].xml") else { return HashMap::new() };
    let Ok(doc) = roxmltree::Document::parse(&xml) else { return HashMap::new() };
    doc.descendants()
        .filter(|n| n.has_tag_name("Override"))
        .filter_map(|n| Some((n.attribute("PartName")?.to_string(), n.attribute("ContentType")?.to_string())))
        .collect()
}

pub fn read_charts(xlsx: &[u8]) -> Vec<ImportedChart> {
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(xlsx)) else { return Vec::new() };
    let types = content_types(&mut zip);
    let Some(wb_xml) = read_part(&mut zip, "xl/workbook.xml") else { return Vec::new() };
    let wb_rels = rels_of(&mut zip, "xl/workbook.xml");
    let Ok(wb_doc) = roxmltree::Document::parse(&wb_xml) else { return Vec::new() };
    let sheet_parts: Vec<String> = wb_doc
        .descendants()
        .filter(|n| n.has_tag_name("sheet"))
        .filter_map(|n| n.attribute((R_NS, "id")).and_then(|id| wb_rels.get(id)).map(|(_, t)| t.clone()))
        .collect();

    let mut out = Vec::new();
    for (si, sheet_part) in sheet_parts.iter().enumerate() {
        let rels = rels_of(&mut zip, sheet_part);
        for (kind, drawing_part) in rels.values() {
            if kind != "drawing" {
                continue;
            }
            let Some(dxml) = read_part(&mut zip, drawing_part) else { continue };
            let drels = rels_of(&mut zip, drawing_part);
            let Ok(ddoc) = roxmltree::Document::parse(&dxml) else { continue };
            for anchor in ddoc
                .root_element()
                .children()
                .filter(|n| n.has_tag_name("twoCellAnchor") || n.has_tag_name("oneCellAnchor"))
            {
                let Some(chart_node) = anchor.descendants().find(|n| n.has_tag_name("chart")) else { continue };
                let Some(rid) = chart_node.attribute((R_NS, "id")) else { continue };
                let Some((_, chart_part)) = drels.get(rid) else { continue };
                let Some(cxml) = read_part(&mut zip, chart_part) else { continue };
                let from = anchor.children().find(|n| n.has_tag_name("from")).map(marker);
                let to = anchor.children().find(|n| n.has_tag_name("to")).map(marker);
                let ext = anchor.children().find(|n| n.has_tag_name("ext")).map(|e| {
                    let g = |a: &str| e.attribute(a).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
                    (g("cx"), g("cy"))
                });
                if let (Some(from), Some(mut c)) = (from, parse_chart(&cxml)) {
                    c.sheet = si;
                    c.from = from;
                    c.to = to;
                    c.ext = ext;
                    // The parts the chart refers to, to write back with it.
                    let crels = rels_raw(&mut zip, chart_part);
                    let mut parts = Vec::new();
                    let mut ok = true;
                    for (id, kind, target) in crels {
                        if target.starts_with("http") {
                            ok = false;
                            break;
                        }
                        let (dir, _) = chart_part.rsplit_once('/').unwrap_or(("", ""));
                        let path = resolve(dir, &target);
                        let Some(bytes) = read_part_bytes(&mut zip, &path) else {
                            ok = false;
                            break;
                        };
                        let name = path.rsplit('/').next().unwrap_or("part").to_string();
                        parts.push((id, kind, name, bytes, types.get(&format!("/{path}")).cloned()));
                    }
                    if ok {
                        c.raw = Some(cxml);
                        c.parts = parts;
                    }
                    out.push(c);
                }
            }
        }
    }
    out
}

/// A picture of an xlsx drawing.
pub struct ImportedPicture {
    pub sheet: usize,
    pub name: String,
    pub data: Vec<u8>,
    pub from: Marker,
    pub to: Option<Marker>,
    /// Size in EMU, when the file gives it.
    pub ext: Option<(i64, i64)>,
}

fn read_part_bytes<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Option<Vec<u8>> {
    let mut f = zip.by_name(name).ok()?;
    let mut v = Vec::new();
    f.read_to_end(&mut v).ok()?;
    Some(v)
}

/// Shapes and text boxes of xlsx drawings: (sheet, from, to, size in EMU, shape).
pub(crate) fn read_shapes(xlsx: &[u8]) -> Vec<(usize, Marker, Option<Marker>, Option<(i64, i64)>, crate::shapes::ImportedShape)> {
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(xlsx)) else { return Vec::new() };
    if !zip.file_names().any(|n| n.is_ok_and(|n| n.starts_with("xl/drawings/drawing"))) {
        return Vec::new();
    }
    let Some(wb_xml) = read_part(&mut zip, "xl/workbook.xml") else { return Vec::new() };
    let wb_rels = rels_of(&mut zip, "xl/workbook.xml");
    let Ok(wb_doc) = roxmltree::Document::parse(&wb_xml) else { return Vec::new() };
    let sheet_parts: Vec<String> = wb_doc
        .descendants()
        .filter(|n| n.has_tag_name("sheet"))
        .filter_map(|n| n.attribute((R_NS, "id")).and_then(|id| wb_rels.get(id)).map(|(_, t)| t.clone()))
        .collect();
    let mut out = Vec::new();
    for (si, sheet_part) in sheet_parts.iter().enumerate() {
        let rels = rels_of(&mut zip, sheet_part);
        for (kind, drawing_part) in rels.values() {
            if kind != "drawing" {
                continue;
            }
            let Some(dxml) = read_part(&mut zip, drawing_part) else { continue };
            let Ok(ddoc) = roxmltree::Document::parse(&dxml) else { continue };
            for anchor in ddoc
                .root_element()
                .children()
                .filter(|n| n.has_tag_name("twoCellAnchor") || n.has_tag_name("oneCellAnchor") || n.has_tag_name("absoluteAnchor"))
            {
                let Some(sp) = anchor.children().find(|n| n.has_tag_name("sp") || n.has_tag_name("cxnSp")) else { continue };
                let Some(shape) = crate::shapes::read_sp(sp) else { continue };
                let Some(from) = anchor.children().find(|n| n.has_tag_name("from")).map(marker) else { continue };
                let to = anchor.children().find(|n| n.has_tag_name("to")).map(marker);
                let size = |e: roxmltree::Node| {
                    let g = |a: &str| e.attribute(a).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
                    (g("cx"), g("cy"))
                };
                let ext = anchor
                    .children()
                    .find(|n| n.has_tag_name("ext"))
                    .map(size)
                    .or_else(|| sp.descendants().find(|n| n.has_tag_name("xfrm")).and_then(|x| x.children().find(|n| n.has_tag_name("ext"))).map(size));
                out.push((si, from, to, ext, shape));
            }
        }
    }
    out
}

/// A table slicer of an xlsx file.
pub(crate) struct ImportedSlicer {
    pub cache: String,
    pub caption: String,
    pub table: String,
    pub column: String,
    pub columns: u32,
    pub from: Marker,
    pub to: Marker,
    pub cx: i64,
    pub cy: i64,
}

pub(crate) fn read_slicers(xlsx: &[u8]) -> Vec<(usize, ImportedSlicer)> {
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(xlsx)) else { return Vec::new() };
    if !zip.file_names().any(|n| n.is_ok_and(|n| n.starts_with("xl/slicerCaches/"))) {
        return Vec::new();
    }
    let Some(wb_xml) = read_part(&mut zip, "xl/workbook.xml") else { return Vec::new() };
    let wb_rels = rels_of(&mut zip, "xl/workbook.xml");
    let Ok(wb_doc) = roxmltree::Document::parse(&wb_xml) else { return Vec::new() };
    // Tables by id, from every table part.
    let names: Vec<String> = zip.file_names().filter_map(|n| n.ok()).filter(|n| n.starts_with("xl/tables/") && n.ends_with(".xml")).map(|n| n.to_string()).collect();
    let mut tables: HashMap<String, (String, HashMap<String, String>)> = HashMap::new();
    for n in names {
        let Some(x) = read_part(&mut zip, &n) else { continue };
        let Ok(d) = roxmltree::Document::parse(&x) else { continue };
        let r = d.root_element();
        let cols = r.descendants().filter(|c| c.has_tag_name("tableColumn")).filter_map(|c| Some((c.attribute("id")?.to_string(), c.attribute("name")?.to_string()))).collect();
        if let (Some(id), Some(name)) = (r.attribute("id"), r.attribute("name").or(r.attribute("displayName"))) {
            tables.insert(id.to_string(), (name.to_string(), cols));
        }
    }
    // Caches by name → (table, column).
    let mut caches: HashMap<String, (String, String)> = HashMap::new();
    for (kind, part) in wb_rels.values() {
        if kind != "slicerCache" {
            continue;
        }
        let Some(x) = read_part(&mut zip, part) else { continue };
        let Ok(d) = roxmltree::Document::parse(&x) else { continue };
        let r = d.root_element();
        let Some(tc) = r.descendants().find(|n| n.has_tag_name("tableSlicerCache")) else { continue };
        let (Some(name), Some(tid), Some(cid)) = (r.attribute("name"), tc.attribute("tableId"), tc.attribute("column")) else { continue };
        let Some((table, cols)) = tables.get(tid) else { continue };
        let Some(column) = cols.get(cid) else { continue };
        caches.insert(name.to_string(), (table.clone(), column.clone()));
    }
    let sheet_parts: Vec<String> = wb_doc
        .descendants()
        .filter(|n| n.has_tag_name("sheet"))
        .filter_map(|n| n.attribute((R_NS, "id")).and_then(|id| wb_rels.get(id)).map(|(_, t)| t.clone()))
        .collect();
    let mut out = Vec::new();
    for (si, sheet_part) in sheet_parts.iter().enumerate() {
        let rels = rels_of(&mut zip, sheet_part);
        // slicer name → (cache, caption, columns)
        let mut slicers: HashMap<String, (String, String, u32)> = HashMap::new();
        for (kind, part) in rels.values() {
            if kind != "slicer" {
                continue;
            }
            let Some(x) = read_part(&mut zip, part) else { continue };
            let Ok(d) = roxmltree::Document::parse(&x) else { continue };
            for s in d.descendants().filter(|n| n.has_tag_name("slicer")) {
                let (Some(name), Some(cache)) = (s.attribute("name"), s.attribute("cache")) else { continue };
                let cols = s.attribute("columnCount").and_then(|c| c.parse().ok()).unwrap_or(1);
                slicers.insert(name.to_string(), (cache.to_string(), s.attribute("caption").unwrap_or("").to_string(), cols));
            }
        }
        if slicers.is_empty() {
            continue;
        }
        for (kind, drawing_part) in rels.values() {
            if kind != "drawing" {
                continue;
            }
            let Some(dxml) = read_part(&mut zip, drawing_part) else { continue };
            let Ok(ddoc) = roxmltree::Document::parse(&dxml) else { continue };
            for anchor in ddoc.root_element().children().filter(|n| n.has_tag_name("twoCellAnchor")) {
                let Some(name) = anchor.descendants().find(|n| n.has_tag_name("slicer") && n.attribute("name").is_some()).and_then(|n| n.attribute("name")) else { continue };
                let Some((cache, caption, columns)) = slicers.get(name) else { continue };
                let Some((table, column)) = caches.get(cache) else { continue };
                let (Some(from), Some(to)) = (anchor.children().find(|n| n.has_tag_name("from")).map(marker), anchor.children().find(|n| n.has_tag_name("to")).map(marker)) else { continue };
                let ext = anchor.descendants().find(|n| n.has_tag_name("ext") && n.attribute("cx").is_some_and(|v| v != "0"));
                let g = |a: &str| ext.and_then(|e| e.attribute(a)).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
                out.push((
                    si,
                    ImportedSlicer { cache: cache.clone(), caption: caption.clone(), table: table.clone(), column: column.clone(), columns: *columns, from, to, cx: g("cx"), cy: g("cy") },
                ));
            }
        }
    }
    out
}

pub fn read_pictures(xlsx: &[u8]) -> Vec<ImportedPicture> {
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(xlsx)) else { return Vec::new() };
    if !zip.file_names().any(|n| n.is_ok_and(|n| n.starts_with("xl/media/"))) {
        return Vec::new();
    }
    let Some(wb_xml) = read_part(&mut zip, "xl/workbook.xml") else { return Vec::new() };
    let wb_rels = rels_of(&mut zip, "xl/workbook.xml");
    let Ok(wb_doc) = roxmltree::Document::parse(&wb_xml) else { return Vec::new() };
    let sheet_parts: Vec<String> = wb_doc
        .descendants()
        .filter(|n| n.has_tag_name("sheet"))
        .filter_map(|n| n.attribute((R_NS, "id")).and_then(|id| wb_rels.get(id)).map(|(_, t)| t.clone()))
        .collect();
    let mut out = Vec::new();
    for (si, sheet_part) in sheet_parts.iter().enumerate() {
        let rels = rels_of(&mut zip, sheet_part);
        for (kind, drawing_part) in rels.values() {
            if kind != "drawing" {
                continue;
            }
            let Some(dxml) = read_part(&mut zip, drawing_part) else { continue };
            let drels = rels_of(&mut zip, drawing_part);
            let Ok(ddoc) = roxmltree::Document::parse(&dxml) else { continue };
            for anchor in ddoc
                .root_element()
                .children()
                .filter(|n| n.has_tag_name("twoCellAnchor") || n.has_tag_name("oneCellAnchor"))
            {
                let Some(pic) = anchor.children().find(|n| n.has_tag_name("pic")) else { continue };
                let Some(rid) = pic.descendants().find(|n| n.has_tag_name("blip")).and_then(|b| b.attribute((R_NS, "embed"))) else {
                    continue;
                };
                let Some((_, media)) = drels.get(rid) else { continue };
                let Some(data) = read_part_bytes(&mut zip, media) else { continue };
                let Some(from) = anchor.children().find(|n| n.has_tag_name("from")).map(marker) else { continue };
                let to = anchor.children().find(|n| n.has_tag_name("to")).map(marker);
                let size = |e: roxmltree::Node| {
                    let g = |a: &str| e.attribute(a).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
                    (g("cx"), g("cy"))
                };
                // The anchor's own size (oneCellAnchor), else the picture's.
                let ext = anchor
                    .children()
                    .find(|n| n.has_tag_name("ext"))
                    .map(size)
                    .or_else(|| pic.descendants().find(|n| n.has_tag_name("xfrm")).and_then(|x| x.children().find(|n| n.has_tag_name("ext"))).map(size));
                let name = pic.descendants().find(|n| n.has_tag_name("cNvPr")).and_then(|n| n.attribute("name")).unwrap_or("").to_string();
                out.push(ImportedPicture { sheet: si, name, data, from, to, ext });
            }
        }
    }
    out
}

fn parse_chart(xml: &str) -> Option<ImportedChart> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let plot = doc.descendants().find(|n| n.has_tag_name("plotArea"))?;
    let chart = plot.children().find(|n| n.is_element() && n.tag_name().name().ends_with("Chart"))?;
    let kind = match chart.tag_name().name() {
        "barChart" | "bar3DChart" => {
            let dir = chart.children().find(|n| n.has_tag_name("barDir")).and_then(|n| n.attribute("val"));
            if dir == Some("bar") {
                ChartKind::Bar
            } else {
                ChartKind::Column
            }
        }
        "lineChart" | "line3DChart" => ChartKind::Line,
        "pieChart" | "pie3DChart" => ChartKind::Pie,
        "doughnutChart" => ChartKind::Doughnut,
        "areaChart" | "area3DChart" => ChartKind::Area,
        "scatterChart" => ChartKind::Scatter,
        _ => return None,
    };
    let formula = |n: roxmltree::Node, tag: &str| -> Option<String> {
        n.children()
            .find(|c| c.has_tag_name(tag))?
            .descendants()
            .find(|c| c.has_tag_name("f"))
            .and_then(|f| f.text())
            .map(|t| t.trim().to_string())
    };
    let mut names = Vec::new();
    let mut categories = Vec::new();
    let mut values = Vec::new();
    for ser in chart.children().filter(|n| n.has_tag_name("ser")) {
        if let Some(f) = formula(ser, "tx") {
            names.push(f);
        }
        if let Some(f) = formula(ser, "cat").or_else(|| formula(ser, "xVal")) {
            categories.push(f);
        }
        if let Some(f) = formula(ser, "val").or_else(|| formula(ser, "yVal")) {
            values.push(f);
        }
    }
    let chart_el = doc.descendants().find(|n| n.has_tag_name("chart"))?;
    let title = chart_el
        .children()
        .find(|n| n.has_tag_name("title"))
        .map(|t| t.descendants().filter(|n| n.has_tag_name("t")).filter_map(|n| n.text()).collect::<String>())
        .unwrap_or_default();
    let legend = chart_el.children().any(|n| n.has_tag_name("legend"));
    Some(ImportedChart {
        sheet: 0,
        kind,
        title,
        legend,
        names,
        categories,
        values,
        from: Marker { col: 0, col_off: 0, row: 0, row_off: 0 },
        to: None,
        ext: None,
        trendline: plot.descendants().any(|n| n.has_tag_name("trendline")),
        raw: None,
        parts: Vec::new(),
    })
}

/// Sheet settings IronCalc does not read: outline levels, protection, editable
/// ranges (and cells whose style is unlocked) and page setup.
pub struct SheetOptionsXml {
    pub sheet: usize,
    pub outline_rows: Vec<(i32, u8)>,
    pub outline_cols: Vec<(i32, u8)>,
    pub protection: Option<SheetProtection>,
    pub unlocked: Vec<CellRange>,
    pub page: Option<PageSetup>,
}

/// A worksheet's XML without its cell data (`<sheetData>…</sheetData>`), and
/// that cell data. The cells are what IronCalc reads; CellX needs the small rest
/// as a tree and only a few attributes of the cells, so the cells are scanned
/// as a stream instead of being built into a tree of millions of nodes.
fn split_sheet_data(xml: &str) -> (std::borrow::Cow<'_, str>, &str) {
    use std::borrow::Cow;
    // The opening tag, with or without a namespace prefix.
    let Some(open) = xml.match_indices("sheetData").map(|(i, _)| i).find(|&i| {
        let before = xml[..i].rfind('<').map(|lt| &xml[lt + 1..i]);
        matches!(before, Some(p) if p.is_empty() || (p.ends_with(':') && !p.contains(['/', ' ', '>'])))
    }) else {
        return (Cow::Borrowed(xml), "");
    };
    let start = xml[..open].rfind('<').unwrap_or(open);
    let Some(gt) = xml[open..].find('>').map(|i| open + i) else { return (Cow::Borrowed(xml), "") };
    if xml.as_bytes()[gt - 1] == b'/' {
        let mut rest = String::with_capacity(xml.len());
        rest.push_str(&xml[..start]);
        rest.push_str(&xml[gt + 1..]);
        return (Cow::Owned(rest), "");
    }
    let Some(close) = xml.rfind("sheetData>").filter(|&i| i > gt) else { return (Cow::Borrowed(xml), "") };
    let Some(close_start) = xml[..close].rfind("</") else { return (Cow::Borrowed(xml), "") };
    let mut rest = String::with_capacity(start + xml.len() - close);
    rest.push_str(&xml[..start]);
    rest.push_str(&xml[close + "sheetData>".len()..]);
    (Cow::Owned(rest), &xml[gt + 1..close_start])
}

/// From the cell data of a worksheet: the outline level of each row that has
/// one, and the cells whose style is in `unlocked_xf`.
fn scan_sheet_data(data: &str, unlocked_xf: &std::collections::HashSet<usize>) -> (Vec<(i32, u8)>, Vec<(i32, i32)>) {
    use quick_xml::events::Event;
    let mut outline = Vec::new();
    let mut unlocked = Vec::new();
    // Most sheets have neither: one fast search instead of a scan of every cell.
    if unlocked_xf.is_empty() && !data.contains("outlineLevel") {
        return (outline, unlocked);
    }
    let mut reader = quick_xml::Reader::from_str(data);
    reader.config_mut().check_end_names = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                "row" => {
                    let (mut r, mut level) = (0, None);
                    for a in e.attributes().flatten() {
                        match a.key.local_name().as_ref() {
                            "r" => r = a.value.parse::<i32>().unwrap_or(0),
                            "outlineLevel" => level = a.value.parse::<u8>().ok(),
                            _ => {}
                        }
                    }
                    if let Some(l) = level {
                        if l > 0 && r > 0 {
                            outline.push((r, l));
                        }
                    }
                }
                "c" if !unlocked_xf.is_empty() => {
                    let (mut style, mut at) = (0usize, None);
                    for a in e.attributes().flatten() {
                        match a.key.local_name().as_ref() {
                            "s" => style = a.value.parse::<usize>().unwrap_or(0),
                            "r" => at = parse_cell(&a.value),
                            _ => {}
                        }
                    }
                    if unlocked_xf.contains(&style) {
                        if let Some(cell) = at {
                            unlocked.push(cell);
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    (outline, unlocked)
}

pub fn read_sheet_options(xlsx: &[u8]) -> Vec<SheetOptionsXml> {
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(xlsx)) else { return Vec::new() };
    let Some(wb_xml) = read_part(&mut zip, "xl/workbook.xml") else { return Vec::new() };
    let wb_rels = rels_of(&mut zip, "xl/workbook.xml");
    let Ok(wb_doc) = roxmltree::Document::parse(&wb_xml) else { return Vec::new() };
    let parts: Vec<String> = wb_doc
        .descendants()
        .filter(|n| n.has_tag_name("sheet"))
        .filter_map(|n| n.attribute((R_NS, "id")).and_then(|id| wb_rels.get(id)).map(|(_, t)| t.clone()))
        .collect();
    // Print titles: _xlnm.Print_Titles = Sheet1!$1:$2.
    let mut titles: std::collections::HashMap<usize, (i32, i32)> = std::collections::HashMap::new();
    let mut areas: std::collections::HashMap<usize, CellRange> = std::collections::HashMap::new();
    for dn in wb_doc.descendants().filter(|n| n.has_tag_name("definedName")) {
        if dn.attribute("name") == Some("_xlnm.Print_Area") {
            let Some(i) = dn.attribute("localSheetId").and_then(|v| v.parse::<usize>().ok()) else { continue };
            // Several areas print on separate pages in Excel; CellX keeps the first.
            let first = dn.text().unwrap_or("").split(',').next().unwrap_or("").replace('$', "");
            if let Some((_, r0, c0, r1, c1)) = crate::extras::parse_range(first.trim()) {
                areas.insert(i, CellRange { r0, c0, r1, c1 });
            }
            continue;
        }
        if dn.attribute("name") != Some("_xlnm.Print_Titles") {
            continue;
        }
        let Some(i) = dn.attribute("localSheetId").and_then(|v| v.parse::<usize>().ok()) else { continue };
        let text = dn.text().unwrap_or("");
        for part in text.split(',') {
            let rows = part.rsplit('!').next().unwrap_or("").replace('$', "");
            if let Some((a, b)) = rows.split_once(':') {
                if let (Ok(a), Ok(b)) = (a.parse::<i32>(), b.parse::<i32>()) {
                    titles.insert(i, (a, b));
                }
            }
        }
    }
    // Styles whose protection is locked="0".
    let unlocked_xf: std::collections::HashSet<usize> = read_part(&mut zip, "xl/styles.xml")
        .and_then(|x| {
            let doc = roxmltree::Document::parse(&x).ok()?;
            let xfs = doc.descendants().find(|n| n.has_tag_name("cellXfs"))?;
            Some(
                xfs.children()
                    .filter(|n| n.has_tag_name("xf"))
                    .enumerate()
                    .filter(|(_, xf)| {
                        xf.children().any(|c| c.has_tag_name("protection") && matches!(c.attribute("locked"), Some("0") | Some("false")))
                    })
                    .map(|(i, _)| i)
                    .collect(),
            )
        })
        .unwrap_or_default();
    let mut out = Vec::new();
    for (si, part) in parts.iter().enumerate() {
        let Some(xml) = read_part(&mut zip, part) else { continue };
        let (rest, data) = split_sheet_data(&xml);
        let Ok(doc) = roxmltree::Document::parse(&rest) else { continue };
        let root = doc.root_element();
        let mut o = SheetOptionsXml { sheet: si, outline_rows: vec![], outline_cols: vec![], protection: None, unlocked: vec![], page: None };
        let (outline_rows, mut unlocked_cells) = scan_sheet_data(data, &unlocked_xf);
        o.outline_rows = outline_rows;
        if let Some(cols) = root.children().find(|n| n.has_tag_name("cols")) {
            for c in cols.children().filter(|n| n.has_tag_name("col")) {
                let (Some(a), Some(b)) = (c.attribute("min").and_then(|v| v.parse::<i32>().ok()), c.attribute("max").and_then(|v| v.parse::<i32>().ok())) else { continue };
                if let Some(l) = c.attribute("outlineLevel").and_then(|v| v.parse::<u8>().ok()).filter(|l| *l > 0) {
                    for i in a..=b.min(a + 16384) {
                        o.outline_cols.push((i, l));
                    }
                }
            }
        }
        if let Some(sp) = root.children().find(|n| n.has_tag_name("sheetProtection")) {
            if matches!(sp.attribute("sheet"), Some("1") | Some("true")) {
                let allowed = |k: &str, default_protected: bool| match sp.attribute(k) {
                    Some("0") | Some("false") => true,
                    Some(_) => false,
                    None => !default_protected,
                };
                o.protection = Some(SheetProtection {
                    sheet: si as u32,
                    password_hash: sp.attribute("password").map(str::to_string),
                    options: crate::sheetopts::ProtectOptions {
                        format_cells: allowed("formatCells", true),
                        format_columns: allowed("formatColumns", true),
                        format_rows: allowed("formatRows", true),
                        insert_rows: allowed("insertRows", true),
                        insert_columns: allowed("insertColumns", true),
                        delete_rows: allowed("deleteRows", true),
                        delete_columns: allowed("deleteColumns", true),
                        sort: allowed("sort", true),
                        auto_filter: allowed("autoFilter", true),
                    },
                });
            }
        }
        for pr in root.descendants().filter(|n| n.has_tag_name("protectedRange")) {
            for part in pr.attribute("sqref").unwrap_or("").split_whitespace() {
                if let Some((_, r0, c0, r1, c1)) = crate::extras::parse_range(part) {
                    o.unlocked.push(CellRange { r0, c0, r1, c1 });
                }
            }
        }
        // Unlocked cells become runs down each column.
        unlocked_cells.sort_by_key(|(r, c)| (*c, *r));
        let mut i = 0;
        while i < unlocked_cells.len() {
            let (r0, c) = unlocked_cells[i];
            let mut r1 = r0;
            while i + 1 < unlocked_cells.len() && unlocked_cells[i + 1] == (r1 + 1, c) {
                r1 += 1;
                i += 1;
            }
            o.unlocked.push(CellRange { r0, c0: c, r1, c1: c });
            i += 1;
        }
        let mut page = PageSetup::new(si as u32);
        if let Some(po) = root.children().find(|n| n.has_tag_name("printOptions")) {
            page.gridlines = matches!(po.attribute("gridLines"), Some("1") | Some("true"));
            page.headings = matches!(po.attribute("headings"), Some("1") | Some("true"));
        }
        let fit_to_page = root
            .descendants()
            .find(|n| n.has_tag_name("pageSetUpPr"))
            .is_some_and(|n| matches!(n.attribute("fitToPage"), Some("1") | Some("true")));
        if let Some(ps) = root.children().find(|n| n.has_tag_name("pageSetup")) {
            page.landscape = ps.attribute("orientation") == Some("landscape");
            if fit_to_page {
                page.fit = if ps.attribute("fitToHeight") == Some("0") { FitMode::Width } else { FitMode::Page };
            }
        }
        if let Some(hf) = root.children().find(|n| n.has_tag_name("headerFooter")) {
            let part = |tag: &str| hf.children().find(|n| n.has_tag_name(tag)).and_then(|n| n.text()).map(crate::sheetopts::parse_header_footer);
            if let Some(h) = part("oddHeader") {
                page.header = h;
            }
            if let Some(f) = part("oddFooter") {
                // CellX's own 「페이지 번호」 comes back as that switch.
                if f[0].is_empty() && f[1] == "&P / &N" && f[2].is_empty() {
                    page.page_numbers = true;
                } else {
                    page.footer = f;
                }
            }
        }
        let brks = |tag: &str| -> Vec<i32> {
            root.children()
                .find(|n| n.has_tag_name(tag))
                .map(|b| b.children().filter(|n| n.has_tag_name("brk")).filter_map(|n| n.attribute("id")?.parse::<i32>().ok()).map(|i| i + 1).collect())
                .unwrap_or_default()
        };
        page.row_breaks = brks("rowBreaks");
        page.col_breaks = brks("colBreaks");
        page.title_rows = titles.get(&si).copied();
        page.print_area = areas.get(&si).copied();
        if !page.is_default() {
            o.page = Some(page);
        }
        out.push(o);
    }
    out
}

fn parse_cell(s: &str) -> Option<(i32, i32)> {
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = s.split_at(split);
    let mut col = 0i32;
    for ch in letters.chars() {
        col = col * 26 + (ch.to_ascii_uppercase() as i32 - 'A' as i32 + 1);
    }
    Some((digits.parse().ok()?, col))
}

pub(crate) fn parse_auto_filter(af: roxmltree::Node, si: u32) -> Option<AutoFilter> {
            let (_, r0, c0, r1, c1) = crate::extras::parse_range(af.attribute("ref")?)?;
            let mut columns = Vec::new();
            for fc in af.children().filter(|n| n.has_tag_name("filterColumn")) {
                let col = c0 + fc.attribute("colId").and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
                if let Some(fs) = fc.children().find(|n| n.has_tag_name("filters")) {
                    let mut values: Vec<String> = fs
                        .children()
                        .filter(|n| n.has_tag_name("filter"))
                        .filter_map(|n| n.attribute("val").map(str::to_string))
                        .collect();
                    if fs.attribute("blank") == Some("1") {
                        values.push(String::new());
                    }
                    columns.push(ColumnFilter { col, values: Some(values), conditions: vec![], and: true });
                } else if let Some(cfs) = fc.children().find(|n| n.has_tag_name("customFilters")) {
                    let conditions = cfs
                        .children()
                        .filter(|n| n.has_tag_name("customFilter"))
                        .map(|n| {
                            let val = n.attribute("val").unwrap_or("").to_string();
                            let op = n.attribute("operator").unwrap_or("equal");
                            let (op, value) = match op {
                                "notEqual" if val.starts_with('*') && val.ends_with('*') && val.len() > 1 => {
                                    (FilterOp::NotContains, val[1..val.len() - 1].to_string())
                                }
                                "notEqual" => (FilterOp::NotEqual, val),
                                "greaterThan" => (FilterOp::GreaterThan, val),
                                "greaterThanOrEqual" => (FilterOp::GreaterThanOrEqual, val),
                                "lessThan" => (FilterOp::LessThan, val),
                                "lessThanOrEqual" => (FilterOp::LessThanOrEqual, val),
                                _ if val.starts_with('*') && val.ends_with('*') && val.len() > 1 => {
                                    (FilterOp::Contains, val[1..val.len() - 1].to_string())
                                }
                                _ if val.ends_with('*') && !val.starts_with('*') => (FilterOp::BeginsWith, val[..val.len() - 1].to_string()),
                                _ if val.starts_with('*') && !val.ends_with('*') => (FilterOp::EndsWith, val[1..].to_string()),
                                _ => (FilterOp::Equal, val),
                            };
                            FilterCondition { op, value }
                        })
                        .collect();
                    columns.push(ColumnFilter { col, values: None, conditions, and: cfs.attribute("and") == Some("1") });
                }
            }
            Some(AutoFilter { sheet: si, range: CellRange { r0, c0, r1, c1 }, columns })
}

/// What IronCalc's table import misses: style info, filter flag, totals row settings.
pub struct TableMeta {
    pub style: ironcalc_base::types::TableStyleInfo,
    pub has_filters: bool,
    /// (totalsRowLabel, totalsRowFunction) per column.
    pub totals: Vec<(Option<String>, Option<String>)>,
}

pub fn read_table_meta(xlsx: &[u8]) -> HashMap<String, TableMeta> {
    let mut out = HashMap::new();
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(xlsx)) else { return out };
    let names: Vec<String> = zip
        .file_names()
        .filter_map(|n| n.ok().map(|n| n.to_string()))
        .filter(|n| n.starts_with("xl/tables/") && n.ends_with(".xml"))
        .collect();
    for part in names {
        let Some(xml) = read_part(&mut zip, &part) else { continue };
        let Ok(doc) = roxmltree::Document::parse(&xml) else { continue };
        let root = doc.root_element();
        let Some(name) = root.attribute("name") else { continue };
        let flag = |n: roxmltree::Node, k: &str, d: bool| n.attribute(k).map(|v| v == "1" || v == "true").unwrap_or(d);
        let style = match root.children().find(|n| n.has_tag_name("tableStyleInfo")) {
            Some(si) => ironcalc_base::types::TableStyleInfo {
                name: si.attribute("name").map(str::to_string),
                show_first_column: flag(si, "showFirstColumn", false),
                show_last_column: flag(si, "showLastColumn", false),
                show_row_stripes: flag(si, "showRowStripes", true),
                show_column_stripes: flag(si, "showColumnStripes", false),
            },
            None => ironcalc_base::types::TableStyleInfo {
                name: None,
                show_first_column: false,
                show_last_column: false,
                show_row_stripes: true,
                show_column_stripes: false,
            },
        };
        let totals = root
            .descendants()
            .filter(|n| n.has_tag_name("tableColumn"))
            .map(|c| (c.attribute("totalsRowLabel").map(str::to_string), c.attribute("totalsRowFunction").map(str::to_string)))
            .collect();
        let has_filters = root.children().any(|n| n.has_tag_name("autoFilter"));
        out.insert(name.to_string(), TableMeta { style, has_filters, totals });
    }
    out
}

/// The filter of a table part (xl/tables/*.xml) by table name.
pub fn read_table_filter(xlsx: &[u8], name: &str) -> Option<AutoFilter> {
    let mut zip = zip::ZipArchive::new(Cursor::new(xlsx)).ok()?;
    let names: Vec<String> = zip
        .file_names()
        .filter_map(|n| n.ok().map(|n| n.to_string()))
        .filter(|n| n.starts_with("xl/tables/") && n.ends_with(".xml"))
        .collect();
    for part in names {
        let Some(xml) = read_part(&mut zip, &part) else { continue };
        let Ok(doc) = roxmltree::Document::parse(&xml) else { continue };
        let root = doc.root_element();
        if root.attribute("name").is_some_and(|n| n.eq_ignore_ascii_case(name)) {
            return root.children().find(|n| n.has_tag_name("autoFilter")).and_then(|af| parse_auto_filter(af, 0));
        }
    }
    None
}

/// AutoFilter and data validation rules of every sheet, read from the sheet XML.
/// (sheet, pivot table definition, its cache definition) of every pivot table.
pub(crate) fn read_pivot_tables(xlsx: &[u8]) -> Vec<(u32, String, String)> {
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(xlsx)) else { return Vec::new() };
    if !zip.file_names().any(|n| n.is_ok_and(|n| n.starts_with("xl/pivotTables/"))) {
        return Vec::new();
    }
    let Some(wb_xml) = read_part(&mut zip, "xl/workbook.xml") else { return Vec::new() };
    let wb_rels = rels_of(&mut zip, "xl/workbook.xml");
    let Ok(wb_doc) = roxmltree::Document::parse(&wb_xml) else { return Vec::new() };
    let parts: Vec<String> = wb_doc
        .descendants()
        .filter(|n| n.has_tag_name("sheet"))
        .filter_map(|n| n.attribute((R_NS, "id")).and_then(|id| wb_rels.get(id)).map(|(_, t)| t.clone()))
        .collect();
    let mut out = Vec::new();
    for (si, part) in parts.iter().enumerate() {
        let rels = rels_of(&mut zip, part);
        let mut tables: Vec<String> = rels.values().filter(|(k, _)| k == "pivotTable").map(|(_, t)| t.clone()).collect();
        tables.sort();
        for t in tables {
            let Some(txml) = read_part(&mut zip, &t) else { continue };
            let trels = rels_of(&mut zip, &t);
            let Some((_, cache)) = trels.values().find(|(k, _)| k == "pivotCacheDefinition") else { continue };
            let Some(cxml) = read_part(&mut zip, cache) else { continue };
            out.push((si as u32, txml, cxml));
        }
    }
    out
}

/// The whole worksheet XML of the sheets whose XML mentions one of `markers`.
pub(crate) fn raw_sheet_xmls(xlsx: &[u8], markers: &[&str]) -> Vec<(u32, String)> {
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(xlsx)) else { return Vec::new() };
    let Some(wb_xml) = read_part(&mut zip, "xl/workbook.xml") else { return Vec::new() };
    let wb_rels = rels_of(&mut zip, "xl/workbook.xml");
    let Ok(wb_doc) = roxmltree::Document::parse(&wb_xml) else { return Vec::new() };
    let parts: Vec<String> = wb_doc
        .descendants()
        .filter(|n| n.has_tag_name("sheet"))
        .filter_map(|n| n.attribute((R_NS, "id")).and_then(|id| wb_rels.get(id)).map(|(_, t)| t.clone()))
        .collect();
    let mut out = Vec::new();
    for (si, part) in parts.iter().enumerate() {
        let Some(xml) = read_part(&mut zip, part) else { continue };
        if markers.iter().any(|m| xml.contains(m)) {
            out.push((si as u32, xml));
        }
    }
    out
}

/// The worksheet XML (without its cells) of the sheets whose XML mentions `marker`.
pub(crate) fn sheet_xmls(xlsx: &[u8], marker: &str) -> Vec<(u32, String)> {
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(xlsx)) else { return Vec::new() };
    let Some(wb_xml) = read_part(&mut zip, "xl/workbook.xml") else { return Vec::new() };
    let wb_rels = rels_of(&mut zip, "xl/workbook.xml");
    let Ok(wb_doc) = roxmltree::Document::parse(&wb_xml) else { return Vec::new() };
    let parts: Vec<String> = wb_doc
        .descendants()
        .filter(|n| n.has_tag_name("sheet"))
        .filter_map(|n| n.attribute((R_NS, "id")).and_then(|id| wb_rels.get(id)).map(|(_, t)| t.clone()))
        .collect();
    let mut out = Vec::new();
    for (si, part) in parts.iter().enumerate() {
        let Some(xml) = read_part(&mut zip, part) else { continue };
        if !xml.contains(marker) {
            continue;
        }
        let (rest, _) = split_sheet_data(&xml);
        out.push((si as u32, rest.into_owned()));
    }
    out
}

pub fn read_sheet_rules(xlsx: &[u8]) -> Vec<(usize, Option<AutoFilter>, Vec<Validation>)> {
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(xlsx)) else { return Vec::new() };
    let Some(wb_xml) = read_part(&mut zip, "xl/workbook.xml") else { return Vec::new() };
    let wb_rels = rels_of(&mut zip, "xl/workbook.xml");
    let Ok(wb_doc) = roxmltree::Document::parse(&wb_xml) else { return Vec::new() };
    let parts: Vec<String> = wb_doc
        .descendants()
        .filter(|n| n.has_tag_name("sheet"))
        .filter_map(|n| n.attribute((R_NS, "id")).and_then(|id| wb_rels.get(id)).map(|(_, t)| t.clone()))
        .collect();
    let mut out = Vec::new();
    for (si, part) in parts.iter().enumerate() {
        let Some(xml) = read_part(&mut zip, part) else { continue };
        if !xml.contains("<autoFilter") && !xml.contains("<dataValidation") {
            continue;
        }
        let (rest, _) = split_sheet_data(&xml);
        let Ok(doc) = roxmltree::Document::parse(&rest) else { continue };
        let root = doc.root_element();
        let filter = root.children().find(|n| n.has_tag_name("autoFilter")).and_then(|af| parse_auto_filter(af, si as u32));
        let mut validations = Vec::new();
        for dv in root.descendants().filter(|n| n.has_tag_name("dataValidation")) {
            let attr = |k: &str| dv.attribute(k).unwrap_or("").to_string();
            let flag = |k: &str, default: bool| dv.attribute(k).map(|v| v == "1" || v == "true").unwrap_or(default);
            let kind = match dv.attribute("type").unwrap_or("none") {
                "whole" => ValidationKind::WholeNumber,
                "decimal" => ValidationKind::Decimal,
                "list" => ValidationKind::List,
                "date" => ValidationKind::Date,
                "time" => ValidationKind::Time,
                "textLength" => ValidationKind::TextLength,
                "custom" => ValidationKind::Custom,
                _ => ValidationKind::Any,
            };
            let op = match dv.attribute("operator").unwrap_or("between") {
                "notBetween" => ValidationOp::NotBetween,
                "equal" => ValidationOp::Equal,
                "notEqual" => ValidationOp::NotEqual,
                "greaterThan" => ValidationOp::GreaterThan,
                "lessThan" => ValidationOp::LessThan,
                "greaterThanOrEqual" => ValidationOp::GreaterThanOrEqual,
                "lessThanOrEqual" => ValidationOp::LessThanOrEqual,
                _ => ValidationOp::Between,
            };
            let formula = |tag: &str| {
                dv.descendants()
                    .find(|n| n.has_tag_name(tag))
                    .map(|n| n.children().filter(|c| c.is_text()).filter_map(|c| c.text()).collect::<String>())
                    .unwrap_or_default()
            };
            validations.push(Validation {
                sheet: si as u32,
                range: attr("sqref"),
                kind,
                op,
                formula1: formula("formula1"),
                formula2: formula("formula2"),
                allow_blank: flag("allowBlank", false),
                in_cell_dropdown: !flag("showDropDown", false),
                show_input: flag("showInputMessage", false),
                input_title: attr("promptTitle"),
                input_message: attr("prompt"),
                show_error: flag("showErrorMessage", false),
                error_style: match dv.attribute("errorStyle") {
                    Some("warning") => ErrorStyle::Warning,
                    Some("information") => ErrorStyle::Information,
                    _ => ErrorStyle::Stop,
                },
                error_title: attr("errorTitle"),
                error_message: attr("error"),
            });
        }
        out.push((si, filter, validations));
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn sheet_data_is_split_off_and_scanned() {
        let xml = r#"<worksheet xmlns="x"><cols><col min="2" max="2" outlineLevel="1"/></cols><sheetData><row r="1"><c r="A1" s="3"><v>1</v></c></row><row r="2" outlineLevel="2" hidden="1"><c r="B2"><f>A1&lt;2</f></c></row></sheetData><autoFilter ref="A1:B2"/></worksheet>"#;
        let (rest, data) = split_sheet_data(xml);
        assert_eq!(rest, r#"<worksheet xmlns="x"><cols><col min="2" max="2" outlineLevel="1"/></cols><autoFilter ref="A1:B2"/></worksheet>"#);
        assert!(data.starts_with("<row r=\"1\">") && data.ends_with("</row>"));
        let unlocked: std::collections::HashSet<usize> = [3].into_iter().collect();
        assert_eq!(scan_sheet_data(data, &unlocked), (vec![(2, 2)], vec![(1, 1)]));
        let none = std::collections::HashSet::new();
        assert_eq!(scan_sheet_data(data, &none), (vec![(2, 2)], vec![]));
        // A prefix, an empty sheetData, and none at all.
        let (rest, data) = split_sheet_data(r#"<x:worksheet><x:sheetData><x:row r="4" outlineLevel="1"/></x:sheetData><x:cols/></x:worksheet>"#);
        assert_eq!((rest.as_ref(), data), (r#"<x:worksheet><x:cols/></x:worksheet>"#, r#"<x:row r="4" outlineLevel="1"/>"#));
        assert_eq!(scan_sheet_data(data, &none).0, vec![(4, 1)]);
        let (rest, data) = split_sheet_data(r#"<worksheet><sheetData/><cols/></worksheet>"#);
        assert_eq!((rest.as_ref(), data), ("<worksheet><cols/></worksheet>", ""));
        let (rest, data) = split_sheet_data("<worksheet><cols/></worksheet>");
        assert_eq!((rest.as_ref(), data), ("<worksheet><cols/></worksheet>", ""));
    }

    use super::*;

    #[test]
    fn resolves_relative_targets() {
        assert_eq!(resolve("xl/worksheets", "../drawings/drawing1.xml"), "xl/drawings/drawing1.xml");
        assert_eq!(resolve("xl", "worksheets/sheet1.xml"), "xl/worksheets/sheet1.xml");
        assert_eq!(resolve("xl", "/xl/worksheets/sheet1.xml"), "xl/worksheets/sheet1.xml");
    }
}
