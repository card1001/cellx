//! Sheet tab operations beyond add, rename and delete: 이동/복사, 탭 색, 숨기기.
//! IronCalc moves and copies the cells; what CellX keeps per sheet index
//! (charts, pictures, filters, outline, protection, page setup, data engine
//! formulas…) is moved or copied along with them.

use std::sync::Arc;

use ironcalc_base::types::{Color, SheetState};
use serde::Serialize;

use crate::error::{CellxError, Result};
use crate::workbook::Workbook;

/// What the sheet tabs show.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct SheetTab {
    pub name: String,
    pub hidden: bool,
    /// `#RRGGBB`, if the tab has a color.
    pub color: Option<String>,
}

/// A theme color slot as an RGB color, for tabs colored in Excel with a theme color.
fn theme_rgb(slot: i32) -> &'static str {
    // Office theme: lt1, dk1, lt2, dk2, accent1..6.
    const OFFICE: [&str; 10] = ["#FFFFFF", "#000000", "#E7E6E6", "#44546A", "#4472C4", "#ED7D31", "#A5A5A5", "#FFC000", "#5B9BD5", "#70AD47"];
    OFFICE.get(slot as usize).copied().unwrap_or("#808080")
}

impl Workbook {
    pub fn sheet_tabs(&self) -> Vec<SheetTab> {
        self.model()
            .workbook
            .worksheets
            .iter()
            .map(|w| SheetTab {
                name: w.get_name(),
                hidden: w.state != SheetState::Visible,
                color: match &w.color {
                    Color::Rgb(rgb) => Some(if rgb.starts_with('#') { rgb.clone() } else { format!("#{rgb}") }),
                    Color::Theme(slot, _) => Some(theme_rgb(*slot).to_string()),
                    Color::None => None,
                },
            })
            .collect()
    }

    /// Renumbers everything CellX keeps per sheet: sheet `i` becomes `map(i)`.
    fn remap_sheets(&mut self, map: impl Fn(u32) -> u32) {
        let bridges = std::mem::take(&mut self.bridges);
        self.bridges = bridges.into_iter().map(|((s, r, c), b)| ((map(s), r, c), b)).collect();
        for x in &mut self.charts {
            x.sheet = map(x.sheet);
        }
        for x in &mut self.pictures {
            x.sheet = map(x.sheet);
        }
        for x in &mut self.pivots {
            x.sheet = map(x.sheet);
        }
        for x in &mut self.filters {
            x.sheet = map(x.sheet);
        }
        for x in &mut self.validations {
            x.sheet = map(x.sheet);
        }
        self.outline_rows = std::mem::take(&mut self.outline_rows).into_iter().map(|((s, i), l)| ((map(s), i), l)).collect();
        self.outline_cols = std::mem::take(&mut self.outline_cols).into_iter().map(|((s, i), l)| ((map(s), i), l)).collect();
        for x in &mut self.protections {
            x.sheet = map(x.sheet);
        }
        for (s, _) in &mut self.unlocked {
            *s = map(*s);
        }
        for x in &mut self.page_setups {
            x.sheet = map(x.sheet);
        }
    }

    /// Copies what CellX keeps for sheet `from` to sheet `to` (a fresh copy of it).
    fn copy_sheet_state(&mut self, from: u32, to: u32) {
        let copied: Vec<_> = self.bridges.iter().filter(|((s, _, _), _)| *s == from).map(|((_, r, c), b)| ((to, *r, *c), b.clone())).collect();
        self.bridges.extend(copied);
        let charts: Vec<_> = self.charts.iter().filter(|c| c.sheet == from).cloned().collect();
        for mut c in charts {
            c.id = self.next_object_id;
            self.next_object_id += 1;
            c.sheet = to;
            self.charts.push(c);
        }
        let pictures: Vec<_> = self.pictures.iter().filter(|p| p.sheet == from).cloned().collect();
        for mut p in pictures {
            p.id = self.next_object_id;
            self.next_object_id += 1;
            p.sheet = to;
            // The image bytes are shared, not copied.
            p.data = crate::pictures::Blob(Arc::clone(&p.data.0));
            self.pictures.push(p);
        }
        let filters: Vec<_> = self.filters.iter().filter(|f| f.sheet == from).cloned().collect();
        self.filters.extend(filters.into_iter().map(|mut f| {
            f.sheet = to;
            f
        }));
        let validations: Vec<_> = self.validations.iter().filter(|v| v.sheet == from).cloned().collect();
        self.validations.extend(validations.into_iter().map(|mut v| {
            v.sheet = to;
            v
        }));
        let rows: Vec<_> = self.outline_rows.iter().filter(|((s, _), _)| *s == from).map(|((_, i), l)| ((to, *i), *l)).collect();
        self.outline_rows.extend(rows);
        let cols: Vec<_> = self.outline_cols.iter().filter(|((s, _), _)| *s == from).map(|((_, i), l)| ((to, *i), *l)).collect();
        self.outline_cols.extend(cols);
        let protections: Vec<_> = self.protections.iter().filter(|p| p.sheet == from).cloned().collect();
        self.protections.extend(protections.into_iter().map(|mut p| {
            p.sheet = to;
            p
        }));
        let unlocked: Vec<_> = self.unlocked.iter().filter(|(s, _)| *s == from).map(|(_, r)| (to, *r)).collect();
        self.unlocked.extend(unlocked);
        let pages: Vec<_> = self.page_setups.iter().filter(|p| p.sheet == from).cloned().collect();
        self.page_setups.extend(pages.into_iter().map(|mut p| {
            p.sheet = to;
            p
        }));
    }

