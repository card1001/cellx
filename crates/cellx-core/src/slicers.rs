//! 슬라이서: buttons that filter a table by one of its columns (삽입 → 슬라이서).
//! The filter itself is the table's AutoFilter; a slicer only shows and sets it.
//! Kept in xlsx as Excel 2013 table slicers.

use serde::{Deserialize, Serialize};

use crate::datatools::ColumnFilter;
use crate::error::{CellxError, Result};
use crate::extras::Anchor;
use crate::ooxml::{SlicerXml, EMU_PER_PX};
use crate::value::Scalar;
use crate::workbook::{CellRange, RecalcStats, Workbook};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Slicer {
    pub id: u32,
    /// The sheet it is drawn on.
    pub sheet: u32,
    /// Also the name of its cache in xlsx (`슬라이서_지역`).
    pub name: String,
    pub caption: String,
    pub table: String,
    pub column: String,
    pub anchor: Anchor,
    /// Buttons side by side.
    pub columns: u32,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct SlicerItem {
    pub text: String,
    pub selected: bool,
    /// Some row with this value is left by the table's other filters.
    pub available: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct SlicerView {
    pub slicer: Slicer,
    pub items: Vec<SlicerItem>,
    /// Some value is filtered out (the 필터 지우기 button is on).
    pub filtered: bool,
}

impl Workbook {
    fn slicer_target(&self, s: &Slicer) -> Option<(u32, CellRange, i32, bool)> {
        let t = self.tables_on_all().into_iter().find(|t| t.name.eq_ignore_ascii_case(&s.table))?;
        let i = t.columns.iter().position(|c| c.eq_ignore_ascii_case(&s.column))?;
        let data = CellRange { r0: t.range.r0 + t.header as i32, r1: t.range.r1 - t.totals as i32, ..t.range };
        Some((t.sheet, data, t.range.c0 + i as i32, t.header))
    }

    fn tables_on_all(&self) -> Vec<crate::sheetopts::TableView> {
        (0..self.sheet_names().len() as u32).flat_map(|s| self.tables_on(s)).collect()
    }

    pub fn slicers(&self, sheet: u32) -> Vec<SlicerView> {
        self.slicers.iter().filter(|s| s.sheet == sheet).map(|s| self.slicer_view(s)).collect()
    }

    fn slicer_view(&self, s: &Slicer) -> SlicerView {
        let Some((ts, data, col, _)) = self.slicer_target(s) else {
            return SlicerView { slicer: s.clone(), items: Vec::new(), filtered: false };
        };
        let filter = self.filters.iter().find(|f| f.sheet == ts);
        let own = filter.and_then(|f| f.columns.iter().find(|c| c.col == col));
        let mut items: Vec<(Scalar, SlicerItem)> = Vec::new();
        for row in data.r0..=data.r1 {
            let text = self.model().get_formatted_cell_value(ts, row, col).unwrap_or_default().trim().to_string();
            let available = filter.is_none_or(|f| {
                f.columns.iter().filter(|c| c.col != col).all(|c| {
                    let shown = self.model().get_formatted_cell_value(ts, row, c.col).unwrap_or_default();
                    let value = self.value(ts, row, c.col).unwrap_or(Scalar::Empty);
                    crate::datatools::column_passes(c, &shown, &value)
                })
            });
            match items.iter_mut().find(|(_, i)| i.text.eq_ignore_ascii_case(&text)) {
                Some((_, i)) => i.available |= available,
                None => {
                    let selected = own.and_then(|c| c.values.as_ref()).is_none_or(|v| v.iter().any(|x| x.trim().eq_ignore_ascii_case(&text)));
                    items.push((self.value(ts, row, col).unwrap_or(Scalar::Empty), SlicerItem { text, selected, available }));
                }
            }
        }
        items.sort_by(|a, b| crate::datatools::compare_values(&a.0, &b.0, false));
        let filtered = items.iter().any(|(_, i)| !i.selected);
        SlicerView { slicer: s.clone(), items: items.into_iter().map(|(_, i)| i).collect(), filtered }
    }

    /// 삽입 → 슬라이서: a slicer for a table's column, next to the table.
    pub fn add_slicer(&mut self, table: &str, column: &str, sheet: u32) -> Result<Slicer> {
        let t = self.tables_on_all().into_iter().find(|t| t.name.eq_ignore_ascii_case(table)).ok_or_else(|| CellxError::Calc(format!("표가 없습니다: {table}")))?;
        let column = t.columns.iter().find(|c| c.eq_ignore_ascii_case(column)).cloned().ok_or_else(|| CellxError::Calc(format!("열이 없습니다: {column}")))?;
        let taken = |n: &str| self.slicers.iter().any(|s| s.name.eq_ignore_ascii_case(n));
        let base = format!("슬라이서_{}", column.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect::<String>());
        let name = std::iter::once(base.clone()).chain((1..).map(|n| format!("{base}{n}"))).find(|n| !taken(n)).unwrap_or(base);
        let n = self.slicers.iter().filter(|s| s.sheet == sheet).count() as f64;
        let anchor = Anchor { row: t.range.r0, col: t.range.c1 + 2, dx: n * 24.0, dy: n * 24.0, width: 192.0, height: 252.0 };
        let slicer = Slicer { id: self.next_object_id, sheet, name, caption: column.clone(), table: t.name.clone(), column, anchor, columns: 1 };
        self.next_object_id += 1;
        let start = self.begin();
        // The table's filter is what the slicer sets.
        if !self.filters.iter().any(|f| f.sheet == t.sheet) {
            let data_end = t.range.r1 - t.totals as i32;
            self.filters.push(crate::datatools::AutoFilter { sheet: t.sheet, range: CellRange { r1: data_end, ..t.range }, columns: Vec::new() });
        }
        self.slicers.push(slicer.clone());
        self.commit(start)?;
        Ok(slicer)
    }

    /// Moves, resizes or renames a slicer, or changes its button columns.
    pub fn update_slicer(&mut self, slicer: Slicer) -> Result<()> {
        let start = self.begin();
        if let Some(s) = self.slicers.iter_mut().find(|s| s.id == slicer.id) {
            *s = Slicer {
                sheet: s.sheet,
                table: s.table.clone(),
                column: s.column.clone(),
                name: s.name.clone(),
                columns: slicer.columns.clamp(1, 20),
                anchor: Anchor { width: slicer.anchor.width.max(40.0), height: slicer.anchor.height.max(40.0), row: slicer.anchor.row.max(1), col: slicer.anchor.col.max(1), ..slicer.anchor },
                ..slicer
            };
        }
        self.commit(start)?;
        Ok(())
    }

    pub fn delete_slicer(&mut self, id: u32) -> Result<()> {
        let start = self.begin();
        self.slicers.retain(|s| s.id != id);
        self.commit(start)?;
        Ok(())
    }

    /// Clicking buttons: the values to show, or `None` for 필터 지우기.
    pub fn slicer_select(&mut self, id: u32, values: Option<Vec<String>>) -> Result<RecalcStats> {
        let s = self.slicers.iter().find(|s| s.id == id).cloned().ok_or_else(|| CellxError::Calc("슬라이서가 없습니다".into()))?;
        let (ts, _, col, _) = self.slicer_target(&s).ok_or_else(|| CellxError::Calc(format!("슬라이서의 표나 열이 없습니다: {}[{}]", s.table, s.column)))?;
        self.set_filter_column(ts, ColumnFilter { col, values, conditions: Vec::new(), and: true })
    }

    pub(crate) fn slicer_parts(&self) -> Vec<(u32, SlicerXml)> {
        self.slicers
            .iter()
            .filter_map(|s| {
                let t = self.model().workbook.tables.values().find(|t| t.name.eq_ignore_ascii_case(&s.table))?;
                let column_id = t.columns.iter().find(|c| c.name.eq_ignore_ascii_case(&s.column))?.id;
                let (from, to) = self.to_marker(s.sheet, &s.anchor);
                Some((
                    s.sheet,
                    SlicerXml {
                        name: s.caption.clone(),
                        cache: s.name.clone(),
                        caption: s.caption.clone(),
                        column: s.column.clone(),
                        table: t.name.clone(),
                        column_id,
                        columns: s.columns,
                        from,
                        to,
                        cx: (s.anchor.width * EMU_PER_PX) as i64,
                        cy: (s.anchor.height * EMU_PER_PX) as i64,
                    },
                ))
            })
            .collect()
    }

    pub(crate) fn import_slicers(&mut self, xlsx: &[u8]) {
        for (sheet, x) in crate::ooxml::read_slicers(xlsx) {
            let sheet = sheet as u32;
            let dx = x.from.col_off as f64 / EMU_PER_PX;
            let dy = x.from.row_off as f64 / EMU_PER_PX;
            let (width, height) = if x.cx > 0 && x.cy > 0 {
                (x.cx as f64 / EMU_PER_PX, x.cy as f64 / EMU_PER_PX)
            } else {
                (
                    ((x.from.col + 1)..=x.to.col).map(|c| self.col_px(sheet, c)).sum::<f64>() - dx + x.to.col_off as f64 / EMU_PER_PX,
                    ((x.from.row + 1)..=x.to.row).map(|r| self.row_px(sheet, r)).sum::<f64>() - dy + x.to.row_off as f64 / EMU_PER_PX,
                )
            };
            let id = self.next_object_id;
            self.next_object_id += 1;
            self.slicers.push(Slicer {
                id,
                sheet,
                name: x.cache,
                caption: if x.caption.is_empty() { x.column.clone() } else { x.caption },
                table: x.table,
                column: x.column,
                anchor: Anchor { row: x.from.row + 1, col: x.from.col + 1, dx, dy, width: width.max(40.0), height: height.max(40.0) },
                columns: x.columns.max(1),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slicer_filters_the_table_and_round_trips() {
        let mut wb = Workbook::new().unwrap();
        let rows = [["지역", "금액"], ["서울", "100"], ["부산", "50"], ["서울", "70"], ["대구", "30"]];
        let mut cells = Vec::new();
        for (i, r) in rows.iter().enumerate() {
            for (j, v) in r.iter().enumerate() {
                cells.push((i as i32 + 1, j as i32 + 1, v.to_string()));
            }
        }
        wb.set_inputs(0, &cells).unwrap();
        wb.create_table(0, CellRange { r0: 1, c0: 1, r1: 5, c1: 2 }, true, "TableStyleMedium2").unwrap();
        let s = wb.add_slicer("표1", "지역", 0).unwrap();
        assert_eq!(s.name, "슬라이서_지역");
        let v = &wb.slicers(0)[0];
        assert_eq!(v.items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(), vec!["대구", "부산", "서울"]);
        assert!(!v.filtered);
        wb.slicer_select(s.id, Some(vec!["서울".into()])).unwrap();
        assert!(wb.model().workbook.worksheet(0).unwrap().rows.iter().any(|r| r.r == 3 && r.hidden));
        let v = &wb.slicers(0)[0];
        assert!(v.filtered && v.items[2].selected && !v.items[0].selected);
        let dir = crate::project::new_work_dir().unwrap();
        let path = dir.join("slicer.xlsx");
        wb.save_xlsx(&path).unwrap();
        let mut back = Workbook::open_xlsx(&path).unwrap();
        let v = &back.slicers(0)[0];
        assert_eq!((v.slicer.table.as_str(), v.slicer.column.as_str(), v.slicer.name.as_str()), ("표1", "지역", "슬라이서_지역"));
        assert!(v.filtered);
        back.slicer_select(v.slicer.id, None).unwrap();
        assert!(!back.slicers(0)[0].filtered);
    }
}
