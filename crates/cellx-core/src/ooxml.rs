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
    pub kind: ChartKind,
    pub legend: bool,
    pub data: ChartData,
    /// Name of the sheet the data comes from (for references).
    pub data_sheet: String,
    pub from: Marker,
    pub to: Marker,
}

#[derive(Default)]
pub struct SheetParts {
    pub memos: Vec<MemoXml>,
    pub charts: Vec<ChartXml>,
    pub filter: Option<AutoFilter>,
    pub validations: Vec<Validation>,
}

impl SheetParts {
    fn is_empty(&self) -> bool {
        self.memos.is_empty() && self.charts.is_empty() && self.filter.is_none() && self.validations.is_empty()
    }
}

fn a1(r: CellRange) -> String {
    format!("{}{}:{}{}", column_name(r.c0), r.r0, column_name(r.c1), r.r1)
}

fn auto_filter_xml(f: &AutoFilter) -> String {
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

/// Adds memos and charts (`parts[i]` belongs to sheet i+1) to an xlsx made by IronCalc.
pub fn add_extras(xlsx: &[u8], parts: &[SheetParts], sheet_names: &[String]) -> Result<Vec<u8>> {
    if parts.iter().all(SheetParts::is_empty) {
        return Ok(xlsx.to_vec());
    }
    let mut zin = zerr(zip::ZipArchive::new(Cursor::new(xlsx)))?;
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..zin.len() {
        let mut f = zerr(zin.by_index(i))?;
        if f.is_dir() {
            continue;
        }
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        let name = zerr(f.name())?.to_string();
        files.push((name, buf));
    }
    let mut map: HashMap<String, Vec<u8>> = files.iter().cloned().collect();
    let mut order: Vec<String> = files.into_iter().map(|(n, _)| n).collect();
    let put = |map: &mut HashMap<String, Vec<u8>>, order: &mut Vec<String>, name: String, data: Vec<u8>| {
        if !map.contains_key(&name) {
            order.push(name.clone());
        }
        map.insert(name, data);
    };

    let mut overrides = String::new();
    let mut chart_no = 0;
    let mut drawing_no = 0;
    let mut memo_no = 0;
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

        if !p.charts.is_empty() {
            drawing_no += 1;
            let mut anchors = String::new();
            let mut drels = Vec::new();
            for (k, ch) in p.charts.iter().enumerate() {
                chart_no += 1;
                let rid = format!("rId{}", k + 1);
                drels.push(format!(
                    r#"<Relationship Id="{rid}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="../charts/chart{chart_no}.xml"/>"#
                ));
                anchors.push_str(&anchor_xml(ch, k as u32 + 2, &rid));
                put(&mut map, &mut order, format!("xl/charts/chart{chart_no}.xml"), chart_xml(ch).into_bytes());
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

        let mut sheet_xml = sheet_xml;
        // autoFilter comes right after sheetData, dataValidations before hyperlinks.
        if let Some(f) = &p.filter {
            let xml = auto_filter_xml(f);
            let at = sheet_xml
                .find("</sheetData>")
                .map(|i| i + "</sheetData>".len())
                .or_else(|| sheet_xml.find("<sheetData/>").map(|i| i + "<sheetData/>".len()));
            if let Some(at) = at {
                sheet_xml.insert_str(at, &xml);
            }
        }
        if !p.validations.is_empty() {
            let xml = validations_xml(&p.validations);
            let at = sheet_xml
                .find("<hyperlinks")
                .or_else(|| sheet_xml.rfind("<extLst"))
                .or_else(|| sheet_xml.rfind("</worksheet>"))
                .unwrap_or(sheet_xml.len());
            sheet_xml.insert_str(at, &xml);
        }
        // drawing and legacyDrawing come before extLst in a worksheet.
        let at = sheet_xml
            .rfind("<extLst")
            .or_else(|| sheet_xml.rfind("</worksheet>"))
            .unwrap_or(sheet_xml.len());
        let new_sheet = format!("{}{}{}", &sheet_xml[..at], tags, &sheet_xml[at..]);
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
    if !filter_names.is_empty() {
        if let Some(wb) = map.get("xl/workbook.xml") {
            let mut wb = String::from_utf8_lossy(wb).to_string();
            if let Some(at) = wb.find("</definedNames>") {
                wb.insert_str(at, &filter_names);
            } else if let Some(at) = wb.find("<calcPr").or_else(|| wb.find("</workbook>")) {
                wb.insert_str(at, &format!("<definedNames>{filter_names}</definedNames>"));
            }
            map.insert("xl/workbook.xml".into(), wb.into_bytes());
        }
    }

    if let Some(ct) = map.get("[Content_Types].xml") {
        let mut ct = String::from_utf8_lossy(ct).to_string();
        if memo_no > 0 && !ct.contains("Extension=\"vml\"") {
            overrides.insert_str(
                0,
                r#"<Default Extension="vml" ContentType="application/vnd.openxmlformats-officedocument.vmlDrawing"/>"#,
            );
        }
        let at = ct.rfind("</Types>").unwrap_or(ct.len());
        ct.insert_str(at, &overrides);
        map.insert("[Content_Types].xml".into(), ct.into_bytes());
    }

    let mut out = Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut out);
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for name in &order {
            zerr(zw.start_file(name.as_str(), opts))?;
            zw.write_all(&map[name])?;
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
        series.push_str(&format!(
            r#"<c:ser><c:idx val="{i}"/><c:order val="{i}"/>{tx}{sppr}{marker}{cat}{val}{smooth}</c:ser>"#
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
pub fn read_charts(xlsx: &[u8]) -> Vec<ImportedChart> {
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(xlsx)) else { return Vec::new() };
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
                    out.push(c);
                }
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
    })
}

/// AutoFilter and data validation rules of every sheet, read from the sheet XML.
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
        let Ok(doc) = roxmltree::Document::parse(&xml) else { continue };
        let root = doc.root_element();
        let filter = root.children().find(|n| n.has_tag_name("autoFilter")).and_then(|af| {
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
            Some(AutoFilter { sheet: si as u32, range: CellRange { r0, c0, r1, c1 }, columns })
        });
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
    use super::*;

    #[test]
    fn resolves_relative_targets() {
        assert_eq!(resolve("xl/worksheets", "../drawings/drawing1.xml"), "xl/drawings/drawing1.xml");
        assert_eq!(resolve("xl", "worksheets/sheet1.xml"), "xl/worksheets/sheet1.xml");
        assert_eq!(resolve("xl", "/xl/worksheets/sheet1.xml"), "xl/worksheets/sheet1.xml");
    }
}
