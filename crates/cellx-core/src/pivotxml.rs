//! Pivot tables in xlsx: CellX's pivots of sheet ranges are written as real
//! Excel pivot tables (a cache that Excel refreshes when it opens the file),
//! and the pivot tables of xlsx files become CellX pivots again.

use crate::extras::{parse_range, Aggregate, PivotDef, PivotSource, PivotValue};
use crate::ooxml::esc;
use crate::refs::column_name;
use crate::value::Scalar;
use crate::workbook::Workbook;

/// One pivot table with its cache, ready to write.
pub struct PivotXml {
    /// The sheet the table is on.
    pub sheet: u32,
    pub cache: String,
    pub table: String,
}

fn subtotal(a: Aggregate) -> &'static str {
    match a {
        Aggregate::Sum => "sum",
        Aggregate::Count => "count",
        Aggregate::Average => "average",
        Aggregate::Max => "max",
        Aggregate::Min => "min",
    }
}

fn aggregate(s: &str) -> Aggregate {
    match s {
        "count" | "countNums" => Aggregate::Count,
        "average" => Aggregate::Average,
        "max" => Aggregate::Max,
        "min" => Aggregate::Min,
        _ => Aggregate::Sum,
    }
}

fn a1(r0: i32, c0: i32, r1: i32, c1: i32) -> String {
    format!("{}{}:{}{}", column_name(c0), r0, column_name(c1), r1)
}