    fn check_sheet(&self, sheet: u32) -> Result<()> {
        if (sheet as usize) < self.sheet_names().len() {
            Ok(())
        } else {
            Err(CellxError::Calc(format!("시트가 없습니다: {sheet}")))
        }
    }

    /// 이동: sheet `from` goes to position `to` (0-based, counted after it was taken out).
    pub fn move_sheet(&mut self, from: u32, to: u32) -> Result<()> {
        self.check_sheet(from)?;
        self.check_sheet(to)?;
        if from == to {
            return Ok(());
        }
        let start = self.begin();
        self.um.move_sheet(from, to)?;
        self.remap_sheets(|i| {
            if i == from {
                to
            } else if from < to && i > from && i <= to {
                i - 1
            } else if to < from && i >= to && i < from {
                i + 1
            } else {
                i
            }
        });
        self.commit(start)?;
        Ok(())
    }

    /// 복사본 만들기: a copy of sheet `sheet` named like Excel's ("Sheet1 (2)"),
    /// placed at position `to`. Returns its index.
    pub fn copy_sheet(&mut self, sheet: u32, to: u32) -> Result<u32> {
        self.check_sheet(sheet)?;
        let start = self.begin();
        self.um.duplicate_sheet(sheet)?;
        // IronCalc puts the copy right after its source and calls it "Sheet1 (1)";
        // Excel starts at (2).
        let copy = sheet + 1;
        let base = self.sheet_names()[sheet as usize].clone();
        let taken: Vec<String> = self.sheet_names().iter().map(|n| n.to_uppercase()).collect();
        let name = (2..)
            .map(|n| {
                let suffix = format!(" ({n})");
                let room = 31usize.saturating_sub(suffix.chars().count());
                format!("{}{suffix}", base.chars().take(room).collect::<String>())
            })
            .find(|c| !taken.contains(&c.to_uppercase()))
            .unwrap_or_default();
        self.um.rename_sheet(copy, &name)?;
        self.remap_sheets(|i| if i >= copy { i + 1 } else { i });
        self.copy_sheet_state(sheet, copy);
        let count = self.sheet_names().len() as u32;
        let to = to.min(count - 1);
        if to != copy {
            self.um.move_sheet(copy, to)?;
            self.remap_sheets(|i| {
                if i == copy {
                    to
                } else if copy < to && i > copy && i <= to {
                    i - 1
                } else if to < copy && i >= to && i < copy {
                    i + 1
                } else {
                    i
                }
            });
        }
        self.commit(start)?;
        Ok(to)
    }

    /// 탭 색: `None` removes it.
    pub fn set_sheet_color(&mut self, sheet: u32, color: Option<String>) -> Result<()> {
        self.check_sheet(sheet)?;
        let start = self.begin();
        let c = match color {
            Some(rgb) => Color::Rgb(rgb.to_uppercase()),
            None => Color::None,
        };
        self.um.set_sheet_color(sheet, &c)?;
        self.commit(start)?;
        Ok(())
    }

