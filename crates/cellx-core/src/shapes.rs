//! Shapes and text boxes over the sheet (삽입 → 도형, 텍스트 상자), and the
//! shapes of xlsx drawings, kept when a file is opened and saved again.

use serde::{Deserialize, Serialize};

use crate::error::{CellxError, Result};
use crate::extras::Anchor;
use crate::ooxml::{esc, ShapeXml, EMU_PER_PX};
use crate::workbook::{CellRange, Workbook};

/// DrawingML preset geometries CellX draws.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ShapeKind {
    Rect,
    RoundRect,
    Ellipse,
    Triangle,
    Diamond,
    RightArrow,
    Pentagon,
    Line,
    /// A line with an arrowhead at its end.
    Arrow,
    TextBox,
}

impl ShapeKind {
    pub fn preset(self) -> &'static str {
        match self {
            ShapeKind::Rect | ShapeKind::TextBox => "rect",
            ShapeKind::RoundRect => "roundRect",
            ShapeKind::Ellipse => "ellipse",
            ShapeKind::Triangle => "triangle",
            ShapeKind::Diamond => "diamond",
            ShapeKind::RightArrow => "rightArrow",
            ShapeKind::Pentagon => "homePlate",
            ShapeKind::Line | ShapeKind::Arrow => "line",
        }
    }

    fn from_preset(p: &str) -> Option<ShapeKind> {
        Some(match p {
            "rect" => ShapeKind::Rect,
            "roundRect" => ShapeKind::RoundRect,
            "ellipse" => ShapeKind::Ellipse,
            "triangle" => ShapeKind::Triangle,
            "diamond" => ShapeKind::Diamond,
            "rightArrow" => ShapeKind::RightArrow,
            "homePlate" => ShapeKind::Pentagon,
            "line" | "straightConnector1" => ShapeKind::Line,
            _ => return None,
        })
    }

    pub fn is_line(self) -> bool {
        matches!(self, ShapeKind::Line | ShapeKind::Arrow)
    }

    fn label(self) -> &'static str {
        match self {
            ShapeKind::TextBox => "텍스트 상자",
            ShapeKind::Line | ShapeKind::Arrow => "직선 연결선",
            ShapeKind::Rect => "직사각형",
            ShapeKind::RoundRect => "사각형: 둥근 모서리",
            ShapeKind::Ellipse => "타원",
            ShapeKind::Triangle => "이등변 삼각형",
            ShapeKind::Diamond => "다이아몬드",
            ShapeKind::RightArrow => "화살표: 오른쪽",
            ShapeKind::Pentagon => "화살표: 오각형",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Shape {
    pub id: u32,
    pub sheet: u32,
    pub name: String,
    pub kind: ShapeKind,
    pub anchor: Anchor,
    /// `#RRGGBB`, or none for no fill.
    pub fill: Option<String>,
    /// `#RRGGBB`, or none for no line.
    pub line: Option<String>,
    /// Line width in px.
    pub line_width: f64,
    pub text: String,
    pub text_color: String,
    /// Points.
    pub font_size: f64,
    pub bold: bool,
    /// "l", "ctr" or "r".
    pub align: String,
    /// "t", "ctr" or "b".
    pub valign: String,
    /// Lines: drawn from the bottom-left to the top-right instead.
    #[serde(default)]
    pub flip_v: bool,
    #[serde(default)]
    pub flip_h: bool,
}

impl Shape {
    /// Excel's look for a new shape of this kind.
    pub fn new(id: u32, sheet: u32, kind: ShapeKind, anchor: Anchor, name: String) -> Shape {
        let text_box = kind == ShapeKind::TextBox;
        Shape {
            id,
            sheet,
            name,
            kind,
            anchor,
            fill: match kind {
                ShapeKind::TextBox => Some("#FFFFFF".into()),
                k if k.is_line() => None,
                _ => Some("#4472C4".into()),
            },
            line: Some(if text_box { "#BFBFBF".into() } else if kind.is_line() { "#4472C4".into() } else { "#2F528F".into() }),
            line_width: if kind.is_line() { 1.5 } else { 1.0 },
            text: String::new(),
            text_color: if text_box { "#000000".into() } else { "#FFFFFF".into() },
            font_size: 11.0,
            bold: false,
            align: if text_box { "l".into() } else { "ctr".into() },
            valign: if text_box { "t".into() } else { "ctr".into() },
            flip_v: false,
            flip_h: false,
        }
    }

    /// The `<xdr:sp>` element of the shape.
    pub(crate) fn sp_xml(&self, id: u32, cx: i64, cy: i64) -> String {
        let color = |c: &str| format!(r#"<a:srgbClr val="{}"/>"#, c.trim_start_matches('#').to_uppercase());
        let fill = match &self.fill {
            Some(c) if !self.kind.is_line() => format!("<a:solidFill>{}</a:solidFill>", color(c)),
            _ => "<a:noFill/>".into(),
        };
        let width = (self.line_width * EMU_PER_PX) as i64;
        let head = if self.kind == ShapeKind::Arrow { r#"<a:tailEnd type="triangle"/>"# } else { "" };
        let line = match &self.line {
            Some(c) => format!(r#"<a:ln w="{width}"><a:solidFill>{}</a:solidFill>{head}</a:ln>"#, color(c)),
            None => "<a:ln><a:noFill/></a:ln>".into(),
        };
        let flips = format!("{}{}", if self.flip_h { r#" flipH="1""# } else { "" }, if self.flip_v { r#" flipV="1""# } else { "" });
        let body = if self.kind.is_line() {
            String::new()
        } else {
            let size = (self.font_size * 100.0).round() as i64;
            let bold = if self.bold { r#" b="1""# } else { "" };
            let paragraphs: String = self
                .text
                .split('\n')
                .map(|p| {
                    let run = if p.is_empty() {
                        format!(r#"<a:endParaRPr lang="ko-KR" sz="{size}"{bold}/>"#)
                    } else {
                        format!(
                            r#"<a:r><a:rPr lang="ko-KR" sz="{size}"{bold}><a:solidFill>{}</a:solidFill></a:rPr><a:t>{}</a:t></a:r>"#,
                            color(&self.text_color),
                            esc(p)
                        )
                    };
                    format!(r#"<a:p><a:pPr algn="{}"/>{run}</a:p>"#, self.align)
                })
                .collect();
            format!(
                r#"<xdr:txBody><a:bodyPr vertOverflow="clip" horzOverflow="clip" wrap="square" rtlCol="0" anchor="{}"/><a:lstStyle/>{paragraphs}</xdr:txBody>"#,
                match self.valign.as_str() {
                    "ctr" => "ctr",
                    "b" => "b",
                    _ => "t",
                }
            )
        };
        let tx_box = if self.kind == ShapeKind::TextBox { r#" txBox="1""# } else { "" };
        format!(
            r#"<xdr:sp macro="" textlink=""><xdr:nvSpPr><xdr:cNvPr id="{id}" name="{}"/><xdr:cNvSpPr{tx_box}/></xdr:nvSpPr><xdr:spPr><a:xfrm{flips}><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="{}"><a:avLst/></a:prstGeom>{fill}{line}</xdr:spPr>{body}</xdr:sp>"#,
            esc(&self.name),
            self.kind.preset()
        )
    }
}

/// What a drawing's `<xdr:sp>` says, as read from a file.
pub(crate) struct ImportedShape {
    pub kind: ShapeKind,
    pub name: String,
    pub fill: Option<String>,
    pub line: Option<String>,
    pub line_width: Option<f64>,
    pub arrow: bool,
    pub text: String,
    pub text_color: Option<String>,
    pub font_size: Option<f64>,
    pub bold: bool,
    pub align: Option<String>,
    pub valign: Option<String>,
    pub flip_h: bool,
    pub flip_v: bool,
}

/// Office theme colors, for shapes drawn with theme colors.
fn scheme(name: &str) -> Option<&'static str> {
    Some(match name {
        "accent1" => "#4472C4",
        "accent2" => "#ED7D31",
        "accent3" => "#A5A5A5",
        "accent4" => "#FFC000",
        "accent5" => "#5B9BD5",
        "accent6" => "#70AD47",
        "dk1" | "tx1" => "#000000",
        "lt1" | "bg1" => "#FFFFFF",
        "dk2" | "tx2" => "#44546A",
        "lt2" | "bg2" => "#E7E6E6",
        _ => return None,
    })
}

/// The color of an element holding `<a:srgbClr>` or `<a:schemeClr>` (with
/// `lumMod`/`lumOff` approximated, enough for Excel's default shapes).
fn color_of(node: roxmltree::Node) -> Option<String> {
    let c = node.children().find(|n| n.has_tag_name("srgbClr") || n.has_tag_name("schemeClr"))?;
    let base = if c.has_tag_name("srgbClr") { format!("#{}", c.attribute("val")?.to_uppercase()) } else { scheme(c.attribute("val")?)?.to_string() };
    let num = |tag: &str| c.children().find(|n| n.has_tag_name(tag)).and_then(|n| n.attribute("val")).and_then(|v| v.parse::<f64>().ok()).map(|v| v / 100_000.0);
    let (m, o) = (num("lumMod").unwrap_or(1.0), num("lumOff").unwrap_or(0.0));
    if m == 1.0 && o == 0.0 {
        return Some(base);
    }
    let v = u32::from_str_radix(base.trim_start_matches('#'), 16).ok()?;
    let ch = |s: u32| (((v >> s) & 0xFF) as f64 * m + 255.0 * o).clamp(0.0, 255.0).round() as u32;
    Some(format!("#{:02X}{:02X}{:02X}", ch(16), ch(8), ch(0)))
}

fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, tag: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.children().find(|c| c.has_tag_name(tag))
}

pub(crate) fn read_sp(sp: roxmltree::Node) -> Option<ImportedShape> {
    let sppr = child(sp, "spPr")?;
    let preset = child(sppr, "prstGeom").and_then(|g| g.attribute("prst"))?;
    let text_box = sp.descendants().any(|n| n.has_tag_name("cNvSpPr") && n.attribute("txBox") == Some("1"));
    let mut kind = if text_box { ShapeKind::TextBox } else { ShapeKind::from_preset(preset)? };
    let style = child(sp, "style");
    let style_color = |tag: &str| style.and_then(|s| child(s, tag)).and_then(color_of);
    let fill = if child(sppr, "noFill").is_some() {
        None
    } else {
        child(sppr, "solidFill").and_then(color_of).or_else(|| style_color("fillRef"))
    };
    let ln = child(sppr, "ln");
    let line = match ln {
        Some(l) if child(l, "noFill").is_some() => None,
        Some(l) => child(l, "solidFill").and_then(color_of).or_else(|| style_color("lnRef")),
        None => style_color("lnRef"),
    };
    let line_width = ln.and_then(|l| l.attribute("w")).and_then(|w| w.parse::<f64>().ok()).map(|w| w / EMU_PER_PX);
    let arrow = ln.is_some_and(|l| {
        l.children().any(|e| (e.has_tag_name("tailEnd") || e.has_tag_name("headEnd")) && e.attribute("type").is_some_and(|t| t != "none"))
    });
    if kind == ShapeKind::Line && arrow {
        kind = ShapeKind::Arrow;
    }
    let xfrm = child(sppr, "xfrm");
    let flag = |a: &str| xfrm.and_then(|x| x.attribute(a)) == Some("1");
    let body = child(sp, "txBody");
    let text = body
        .map(|b| {
            b.children()
                .filter(|p| p.has_tag_name("p"))
                .map(|p| p.descendants().filter(|t| t.has_tag_name("t")).filter_map(|t| t.text()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let rpr = body.and_then(|b| b.descendants().find(|n| n.has_tag_name("rPr") || n.has_tag_name("endParaRPr")));
    Some(ImportedShape {
        kind,
        name: sp.descendants().find(|n| n.has_tag_name("cNvPr")).and_then(|n| n.attribute("name")).unwrap_or("").to_string(),
        fill,
        line,
        line_width,
        arrow,
        text,
        text_color: rpr.and_then(|r| child(r, "solidFill")).and_then(color_of).or_else(|| style_color("fontRef")),
        font_size: rpr.and_then(|r| r.attribute("sz")).and_then(|s| s.parse::<f64>().ok()).map(|s| s / 100.0),
        bold: rpr.and_then(|r| r.attribute("b")).is_some_and(|b| b == "1" || b == "true"),
        align: body.and_then(|b| b.descendants().find(|n| n.has_tag_name("pPr"))).and_then(|p| p.attribute("algn")).map(str::to_string),
        valign: body.and_then(|b| child(b, "bodyPr")).and_then(|b| b.attribute("anchor")).map(str::to_string),
        flip_h: flag("flipH"),
        flip_v: flag("flipV"),
    })
}

impl Workbook {
    pub fn shapes(&self, sheet: u32) -> Vec<Shape> {
        self.shapes.iter().filter(|s| s.sheet == sheet).cloned().collect()
    }

    fn object_guard(&self, sheet: u32, anchor: &Anchor) -> Result<()> {
        let at = CellRange { r0: anchor.row, c0: anchor.col, r1: anchor.row, c1: anchor.col };
        self.guard(sheet, at, crate::sheetopts::Guard::Objects)
    }

    /// 삽입 → 도형 / 텍스트 상자, at a cell, in Excel's default size.
    pub fn add_shape(&mut self, sheet: u32, row: i32, col: i32, kind: ShapeKind) -> Result<Shape> {
        let (width, height) = match kind {
            ShapeKind::TextBox => (160.0, 60.0),
            k if k.is_line() => (160.0, 0.0),
            _ => (120.0, 80.0),
        };
        let anchor = Anchor { row, col, dx: 0.0, dy: 0.0, width, height };
        self.object_guard(sheet, &anchor)?;
        let id = self.next_object_id;
        self.next_object_id += 1;
        let number = self.shapes.iter().filter(|s| s.sheet == sheet).count() + 1;
        let shape = Shape::new(id, sheet, kind, anchor, format!("{} {number}", kind.label()));
        let start = self.begin();
        self.shapes.push(shape.clone());
        self.commit(start)?;
        Ok(shape)
    }

    /// Moves, resizes, restyles or retypes the text of a shape.
    pub fn update_shape(&mut self, shape: Shape) -> Result<()> {
        let old = self.shapes.iter().find(|s| s.id == shape.id).cloned().ok_or_else(|| CellxError::Calc("도형이 없습니다".into()))?;
        self.object_guard(old.sheet, &old.anchor)?;
        let min = if shape.kind.is_line() { 0.0 } else { 4.0 };
        let fixed = Shape {
            sheet: old.sheet,
            anchor: Anchor {
                width: shape.anchor.width.max(min),
                height: shape.anchor.height.max(min),
                row: shape.anchor.row.max(1),
                col: shape.anchor.col.max(1),
                ..shape.anchor
            },
            font_size: shape.font_size.clamp(1.0, 409.0),
            line_width: shape.line_width.clamp(0.0, 100.0),
            ..shape
        };
        if fixed == old {
            return Ok(());
        }
        let start = self.begin();
        if let Some(s) = self.shapes.iter_mut().find(|s| s.id == fixed.id) {
            *s = fixed;
        }
        self.commit(start)?;
        Ok(())
    }

    pub fn delete_shape(&mut self, id: u32) -> Result<()> {
        if let Some(s) = self.shapes.iter().find(|s| s.id == id) {
            self.object_guard(s.sheet, &s.anchor)?;
        }
        let start = self.begin();
        self.shapes.retain(|s| s.id != id);
        self.commit(start)?;
        Ok(())
    }

    pub(crate) fn shape_parts(&self) -> Vec<(u32, ShapeXml)> {
        self.shapes
            .iter()
            .map(|s| {
                let (from, to) = self.to_marker(s.sheet, &s.anchor);
                let (cx, cy) = ((s.anchor.width * EMU_PER_PX) as i64, (s.anchor.height * EMU_PER_PX) as i64);
                (s.sheet, ShapeXml { from, to, sp: s.sp_xml(0, cx, cy) })
            })
            .collect()
    }

    pub(crate) fn import_shapes(&mut self, xlsx: &[u8]) {
        for (sheet, from, to, ext, sp) in crate::ooxml::read_shapes(xlsx) {
            let sheet = sheet as u32;
            let dx = from.col_off as f64 / EMU_PER_PX;
            let dy = from.row_off as f64 / EMU_PER_PX;
            let (width, height) = match (ext, to) {
                (Some((cx, cy)), _) if cx > 0 || cy > 0 => (cx as f64 / EMU_PER_PX, cy as f64 / EMU_PER_PX),
                (_, Some(to)) => (
                    ((from.col + 1)..=to.col).map(|c| self.col_px(sheet, c)).sum::<f64>() - dx + to.col_off as f64 / EMU_PER_PX,
                    ((from.row + 1)..=to.row).map(|r| self.row_px(sheet, r)).sum::<f64>() - dy + to.row_off as f64 / EMU_PER_PX,
                ),
                _ => (120.0, 80.0),
            };
            let id = self.next_object_id;
            self.next_object_id += 1;
            let anchor = Anchor { row: from.row + 1, col: from.col + 1, dx, dy, width: width.max(0.0), height: height.max(0.0) };
            let number = self.shapes.iter().filter(|s| s.sheet == sheet).count() + 1;
            let name = if sp.name.is_empty() { format!("{} {number}", sp.kind.label()) } else { sp.name.clone() };
            let mut shape = Shape::new(id, sheet, sp.kind, anchor, name);
            shape.fill = if sp.kind.is_line() { None } else { sp.fill };
            shape.line = sp.line;
            if let Some(w) = sp.line_width {
                shape.line_width = w;
            }
            shape.text = sp.text;
            if let Some(c) = sp.text_color {
                shape.text_color = c;
            }
            if let Some(s) = sp.font_size {
                shape.font_size = s;
            }
            shape.bold = sp.bold;
            if let Some(a) = sp.align.filter(|a| ["l", "ctr", "r"].contains(&a.as_str())) {
                shape.align = a;
            }
            if let Some(v) = sp.valign.filter(|a| ["t", "ctr", "b"].contains(&a.as_str())) {
                shape.valign = v;
            }
            shape.flip_h = sp.flip_h;
            shape.flip_v = sp.flip_v;
            let _ = sp.arrow;
            self.shapes.push(shape);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_round_trip_through_xlsx() {
        let mut wb = Workbook::new().unwrap();
        let mut b = wb.add_shape(0, 2, 2, ShapeKind::TextBox).unwrap();
        b.text = "메모\n두 번째 & <줄>".into();
        b.bold = true;
        wb.update_shape(b.clone()).unwrap();
        let mut a = wb.add_shape(0, 8, 2, ShapeKind::Arrow).unwrap();
        a.flip_v = true;
        wb.update_shape(a).unwrap();
        let mut e = wb.add_shape(0, 4, 6, ShapeKind::Ellipse).unwrap();
        e.fill = Some("#FF0000".into());
        e.text = "원".into();
        wb.update_shape(e).unwrap();
        wb.insert_rows(0, 1, 2).unwrap();
        assert_eq!(wb.shapes(0)[0].anchor.row, 4);
        let dir = crate::project::new_work_dir().unwrap();
        let path = dir.join("shapes.xlsx");
        wb.save_xlsx(&path).unwrap();
        let back = Workbook::open_xlsx(&path).unwrap();
        let s = back.shapes(0);
        assert_eq!(s.len(), 3);
        assert_eq!((s[0].kind, s[0].text.as_str(), s[0].bold, s[0].anchor.row), (ShapeKind::TextBox, "메모\n두 번째 & <줄>", true, 4));
        assert_eq!((s[1].kind, s[1].flip_v, s[1].anchor.height), (ShapeKind::Arrow, true, 0.0));
        assert_eq!((s[2].kind, s[2].fill.as_deref(), s[2].text.as_str()), (ShapeKind::Ellipse, Some("#FF0000"), "원"));
        assert!((s[0].anchor.width - 160.0).abs() < 0.5);
        // Undo takes a shape away again.
        let mut wb = back;
        let n = wb.shapes(0).len();
        wb.add_shape(0, 1, 1, ShapeKind::Rect).unwrap();
        wb.undo().unwrap();
        assert_eq!(wb.shapes(0).len(), n);
    }

    #[test]
    fn excel_default_shape_colors() {
        let xml = r#"<xdr:sp xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><xdr:nvSpPr><xdr:cNvPr id="2" name="직사각형 1"/><xdr:cNvSpPr/></xdr:nvSpPr><xdr:spPr><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></xdr:spPr><xdr:style><a:lnRef idx="2"><a:schemeClr val="accent1"><a:shade val="50000"/></a:schemeClr></a:lnRef><a:fillRef idx="1"><a:schemeClr val="accent1"/></a:fillRef><a:effectRef idx="0"><a:schemeClr val="accent1"/></a:effectRef><a:fontRef idx="minor"><a:schemeClr val="lt1"/></a:fontRef></xdr:style><xdr:txBody><a:bodyPr anchor="ctr"/><a:p><a:pPr algn="ctr"/><a:r><a:rPr sz="1400"/><a:t>안녕</a:t></a:r></a:p></xdr:txBody></xdr:sp>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let s = read_sp(doc.root_element()).unwrap();
        assert_eq!(s.kind, ShapeKind::Rect);
        assert_eq!(s.fill.as_deref(), Some("#4472C4"));
        assert_eq!(s.text_color.as_deref(), Some("#FFFFFF"));
        assert_eq!((s.text.as_str(), s.font_size), ("안녕", Some(14.0)));
    }
}