impl Workbook {
    /// The pivots of sheet ranges as xlsx parts; `cache_id` numbers the caches.
    pub(crate) fn pivot_parts(&self) -> Vec<PivotXml> {
        let names = self.sheet_names();
        let mut out = Vec::new();
        for (k, def) in self.pivots.iter().enumerate() {
            let PivotSource::Range { range } = &def.source else { continue };
            let Some((sn, r0, c0, r1, c1)) = parse_range(range) else { continue };
            let Some(src) = (match sn {
                Some(n) => names.iter().position(|x| x.eq_ignore_ascii_case(&n)).map(|i| i as u32),
                None => Some(def.sheet),
            }) else {
                continue;
            };
            let (last_row, last_col) = self.dimensions(src).unwrap_or((r1, c1));
            let (r1, c1) = (r1.min(last_row.max(r0 + 1)), c1.min(last_col.max(c0)));
            let fields = crate::objects::header_names_pub(self, src, r0, c0, c1);
            let index = |name: &str| fields.iter().position(|f| f.eq_ignore_ascii_case(name));
            let axis: Vec<usize> = def.rows.iter().chain(def.columns.iter()).filter_map(|f| index(f)).collect();
            // Cache fields: shared items for the fields on an axis (their values, in order).
            let mut items: Vec<Vec<String>> = vec![Vec::new(); fields.len()];
            let mut cache_fields = String::new();
            for (i, name) in fields.iter().enumerate() {
                let col = c0 + i as i32;
                if axis.contains(&i) {
                    let mut vals: Vec<(Scalar, String)> = Vec::new();
                    for row in (r0 + 1)..=r1 {
                        let text = self.model().get_formatted_cell_value(src, row, col).unwrap_or_default();
                        if !vals.iter().any(|(_, t)| *t == text) {
                            vals.push((self.value(src, row, col).unwrap_or(Scalar::Empty), text));
                        }
                    }
                    vals.sort_by(|a, b| crate::datatools::compare_values(&a.0, &b.0, false));
                    items[i] = vals.iter().map(|(_, t)| t.clone()).collect();
                    let shared: String = items[i].iter().map(|t| if t.is_empty() { "<m/>".to_string() } else { format!(r#"<s v="{}"/>"#, esc(t)) }).collect();
                    cache_fields.push_str(&format!(r#"<cacheField name="{}" numFmtId="0"><sharedItems containsBlank="{}" count="{}">{shared}</sharedItems></cacheField>"#, esc(name), items[i].iter().any(|t| t.is_empty()) as u8, items[i].len()));
                } else {
                    cache_fields.push_str(&format!(r#"<cacheField name="{}" numFmtId="0"><sharedItems/></cacheField>"#, esc(name)));
                }
            }
            let cache = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><pivotCacheDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" refreshOnLoad="1" saveData="0" refreshedBy="CellX" createdVersion="6" refreshedVersion="6" minRefreshableVersion="3" recordCount="{}"><cacheSource type="worksheet"><worksheetSource ref="{}" sheet="{}"/></cacheSource><cacheFields count="{}">{cache_fields}</cacheFields></pivotCacheDefinition>"#,
                (r1 - r0).max(0),
                a1(r0, c0, r1, c1),
                esc(names.get(src as usize).map(String::as_str).unwrap_or("")),
                fields.len()
            );
            // The table: its fields, axes and values.
            let col_field = def.columns.as_deref().and_then(index);
            let pivot_fields: String = (0..fields.len())
                .map(|i| {
                    let data = def.values.iter().any(|v| index(&v.field) == Some(i));
                    let data = if data { r#" dataField="1""# } else { "" };
                    let axis_attr = if def.rows.iter().any(|f| index(f) == Some(i)) {
                        Some("axisRow")
                    } else if col_field == Some(i) {
                        Some("axisCol")
                    } else {
                        None
                    };
                    match axis_attr {
                        Some(a) => {
                            let its: String = (0..items[i].len()).map(|x| format!(r#"<item x="{x}"/>"#)).collect();
                            format!(r#"<pivotField axis="{a}"{data} showAll="0"><items count="{}">{its}<item t="default"/></items></pivotField>"#, items[i].len() + 1)
                        }
                        None => format!(r#"<pivotField{data} showAll="0"/>"#),
                    }
                })
                .collect();
            let row_fields: String = def.rows.iter().filter_map(|f| index(f)).map(|i| format!(r#"<field x="{i}"/>"#)).collect();
            let mut cols: Vec<i32> = col_field.map(|i| i as i32).into_iter().collect();
            if def.values.len() > 1 {
                cols.push(-2);
            }
            let col_fields: String = cols.iter().map(|i| format!(r#"<field x="{i}"/>"#)).collect();
            let data_fields: String = def
                .values
                .iter()
                .filter_map(|v| {
                    let i = index(&v.field)?;
                    Some(format!(
                        r#"<dataField name="{}" fld="{i}" subtotal="{}" baseField="0" baseItem="0"/>"#,
                        esc(&format!("{} : {}", v.agg.label(), v.field)),
                        subtotal(v.agg)
                    ))
                })
                .collect();
            let (rows, colsn) = (def.extent.0.max(1), def.extent.1.max(1));
            let location = a1(def.row, def.col, def.row + rows - 1, def.col + colsn - 1);
            let header_rows = if col_field.is_some() { 2 } else { 1 };
            let table = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><pivotTableDefinition xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" name="피벗 테이블{}" cacheId="{}" applyNumberFormats="0" applyBorderFormats="0" applyFontFormats="0" applyPatternFormats="0" applyAlignmentFormats="0" applyWidthHeightFormats="1" dataCaption="값" updatedVersion="6" minRefreshableVersion="3" useAutoFormatting="1" itemPrintTitles="1" createdVersion="6" indent="0" outline="1" outlineData="1" multipleFieldFilters="0"><location ref="{location}" firstHeaderRow="1" firstDataRow="{header_rows}" firstDataCol="{}"/><pivotFields count="{}">{pivot_fields}</pivotFields>{}{}{}<pivotTableStyleInfo name="PivotStyleLight16" showRowHeaders="1" showColHeaders="1" showRowStripes="0" showColStripes="0" showLastColumn="1"/></pivotTableDefinition>"#,
                k + 1,
                CACHE_ID_BASE + k as u32,
                def.rows.len().max(1),
                fields.len(),
                if row_fields.is_empty() { String::new() } else { format!(r#"<rowFields count="{}">{row_fields}</rowFields>"#, def.rows.len()) },
                if col_fields.is_empty() { String::new() } else { format!(r#"<colFields count="{}">{col_fields}</colFields>"#, cols.len()) },
                if data_fields.is_empty() { String::new() } else { format!(r#"<dataFields count="{}">{data_fields}</dataFields>"#, def.values.len()) },
            );
            out.push(PivotXml { sheet: def.sheet, cache, table });
        }
        out
    }

    /// Pivot tables of an xlsx file over sheet ranges become CellX pivots.
    pub(crate) fn import_pivot_tables(&mut self, xlsx: &[u8]) {
        let names = self.sheet_names();
        for (sheet, table, cache) in crate::ooxml::read_pivot_tables(xlsx) {
            let Ok(t) = roxmltree::Document::parse(&table) else { continue };
            let Ok(c) = roxmltree::Document::parse(&cache) else { continue };
            let Some(ws) = c.descendants().find(|n| n.has_tag_name("worksheetSource")) else { continue };
            let Some(reference) = ws.attribute("ref") else { continue };
            let src_sheet = ws.attribute("sheet").map(str::to_string).or_else(|| names.get(sheet as usize).cloned()).unwrap_or_default();
            let fields: Vec<String> = c.descendants().filter(|n| n.has_tag_name("cacheField")).map(|n| n.attribute("name").unwrap_or("").to_string()).collect();
            let field = |n: roxmltree::Node| n.attribute("x").and_then(|x| x.parse::<i64>().ok()).filter(|x| *x >= 0).and_then(|x| fields.get(x as usize).cloned());
            let rows: Vec<String> = t.descendants().filter(|n| n.has_tag_name("rowFields")).flat_map(|r| r.children().filter(|n| n.has_tag_name("field")).filter_map(field).collect::<Vec<_>>()).collect();
            let columns = t.descendants().filter(|n| n.has_tag_name("colFields")).flat_map(|r| r.children().filter(|n| n.has_tag_name("field")).filter_map(field).collect::<Vec<_>>()).next();
            let values: Vec<PivotValue> = t
                .descendants()
                .filter(|n| n.has_tag_name("dataField"))
                .filter_map(|d| {
                    let f = d.attribute("fld")?.parse::<usize>().ok()?;
                    Some(PivotValue { field: fields.get(f)?.clone(), agg: aggregate(d.attribute("subtotal").unwrap_or("sum")) })
                })
                .collect();
            let Some(loc) = t.descendants().find(|n| n.has_tag_name("location")).and_then(|l| l.attribute("ref")) else { continue };
            let Some((_, lr0, lc0, lr1, lc1)) = parse_range(loc) else { continue };
            if values.is_empty() && rows.is_empty() {
                continue;
            }
            let id = self.next_object_id;
            self.next_object_id += 1;
            self.pivots.push(PivotDef {
                id,
                source: PivotSource::Range { range: format!("{}!{reference}", crate::workbook::quote_sheet(&src_sheet)) },
                rows,
                columns,
                values,
                sheet,
                row: lr0,
                col: lc0,
                extent: (lr1 - lr0 + 1, lc1 - lc0 + 1),
            });
        }
    }
}

/// Pivot caches are numbered from here (Excel's own start at 0 or 1).
pub(crate) const CACHE_ID_BASE: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extras::PivotDef;

    #[test]
    fn range_pivots_are_written_as_excel_pivot_tables_and_read_back() {
        let mut wb = Workbook::new().unwrap();
        let rows = [["지역", "상품", "금액"], ["서울", "A", "100"], ["부산", "A", "50"], ["서울", "B", "30"]];
        let mut cells = Vec::new();
        for (i, r) in rows.iter().enumerate() {
            for (j, v) in r.iter().enumerate() {
                cells.push((i as i32 + 1, j as i32 + 1, v.to_string()));
            }
        }
        wb.set_inputs(0, &cells).unwrap();
        let def = PivotDef {
            id: 0,
            source: PivotSource::Range { range: "Sheet1!A1:C4".into() },
            rows: vec!["지역".into()],
            columns: Some("상품".into()),
            values: vec![PivotValue { field: "금액".into(), agg: Aggregate::Sum }],
            sheet: 0,
            row: 1,
            col: 6,
            extent: (0, 0),
        };
        wb.save_pivot(def, false).unwrap();
        let dir = crate::project::new_work_dir().unwrap();
        let path = dir.join("pivot.xlsx");
        wb.save_xlsx(&path).unwrap();
        // The parts Excel needs are there.
        let mut z = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let mut read = |n: &str| {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut z.by_name(n).unwrap(), &mut s).unwrap();
            s
        };
        let cache = read("xl/pivotCache/pivotCacheDefinition1.xml");
        assert!(cache.contains(r#"<worksheetSource ref="A1:C4" sheet="Sheet1"/>"#));
        assert!(cache.contains(r#"<s v="부산"/><s v="서울"/>"#));
        let table = read("xl/pivotTables/pivotTable1.xml");
        assert!(table.contains(r#"<rowFields count="1"><field x="0"/></rowFields>"#));
        assert!(table.contains(r#"<colFields count="1"><field x="1"/></colFields>"#));
        assert!(table.contains(r#"subtotal="sum""#));
        let wbx = read("xl/workbook.xml");
        assert!(wbx.contains(r#"<pivotCache cacheId="1" r:id="#));
        roxmltree::Document::parse(&wbx).unwrap();
        // Opening it again gives a live pivot.
        let back = Workbook::open_xlsx(&path).unwrap();
        let p = &back.pivots()[0];
        assert_eq!((p.rows.clone(), p.columns.clone(), p.values[0].field.as_str(), p.row, p.col), (vec!["지역".to_string()], Some("상품".to_string()), "금액", 1, 6));
        assert!(matches!(&p.source, PivotSource::Range { range } if range == "Sheet1!A1:C4"));
    }
}

/// Writes /tmp/cellx-features.xlsx with every kind of object, to open in other programs.
#[test]
#[ignore]
fn write_feature_sample() {
    use crate::extras::{ChartKind, PivotDef};
    use crate::shapes::ShapeKind;
    use crate::sparklines::SparkKind;
    use crate::workbook::CellRange;
    let mut wb = Workbook::new().unwrap();
    let rows = [["지역", "상품", "금액", "1월", "2월", "3월"], ["서울", "A", "100", "1", "3", "2"], ["부산", "A", "50", "4", "2", "5"], ["서울", "B", "30", "2", "2", "1"]];
    let mut cells = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        for (j, v) in r.iter().enumerate() {
            cells.push((i as i32 + 1, j as i32 + 1, v.to_string()));
        }
    }
    wb.set_inputs(0, &cells).unwrap();
    wb.create_table(0, CellRange { r0: 1, c0: 1, r1: 4, c1: 6 }, true, "TableStyleMedium2").unwrap();
    wb.add_sparklines(0, SparkKind::Line, "D2:F4", CellRange { r0: 2, c0: 7, r1: 4, c1: 7 }).unwrap();
    let mut t = wb.add_shape(0, 8, 2, ShapeKind::TextBox).unwrap();
    t.text = "텍스트 상자".into();
    wb.update_shape(t).unwrap();
    wb.add_shape(0, 8, 6, ShapeKind::Ellipse).unwrap();
    wb.add_shape(0, 14, 2, ShapeKind::Arrow).unwrap();
    wb.add_slicer("표1", "지역", 0).unwrap();
    let c = wb.add_chart(0, ChartKind::Column, CellRange { r0: 1, c0: 3, r1: 4, c1: 3 }).unwrap();
    let mut c2 = c.clone();
    c2.trendline = true;
    wb.update_chart(c2).unwrap();
    wb.save_pivot(
        PivotDef {
            id: 0,
            source: PivotSource::Range { range: "Sheet1!A1:C4".into() },
            rows: vec!["지역".into()],
            columns: None,
            values: vec![PivotValue { field: "금액".into(), agg: Aggregate::Sum }],
            sheet: 0,
            row: 20,
            col: 1,
            extent: (0, 0),
        },
        true,
    )
    .unwrap();
    wb.set_inputs(0, &[(30, 1, "0.1".into()), (31, 1, "100".into()), (32, 1, "=A31*A30".into()), (30, 3, "=A32".into()), (31, 2, "0.2".into()), (32, 2, "0.3".into())]).unwrap();
    wb.add_data_table(0, CellRange { r0: 30, c0: 2, r1: 32, c1: 3 }, None, Some("A30")).unwrap();
    wb.set_scenario(crate::whatif::Scenario { sheet: 0, name: "높음".into(), comment: String::new(), cells: vec![(31, 1, "200".into())] }).unwrap();
    wb.add_custom_view("기본", 0).unwrap();
    wb.protect_book(None).unwrap();
    wb.save_xlsx(std::path::Path::new("/tmp/cellx-features.xlsx")).unwrap();
}