    /// 숨기기 / 숨기기 취소. At least one sheet stays visible.
    pub fn set_sheet_hidden(&mut self, sheet: u32, hidden: bool) -> Result<()> {
        self.check_sheet(sheet)?;
        let tabs = self.sheet_tabs();
        if hidden && tabs.iter().enumerate().all(|(i, t)| i as u32 == sheet || t.hidden) {
            return Err(CellxError::Calc("통합 문서에는 표시되는 시트가 하나 이상 있어야 합니다".into()));
        }
        let start = self.begin();
        if hidden {
            self.um.hide_sheet(sheet)?;
        } else {
            self.um.unhide_sheet(sheet)?;
        }
        self.commit(start)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extras::Anchor;
    use crate::workbook::CellRange;

    fn three() -> Workbook {
        let mut wb = Workbook::new().unwrap();
        wb.add_sheet().unwrap();
        wb.add_sheet().unwrap();
        for s in 0..3 {
            wb.set_input(s, 1, 1, &format!("{}", (s + 1) * 10)).unwrap();
        }
        wb.set_input(0, 2, 1, "=Sheet3!A1+A1").unwrap();
        wb
    }

    #[test]
    fn move_keeps_formulas_and_objects_with_their_sheet() {
        let mut wb = three();
        wb.set_page_setup(crate::sheetopts::PageSetup { landscape: true, ..crate::sheetopts::PageSetup::new(2) }).unwrap();
        wb.add_chart(2, crate::extras::ChartKind::Column, CellRange { r0: 1, c0: 1, r1: 1, c1: 1 }).unwrap();
        wb.move_sheet(2, 0).unwrap();
        assert_eq!(wb.sheet_names(), vec!["Sheet3", "Sheet1", "Sheet2"]);
        assert_eq!(wb.input(1, 2, 1).unwrap(), "=Sheet3!A1+A1");
        assert_eq!(wb.model().get_formatted_cell_value(1, 2, 1).unwrap(), "40");
        assert!(wb.page_setup(0).landscape && !wb.page_setup(2).landscape);
        assert_eq!(wb.charts(0).len(), 1);
        wb.undo().unwrap();
        assert_eq!(wb.sheet_names(), vec!["Sheet1", "Sheet2", "Sheet3"]);
        assert!(wb.page_setup(2).landscape);
        assert_eq!(wb.charts(2).len(), 1);
    }

    #[test]
    fn copy_makes_an_excel_style_copy() {
        let mut wb = three();
        wb.set_page_setup(crate::sheetopts::PageSetup { print_area: Some(CellRange { r0: 1, c0: 1, r1: 3, c1: 2 }), ..crate::sheetopts::PageSetup::new(0) }).unwrap();
        let png = crate::pictures::tests_png();
        wb.add_picture(0, 2, 2, png).unwrap();
        let at = wb.copy_sheet(0, 3).unwrap();
        assert_eq!(at, 3);
        assert_eq!(wb.sheet_names(), vec!["Sheet1", "Sheet2", "Sheet3", "Sheet1 (2)"]);
        assert_eq!(wb.input(3, 2, 1).unwrap(), "=Sheet3!A1+A1");
        assert_eq!(wb.page_setup(3).print_area, Some(CellRange { r0: 1, c0: 1, r1: 3, c1: 2 }));
        let (a, b) = (wb.pictures(0), wb.pictures(3));
        assert_eq!((a.len(), b.len()), (1, 1));
        assert_ne!(a[0].id, b[0].id);
        assert_eq!(b[0].anchor, Anchor { ..a[0].anchor.clone() });
        wb.undo().unwrap();
        assert_eq!(wb.sheet_names().len(), 3);
        assert!(wb.pictures(3).is_empty());
    }

    #[test]
    fn tab_colors_and_hidden_sheets_round_trip() {
        let mut wb = three();
        wb.set_sheet_color(1, Some("#ff0000".into())).unwrap();
        wb.set_sheet_hidden(2, true).unwrap();
        wb.set_page_setup(crate::sheetopts::PageSetup {
            print_area: Some(CellRange { r0: 2, c0: 1, r1: 9, c1: 4 }),
            header: ["".into(), "&A 보고서".into(), "".into()],
            footer: ["&D".into(), "".into(), "&P / &N 쪽".into()],
            ..crate::sheetopts::PageSetup::new(0)
        })
        .unwrap();
        assert!(wb.set_sheet_hidden(0, true).is_ok());
        assert!(wb.set_sheet_hidden(1, true).is_err(), "the last visible sheet stays");
        wb.set_sheet_hidden(0, false).unwrap();
        let dir = std::env::temp_dir().join(format!("cellx-tabs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.xlsx");
        wb.save_xlsx(&path).unwrap();
        let back = Workbook::open_xlsx(&path).unwrap();
        let tabs = back.sheet_tabs();
        assert_eq!(tabs[1].color.as_deref(), Some("#FF0000"));
        assert!(tabs[2].hidden && !tabs[0].hidden);
        let p = back.page_setup(0);
        assert_eq!(p.print_area, Some(CellRange { r0: 2, c0: 1, r1: 9, c1: 4 }));
        assert_eq!(p.header[1], "&A 보고서");
        assert_eq!(p.footer, ["&D".to_string(), "".into(), "&P / &N 쪽".into()]);
        assert!(back.model().workbook.defined_names.iter().all(|d| !d.name.starts_with("_xlnm")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn header_footer_codes() {
        use crate::sheetopts::{header_footer_code, parse_header_footer, render_header_footer};
        let parts = ["&D".to_string(), "".into(), "A && B &P/&N".into()];
        let code = header_footer_code(&parts);
        assert_eq!(code, "&L&D&RA && B &P/&N");
        assert_eq!(parse_header_footer(&code), parts);
        // Excel's formatting codes are dropped.
        assert_eq!(parse_header_footer(r#"&C&"맑은 고딕,굵게"&14제목&B!"#)[1], "제목!");
        assert_eq!(render_header_footer("A && B &P/&N", 2, 5, "f", "s", "d", "t"), "A & B 2/5");
    }
}
