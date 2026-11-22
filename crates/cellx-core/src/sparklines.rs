//! 스파크라인: small charts drawn inside cells (꺾은선형, 열, 승패), kept in
//! xlsx as Excel 2010's `x14:sparklineGroups`.

use serde::{Deserialize, Serialize};

use crate::error::{CellxError, Result};
use crate::extras::parse_range;
use crate::ooxml::esc;
use crate::refs::{self, column_name, Axis};
use crate::value::Scalar;
use crate::workbook::{CellRange, Workbook};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SparkKind {
    Line,
    Column,
    WinLoss,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Sparkline {
    pub row: i32,
    pub col: i32,
    /// Its data, with the sheet: `Sheet1!A2:E2`.
    pub range: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SparklineGroup {
    pub id: u32,
    pub sheet: u32,
    pub kind: SparkKind,
    /// `#RRGGBB`.
    pub color: String,
    pub negative_color: String,
    pub markers: bool,
    pub high_low: bool,
    pub items: Vec<Sparkline>,
}

/// A sparkline with its numbers, for drawing.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct SparklineView {
    pub group: u32,
    pub row: i32,
    pub col: i32,
    pub kind: SparkKind,
    pub color: String,
    pub negative_color: String,
    pub markers: bool,
    pub high_low: bool,
    pub values: Vec<Option<f64>>,
}

fn a1(r: CellRange) -> String {
    if r.r0 == r.r1 && r.c0 == r.c1 {
        format!("{}{}", column_name(r.c0), r.r0)
    } else {
        format!("{}{}:{}{}", column_name(r.c0), r.r0, column_name(r.c1), r.r1)
    }
}

impl Workbook {
    fn sparkline_range(&self, range: &str, default: u32) -> Option<(u32, CellRange)> {
        let (sheet, r0, c0, r1, c1) = parse_range(range)?;
        let s = match sheet {
            Some(n) => self.sheet_names().iter().position(|x| x.eq_ignore_ascii_case(&n))? as u32,
            None => default,
        };
        Some((s, CellRange { r0, c0, r1, c1 }.normalized()))
    }

    pub fn sparkline_groups(&self, sheet: u32) -> Vec<SparklineGroup> {
        self.sparklines.iter().filter(|g| g.sheet == sheet).cloned().collect()
    }

    /// Every sparkline of a sheet with its numbers.
    pub fn sparklines(&self, sheet: u32) -> Vec<SparklineView> {
        let mut out = Vec::new();
        for g in self.sparklines.iter().filter(|g| g.sheet == sheet) {
            for s in &g.items {
                let values = match self.sparkline_range(&s.range, sheet) {
                    Some((ds, r)) => {
                        let mut v = Vec::new();
                        for row in r.r0..=r.r1 {
                            for col in r.c0..=r.c1 {
                                v.push(match self.value(ds, row, col) {
                                    Ok(Scalar::Number(n)) => Some(n),
                                    _ => None,
                                });
                            }
                        }
                        v
                    }
                    None => Vec::new(),
                };
                out.push(SparklineView {
                    group: g.id,
                    row: s.row,
                    col: s.col,
                    kind: g.kind,
                    color: g.color.clone(),
                    negative_color: g.negative_color.clone(),
                    markers: g.markers,
                    high_low: g.high_low,
                    values,
                });
            }
        }
        out
    }

    /// 삽입 → 스파크라인: one sparkline per row (or column) of `data`, in the
    /// cells of `location`, as Excel's 스파크라인 만들기 does.
    pub fn add_sparklines(&mut self, sheet: u32, kind: SparkKind, data: &str, location: CellRange) -> Result<SparklineGroup> {
        let loc = location.normalized();
        let (ds, d) = self.sparkline_range(data.trim().trim_start_matches('='), sheet).ok_or_else(|| CellxError::Calc("데이터 범위가 올바르지 않습니다".into()))?;
        let dname = crate::workbook::quote_sheet(self.sheet_names().get(ds as usize).map(String::as_str).unwrap_or(""));
        let (rows, cols) = (d.r1 - d.r0 + 1, d.c1 - d.c0 + 1);
        let cells: Vec<(i32, i32)> = (loc.r0..=loc.r1).flat_map(|r| (loc.c0..=loc.c1).map(move |c| (r, c))).collect();
        let items: Vec<Sparkline> = if cells.len() == 1 {
            vec![Sparkline { row: loc.r0, col: loc.c0, range: format!("{dname}!{}", a1(d)) }]
        } else if loc.c0 == loc.c1 && cells.len() as i32 == rows {
            cells.iter().enumerate().map(|(i, (r, c))| Sparkline { row: *r, col: *c, range: format!("{dname}!{}", a1(CellRange { r0: d.r0 + i as i32, r1: d.r0 + i as i32, ..d })) }).collect()
        } else if loc.r0 == loc.r1 && cells.len() as i32 == cols {
            cells.iter().enumerate().map(|(i, (r, c))| Sparkline { row: *r, col: *c, range: format!("{dname}!{}", a1(CellRange { c0: d.c0 + i as i32, c1: d.c0 + i as i32, ..d })) }).collect()
        } else {
            return Err(CellxError::Calc("위치 범위가 데이터 범위와 맞지 않습니다. 데이터의 행 수(또는 열 수)만큼 셀을 고르세요".into()));
        };
        let start = self.begin();
        // A cell holds one sparkline: new ones replace old ones there.
        for g in &mut self.sparklines {
            if g.sheet == sheet {
                g.items.retain(|s| !loc.contains(s.row, s.col));
            }
        }
        self.sparklines.retain(|g| !g.items.is_empty());
        let group = SparklineGroup {
            id: self.next_object_id,
            sheet,
            kind,
            color: if kind == SparkKind::Column { "#4472C4".into() } else { "#376092".into() },
            negative_color: "#D00000".into(),
            markers: false,
            high_low: false,
            items,
        };
        self.next_object_id += 1;
        self.sparklines.push(group.clone());
        self.commit(start)?;
        Ok(group)
    }

    /// 스파크라인 디자인: kind, colors and markers of a group.
    pub fn update_sparkline_group(&mut self, group: SparklineGroup) -> Result<()> {
        let start = self.begin();
        if let Some(g) = self.sparklines.iter_mut().find(|g| g.id == group.id) {
            *g = SparklineGroup { items: g.items.clone(), sheet: g.sheet, ..group };
        }
        self.commit(start)?;
        Ok(())
    }

    /// 지우기 → 선택한 스파크라인 지우기.
    pub fn delete_sparklines(&mut self, sheet: u32, range: CellRange) -> Result<()> {
        let r = range.normalized();
        let start = self.begin();
        for g in &mut self.sparklines {
            if g.sheet == sheet {
                g.items.retain(|s| !r.contains(s.row, s.col));
            }
        }
        self.sparklines.retain(|g| !g.items.is_empty());
        self.commit(start)?;
        Ok(())
    }

    /// Rows or columns inserted or deleted: sparklines and their data follow.
    pub(crate) fn shift_sparklines(&mut self, sheet: u32, axis: Axis, at: i32, count: i32) {
        let names = self.sheet_names();
        let target = names.get(sheet as usize).cloned().unwrap_or_default();
        for g in &mut self.sparklines {
            let own = names.get(g.sheet as usize).cloned().unwrap_or_default();
            for s in &mut g.items {
                s.range = refs::shift_refs(&format!("={}", s.range), &own, &target, axis, at, count).trim_start_matches('=').to_string();
            }
            if g.sheet == sheet {
                g.items.retain_mut(|s| {
                    let v = if axis == Axis::Row { &mut s.row } else { &mut s.col };
                    if count < 0 && *v >= at && *v < at - count {
                        return false;
                    }
                    if *v >= at {
                        *v += count;
                    }
                    true
                });
            }
        }
        self.sparklines.retain(|g| !g.items.is_empty());
    }

    /// The `<ext>` of a sheet's sparklines, or nothing.
    pub(crate) fn sparkline_ext(&self, sheet: u32) -> String {
        let groups: Vec<&SparklineGroup> = self.sparklines.iter().filter(|g| g.sheet == sheet).collect();
        if groups.is_empty() {
            return String::new();
        }
        let rgb = |c: &str| format!("FF{}", c.trim_start_matches('#').to_uppercase());
        let mut xml = String::new();
        for g in groups {
            let kind = match g.kind {
                SparkKind::Line => "",
                SparkKind::Column => r#" type="column""#,
                SparkKind::WinLoss => r#" type="stacked""#,
            };
            let flags = format!(
                "{}{}{}",
                if g.markers { r#" markers="1""# } else { "" },
                if g.high_low { r#" high="1" low="1""# } else { "" },
                if g.kind != SparkKind::Line { r#" negative="1""# } else { "" }
            );
            xml.push_str(&format!(
                r#"<x14:sparklineGroup displayEmptyCellsAs="gap"{kind}{flags}><x14:colorSeries rgb="{}"/><x14:colorNegative rgb="{}"/><x14:colorAxis rgb="FF000000"/><x14:colorMarkers rgb="{}"/><x14:colorFirst rgb="FFD00000"/><x14:colorLast rgb="FFD00000"/><x14:colorHigh rgb="{}"/><x14:colorLow rgb="{}"/><x14:sparklines>"#,
                rgb(&g.color),
                rgb(&g.negative_color),
                rgb(&g.color),
                rgb(&g.negative_color),
                rgb(&g.negative_color)
            ));
            for s in &g.items {
                xml.push_str(&format!(
                    "<x14:sparkline><xm:f>{}</xm:f><xm:sqref>{}{}</xm:sqref></x14:sparkline>",
                    esc(&s.range),
                    column_name(s.col),
                    s.row
                ));
            }
            xml.push_str("</x14:sparklines></x14:sparklineGroup>");
        }
        format!(
            r#"<ext uri="{{05C60535-1F16-4fd2-B633-F4F36F0B64E0}}" xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main"><x14:sparklineGroups xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main">{xml}</x14:sparklineGroups></ext>"#
        )
    }

    /// Sparklines of an xlsx file.
    pub(crate) fn import_sparklines(&mut self, sheets: &[(u32, String)]) {
        for (sheet, xml) in sheets {
            if !xml.contains("sparklineGroup") {
                continue;
            }
            let Ok(doc) = roxmltree::Document::parse(xml) else { continue };
            for g in doc.descendants().filter(|n| n.has_tag_name("sparklineGroup")) {
                let color = |tag: &str, default: &str| {
                    g.children()
                        .find(|n| n.has_tag_name(tag))
                        .and_then(|n| n.attribute("rgb"))
                        .map(|v| format!("#{}", &v[v.len().saturating_sub(6)..]))
                        .unwrap_or_else(|| default.to_string())
                };
                let kind = match g.attribute("type") {
                    Some("column") => SparkKind::Column,
                    Some("stacked") => SparkKind::WinLoss,
                    _ => SparkKind::Line,
                };
                let items: Vec<Sparkline> = g
                    .descendants()
                    .filter(|n| n.has_tag_name("sparkline"))
                    .filter_map(|s| {
                        let f = s.children().find(|n| n.has_tag_name("f"))?.text()?.to_string();
                        let at = s.children().find(|n| n.has_tag_name("sqref"))?.text()?;
                        let (_, row, col, _, _) = parse_range(at)?;
                        Some(Sparkline { row, col, range: f })
                    })
                    .collect();
                if items.is_empty() {
                    continue;
                }
                let on = |a: &str| matches!(g.attribute(a), Some("1") | Some("true"));
                self.sparklines.push(SparklineGroup {
                    id: self.next_object_id,
                    sheet: *sheet,
                    kind,
                    color: color("colorSeries", "#376092"),
                    negative_color: color("colorNegative", "#D00000"),
                    markers: on("markers"),
                    high_low: on("high") || on("low"),
                    items,
                });
                self.next_object_id += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparklines_per_row_follow_rows_and_round_trip() {
        let mut wb = Workbook::new().unwrap();
        let mut cells = Vec::new();
        for r in 1..=3 {
            for c in 1..=4 {
                cells.push((r, c, ((r * c) as i32 - 5).to_string()));
            }
        }
        wb.set_inputs(0, &cells).unwrap();
        let g = wb.add_sparklines(0, SparkKind::Column, "A1:D3", CellRange { r0: 1, c0: 5, r1: 3, c1: 5 }).unwrap();
        assert_eq!(g.items.len(), 3);
        assert_eq!(g.items[1].range, "Sheet1!A2:D2");
        let v = wb.sparklines(0);
        assert_eq!(v[1].values, vec![Some(-3.0), Some(-1.0), Some(1.0), Some(3.0)]);
        assert!(wb.add_sparklines(0, SparkKind::Line, "A1:D3", CellRange { r0: 1, c0: 6, r1: 2, c1: 6 }).is_err());
        wb.insert_rows(0, 1, 1).unwrap();
        let g = &wb.sparkline_groups(0)[0];
        assert_eq!((g.items[0].row, g.items[0].range.as_str()), (2, "Sheet1!A2:D2"));
        let mut g2 = g.clone();
        g2.markers = true;
        g2.kind = SparkKind::Line;
        wb.update_sparkline_group(g2).unwrap();
        let dir = crate::project::new_work_dir().unwrap();
        let path = dir.join("spark.xlsx");
        wb.save_xlsx(&path).unwrap();
        let back = Workbook::open_xlsx(&path).unwrap();
        let g = &back.sparkline_groups(0)[0];
        assert_eq!((g.kind, g.markers, g.items.len(), g.items[2].row, g.items[2].range.as_str()), (SparkKind::Line, true, 3, 4, "Sheet1!A4:D4"));
        let mut back = back;
        back.delete_sparklines(0, CellRange { r0: 1, c0: 5, r1: 2, c1: 5 }).unwrap();
        assert_eq!(back.sparklines(0).len(), 2);
    }
}
