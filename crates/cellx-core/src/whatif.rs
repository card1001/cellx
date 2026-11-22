//! 가상 분석: 데이터 표 (Excel's TABLE()) and 시나리오 관리자.
//!
//! A data table's results are values CellX works out by putting each input
//! value in the input cell(s), recalculating, and reading the formula, as
//! Excel does. They are written outside the undo history, like data engine
//! results, at the end of every recalculation.

use serde::{Deserialize, Serialize};

use crate::error::{CellxError, Result};
use crate::refs::column_name;
use crate::value::Scalar;
use crate::workbook::{CellRange, RecalcStats, Workbook};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DataTable {
    pub id: u32,
    pub sheet: u32,
    /// The whole table: input values in the first row and/or column, the
    /// formula(s) in the other; results fill the rest.
    pub range: CellRange,
    /// 행 입력 셀: takes the values of the first row.
    pub row_input: Option<(i32, i32)>,
    /// 열 입력 셀: takes the values of the first column.
    pub col_input: Option<(i32, i32)>,
}

impl DataTable {
    pub fn results(&self) -> CellRange {
        CellRange { r0: self.range.r0 + 1, c0: self.range.c0 + 1, ..self.range }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Scenario {
    pub sheet: u32,
    pub name: String,
    pub comment: String,
    /// 변경 셀 and the values the scenario gives them.
    pub cells: Vec<(i32, i32, String)>,
}

fn addr(r: i32, c: i32) -> String {
    format!("{}{r}", column_name(c))
}

fn parse_cell(s: &str) -> Option<(i32, i32)> {
    let (_, r0, c0, _, _) = crate::extras::parse_range(s.trim().trim_start_matches('='))?;
    Some((r0, c0))
}

/// How a value reads back as input.
fn as_input(v: &Scalar) -> String {
    match v {
        Scalar::Empty => String::new(),
        Scalar::Number(n) => {
            let s = format!("{n}");
            if s.contains('e') {
                format!("{n:e}")
            } else {
                s
            }
        }
        Scalar::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Scalar::Text(t) => {
            if t.starts_with('=') || t.starts_with('\'') || t.parse::<f64>().is_ok() || matches!(t.to_uppercase().as_str(), "TRUE" | "FALSE") {
                format!("'{t}")
            } else {
                t.clone()
            }
        }
        Scalar::Error(e) => e.clone(),
    }
}

impl Workbook {
    pub fn data_tables(&self, sheet: u32) -> Vec<DataTable> {
        self.data_tables.iter().filter(|t| t.sheet == sheet).cloned().collect()
    }

    /// 가상 분석 → 데이터 표: the selection is the whole table.
    pub fn add_data_table(&mut self, sheet: u32, range: CellRange, row_input: Option<&str>, col_input: Option<&str>) -> Result<RecalcStats> {
        let r = range.normalized();
        let row_input = row_input.filter(|s| !s.trim().is_empty()).map(|s| parse_cell(s).ok_or_else(|| CellxError::Calc("행 입력 셀이 올바르지 않습니다".into()))).transpose()?;
        let col_input = col_input.filter(|s| !s.trim().is_empty()).map(|s| parse_cell(s).ok_or_else(|| CellxError::Calc("열 입력 셀이 올바르지 않습니다".into()))).transpose()?;
        if row_input.is_none() && col_input.is_none() {
            return Err(CellxError::Calc("행 입력 셀이나 열 입력 셀을 하나 이상 지정하세요".into()));
        }
        if r.r1 <= r.r0 || r.c1 <= r.c0 {
            return Err(CellxError::Calc("데이터 표는 입력 값과 수식을 포함해 두 행, 두 열 이상이어야 합니다".into()));
        }
        for (row, col) in row_input.iter().chain(col_input.iter()) {
            if r.contains(*row, *col) {
                return Err(CellxError::Calc("입력 셀은 데이터 표 밖에 있어야 합니다".into()));
            }
        }
        let t = DataTable { id: self.next_object_id, sheet, range: r, row_input, col_input };
        self.next_object_id += 1;
        let start = self.begin();
        // Results are values; whatever was there goes.
        self.um.range_clear_contents(&t.results().area(sheet))?;
        self.data_tables.retain(|x| !(x.sheet == sheet && x.results().intersects(&t.results())));
        self.data_tables.push(t);
        self.commit(start)
    }

    pub fn delete_data_table(&mut self, sheet: u32, row: i32, col: i32) -> Result<RecalcStats> {
        let start = self.begin();
        if let Some(t) = self.data_tables.iter().find(|t| t.sheet == sheet && t.range.contains(row, col)).cloned() {
            self.um.range_clear_contents(&t.results().area(sheet))?;
            self.data_tables.retain(|x| x.id != t.id);
        }
        self.commit(start)
    }

    pub fn data_table_at(&self, sheet: u32, row: i32, col: i32) -> Option<DataTable> {
        self.data_tables.iter().find(|t| t.sheet == sheet && t.results().contains(row, col)).cloned()
    }

    /// Works every data table out again; returns the result cells that changed,
    /// with their new input. Called at the end of each recalculation pass.
    pub(crate) fn compute_data_tables(&mut self) -> Vec<((u32, i32, i32), String)> {
        let mut changed = Vec::new();
        let tables = self.data_tables.clone();
        for t in tables {
            let s = t.sheet;
            let res = t.results();
            let inputs: Vec<(i32, i32)> = t.row_input.iter().chain(t.col_input.iter()).copied().collect();
            let saved: Vec<String> = inputs.iter().map(|(r, c)| self.input(s, *r, *c).unwrap_or_default()).collect();
            let two_d = t.row_input.is_some() && t.col_input.is_some();
            let mut out = Vec::new();
            for row in res.r0..=res.r1 {
                for col in res.c0..=res.c1 {
                    let mut set = Vec::new();
                    if let Some(ri) = t.row_input {
                        set.push((ri, as_input(&self.value(s, t.range.r0, col).unwrap_or(Scalar::Empty))));
                    }
                    if let Some(ci) = t.col_input {
                        set.push((ci, as_input(&self.value(s, row, t.range.c0).unwrap_or(Scalar::Empty))));
                    }
                    // Where the formula is: the corner (2D), the top row (values down
                    // the first column) or the first column (values across the top).
                    let formula = if two_d {
                        (t.range.r0, t.range.c0)
                    } else if t.col_input.is_some() {
                        (t.range.r0, col)
                    } else {
                        (row, t.range.c0)
                    };
                    let model = self.um.get_model_mut();
                    let mut keys = Vec::new();
                    for ((r, c), v) in &set {
                        let _ = model.set_user_input(s, *r, *c, v.clone());
                        keys.push((s, *r, *c));
                    }
                    model.evaluate_changed(&keys);
                    let v = crate::value::from_ironcalc(model.get_cell_value_by_index(s, formula.0, formula.1).unwrap_or(ironcalc_base::cell::CellValue::None));
                    out.push(((s, row, col), as_input(&v)));
                }
            }
            let model = self.um.get_model_mut();
            let mut keys = Vec::new();
            for ((r, c), v) in inputs.iter().zip(saved) {
                let _ = model.set_user_input(s, *r, *c, v);
                keys.push((s, *r, *c));
            }
            model.evaluate_changed(&keys);
            for (k, v) in out {
                if self.input(k.0, k.1, k.2).unwrap_or_default() != v {
                    changed.push((k, v));
                }
            }
        }
        changed
    }

    // ------------------------------------------------------------ scenarios

    pub fn scenarios(&self, sheet: u32) -> Vec<Scenario> {
        self.scenarios.iter().filter(|s| s.sheet == sheet).cloned().collect()
    }

    /// 시나리오 추가 / 편집 (same name replaces).
    pub fn set_scenario(&mut self, scenario: Scenario) -> Result<()> {
        if scenario.name.trim().is_empty() {
            return Err(CellxError::Calc("시나리오 이름을 입력하세요".into()));
        }
        if scenario.cells.is_empty() || scenario.cells.len() > 32 {
            return Err(CellxError::Calc("변경 셀은 1개에서 32개까지 지정할 수 있습니다".into()));
        }
        let start = self.begin();
        match self.scenarios.iter_mut().find(|s| s.sheet == scenario.sheet && s.name.eq_ignore_ascii_case(&scenario.name)) {
            Some(s) => *s = scenario,
            None => self.scenarios.push(scenario),
        }
        self.commit(start)?;
        Ok(())
    }

    pub fn delete_scenario(&mut self, sheet: u32, name: &str) -> Result<()> {
        let start = self.begin();
        self.scenarios.retain(|s| !(s.sheet == sheet && s.name.eq_ignore_ascii_case(name)));
        self.commit(start)?;
        Ok(())
    }

    /// 표시: puts a scenario's values in its cells (one undo step).
    pub fn show_scenario(&mut self, sheet: u32, name: &str) -> Result<RecalcStats> {
        let s = self.scenarios.iter().find(|s| s.sheet == sheet && s.name.eq_ignore_ascii_case(name)).cloned().ok_or_else(|| CellxError::Calc(format!("시나리오가 없습니다: {name}")))?;
        self.set_inputs(sheet, &s.cells)
    }

    /// 요약: a new sheet with every scenario's values and the result cells each gives.
    pub fn scenario_summary(&mut self, sheet: u32, results: &[(i32, i32)]) -> Result<u32> {
        self.book_guard()?;
        let list = self.scenarios(sheet);
        if list.is_empty() {
            return Err(CellxError::Calc("시나리오가 없습니다".into()));
        }
        let mut changing: Vec<(i32, i32)> = Vec::new();
        for s in &list {
            for (r, c, _) in &s.cells {
                if !changing.contains(&(*r, *c)) {
                    changing.push((*r, *c));
                }
            }
        }
        // Each scenario's results, worked out with its values in place.
        let current: Vec<String> = changing.iter().map(|(r, c)| self.input(sheet, *r, *c).unwrap_or_default()).collect();
        let shown = |wb: &Workbook, r: i32, c: i32| as_input(&wb.value(sheet, r, c).unwrap_or(Scalar::Empty));
        let now: Vec<String> = results.iter().map(|(r, c)| shown(self, *r, *c)).collect();
        let now_changing: Vec<String> = changing.iter().map(|(r, c)| shown(self, *r, *c)).collect();
        let mut columns: Vec<(String, Vec<String>, Vec<String>)> = Vec::new();
        for s in &list {
            let model = self.um.get_model_mut();
            let mut keys = Vec::new();
            for (r, c, v) in &s.cells {
                let _ = model.set_user_input(sheet, *r, *c, v.clone());
                keys.push((sheet, *r, *c));
            }
            model.evaluate_changed(&keys);
            let vals = changing.iter().map(|(r, c)| shown(self, *r, *c)).collect();
            let res = results.iter().map(|(r, c)| shown(self, *r, *c)).collect();
            columns.push((s.name.clone(), vals, res));
            let model = self.um.get_model_mut();
            let mut keys = Vec::new();
            for ((r, c), v) in changing.iter().zip(&current) {
                let _ = model.set_user_input(sheet, *r, *c, v.clone());
                keys.push((sheet, *r, *c));
            }
            model.evaluate_changed(&keys);
        }
        let start = self.begin();
        self.um.new_sheet()?;
        let new = self.sheet_names().len() as u32 - 1;
        let taken: Vec<String> = self.sheet_names().iter().map(|n| n.to_uppercase()).collect();
        let name = std::iter::once("시나리오 요약".to_string()).chain((2..).map(|n| format!("시나리오 요약 {n}"))).find(|n| !taken.contains(&n.to_uppercase())).unwrap_or_default();
        self.um.rename_sheet(new, &name)?;
        let mut cells = vec![(2, 2, "시나리오 요약".to_string()), (3, 3, "현재 값:".to_string())];
        for (i, (n, _, _)) in columns.iter().enumerate() {
            cells.push((3, 4 + i as i32, format!("'{n}")));
        }
        let mut row = 4;
        cells.push((row, 2, "변경 셀:".into()));
        for (k, (r, c)) in changing.iter().enumerate() {
            row += 1;
            cells.push((row, 2, format!("${}${}", column_name(*c), r)));
            cells.push((row, 3, now_changing[k].clone()));
            for (i, (_, vals, _)) in columns.iter().enumerate() {
                cells.push((row, 4 + i as i32, vals[k].clone()));
            }
        }
        if !results.is_empty() {
            row += 1;
            cells.push((row, 2, "결과 셀:".into()));
            for (k, (r, c)) in results.iter().enumerate() {
                row += 1;
                cells.push((row, 2, format!("${}${}", column_name(*c), r)));
                cells.push((row, 3, now[k].clone()));
                for (i, (_, _, res)) in columns.iter().enumerate() {
                    cells.push((row, 4 + i as i32, res[k].clone()));
                }
            }
        }
        row += 2;
        cells.push((row, 2, "참고: 현재 값 열은 시나리오 요약 보고서를 만들 때의 변경 셀 값입니다.".into()));
        for (r, c, v) in &cells {
            self.write_input(new, *r, *c, v, true)?;
        }
        self.um.update_range_style(&CellRange { r0: 2, c0: 2, r1: 2, c1: 2 }.area(new), "font.b", "true")?;
        self.um.update_range_style(&CellRange { r0: 3, c0: 2, r1: 3, c1: 3 + columns.len() as i32 }.area(new), "fill.fg_color", "#D9E1F2")?;
        self.commit(start)?;
        Ok(new)
    }

    // ------------------------------------------------------------ xlsx

    /// `<scenarios>` of a sheet, or nothing.
    pub(crate) fn scenarios_xml(&self, sheet: u32) -> String {
        let list = self.scenarios(sheet);
        if list.is_empty() {
            return String::new();
        }
        let esc = crate::ooxml::esc;
        let body: String = list
            .iter()
            .map(|s| {
                let cells: String = s.cells.iter().map(|(r, c, v)| format!(r#"<inputCells r="{}" val="{}"/>"#, addr(*r, *c), esc(v))).collect();
                format!(r#"<scenario name="{}" count="{}" user="CellX" comment="{}">{cells}</scenario>"#, esc(&s.name), s.cells.len(), esc(&s.comment))
            })
            .collect();
        format!(r#"<scenarios current="0" show="0">{body}</scenarios>"#)
    }

    /// The `<f>` each data table writes in its first result cell.
    pub(crate) fn data_table_formulas(&self, sheet: u32) -> Vec<(String, String)> {
        self.data_tables(sheet)
            .iter()
            .map(|t| {
                let res = t.results();
                let first = addr(res.r0, res.c0);
                let reference = format!("{first}:{}", addr(res.r1, res.c1));
                let f = match (t.row_input, t.col_input) {
                    (Some(ri), Some(ci)) => format!(r#"<f t="dataTable" ref="{reference}" dt2D="1" dtr="1" r1="{}" r2="{}"/>"#, addr(ri.0, ri.1), addr(ci.0, ci.1)),
                    (Some(ri), None) => format!(r#"<f t="dataTable" ref="{reference}" dt2D="0" dtr="1" r1="{}"/>"#, addr(ri.0, ri.1)),
                    (None, Some(ci)) => format!(r#"<f t="dataTable" ref="{reference}" dt2D="0" dtr="0" r1="{}"/>"#, addr(ci.0, ci.1)),
                    (None, None) => String::new(),
                };
                (first, f)
            })
            .collect()
    }

    pub(crate) fn import_whatif(&mut self, xlsx: &[u8]) {
        for (sheet, xml) in crate::ooxml::raw_sheet_xmls(xlsx, &["t=\"dataTable\"", "<scenarios"]) {
            // Data tables: <c r="B2" …><f t="dataTable" ref="B2:D5" dt2D="1" dtr="1" r1="B1" r2="A1"/>
            let mut at = 0;
            while let Some(i) = xml[at..].find("t=\"dataTable\"") {
                let pos = at + i;
                at = pos + 1;
                let start = xml[..pos].rfind("<f").unwrap_or(pos);
                let end = xml[pos..].find('>').map_or(xml.len(), |e| pos + e);
                let tag = &xml[start..end];
                let attr = |name: &str| {
                    let key = format!(" {name}=\"");
                    tag.find(&key).and_then(|k| {
                        let v = &tag[k + key.len()..];
                        v.find('"').map(|e| v[..e].to_string())
                    })
                };
                let Some(reference) = attr("ref") else { continue };
                let Some((_, r0, c0, r1, c1)) = crate::extras::parse_range(&reference) else { continue };
                let two = attr("dt2D").as_deref() == Some("1");
                let row_dir = attr("dtr").as_deref() == Some("1");
                let i1 = attr("r1").and_then(|v| parse_cell(&v));
                let i2 = attr("r2").and_then(|v| parse_cell(&v));
                let (row_input, col_input) = if two { (i1, i2) } else if row_dir { (i1, None) } else { (None, i1) };
                let range = CellRange { r0: r0 - 1, c0: c0 - 1, r1, c1 };
                if range.r0 < 1 || range.c0 < 1 {
                    continue;
                }
                self.data_tables.push(DataTable { id: self.next_object_id, sheet, range, row_input, col_input });
                self.next_object_id += 1;
            }
            if let Some(a) = xml.find("<scenarios") {
                let b = xml[a..].find("</scenarios>").map_or(xml.len(), |e| a + e + "</scenarios>".len());
                if let Ok(doc) = roxmltree::Document::parse(&xml[a..b]) {
                    for s in doc.root_element().children().filter(|n| n.has_tag_name("scenario")) {
                        let cells = s
                            .children()
                            .filter(|n| n.has_tag_name("inputCells"))
                            .filter_map(|n| {
                                let (r, c) = parse_cell(n.attribute("r")?)?;
                                Some((r, c, n.attribute("val").unwrap_or("").to_string()))
                            })
                            .collect();
                        self.scenarios.push(Scenario {
                            sheet,
                            name: s.attribute("name").unwrap_or("").to_string(),
                            comment: s.attribute("comment").unwrap_or("").to_string(),
                            cells,
                        });
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_tables_follow_inputs_and_round_trip() {
        let mut wb = Workbook::new().unwrap();
        // B1: rate, B2: amount, B3: =B2*B1
        wb.set_inputs(0, &[(1, 2, "0.1".into()), (2, 2, "100".into()), (3, 2, "=B2*B1".into())]).unwrap();
        // One input: rates down D2:D4, formula in E1.
        wb.set_inputs(0, &[(1, 5, "=B3".into()), (2, 4, "0.1".into()), (3, 4, "0.2".into()), (4, 4, "0.3".into())]).unwrap();
        wb.add_data_table(0, CellRange { r0: 1, c0: 4, r1: 4, c1: 5 }, None, Some("B1")).unwrap();
        assert_eq!(wb.value(0, 4, 5).unwrap(), Scalar::Number(30.0));
        // Two inputs: amounts across H1:I1, rates down G2:G3, formula in G1.
        wb.set_inputs(0, &[(1, 7, "=B3".into()), (1, 8, "100".into()), (1, 9, "200".into()), (2, 7, "0.5".into()), (3, 7, "1".into())]).unwrap();
        wb.add_data_table(0, CellRange { r0: 1, c0: 7, r1: 3, c1: 9 }, Some("B2"), Some("B1")).unwrap();
        assert_eq!(wb.value(0, 3, 9).unwrap(), Scalar::Number(200.0));
        assert_eq!(wb.value(0, 2, 9).unwrap(), Scalar::Number(100.0));
        // The inputs are left as they were, and the tables follow the model.
        assert_eq!(wb.value(0, 1, 2).unwrap(), Scalar::Number(0.1));
        wb.set_input(0, 2, 2, "1000").unwrap();
        assert_eq!(wb.value(0, 4, 5).unwrap(), Scalar::Number(300.0));
        assert_eq!(wb.value(0, 3, 9).unwrap(), Scalar::Number(200.0));
        wb.set_input(0, 3, 2, "=B2*B1*2").unwrap();
        assert_eq!(wb.value(0, 3, 9).unwrap(), Scalar::Number(400.0));
        let dir = crate::project::new_work_dir().unwrap();
        let path = dir.join("dt.xlsx");
        wb.save_xlsx(&path).unwrap();
        let mut back = Workbook::open_xlsx(&path).unwrap();
        assert_eq!(back.data_tables(0).len(), 2);
        assert_eq!(back.data_tables(0)[1].row_input, Some((2, 2)));
        back.set_input(0, 1, 9, "300").unwrap();
        assert_eq!(back.value(0, 3, 9).unwrap(), Scalar::Number(600.0));
        back.delete_data_table(0, 2, 8).unwrap();
        assert_eq!(back.data_tables(0).len(), 1);
        assert_eq!(back.value(0, 2, 8).unwrap(), Scalar::Empty);
    }

    #[test]
    fn scenarios_show_summarise_and_round_trip() {
        let mut wb = Workbook::new().unwrap();
        wb.set_inputs(0, &[(1, 1, "10".into()), (2, 1, "3".into()), (3, 1, "=A1*A2".into())]).unwrap();
        wb.set_scenario(Scenario { sheet: 0, name: "낙관".into(), comment: String::new(), cells: vec![(1, 1, "20".into()), (2, 1, "5".into())] }).unwrap();
        wb.set_scenario(Scenario { sheet: 0, name: "비관".into(), comment: "최악".into(), cells: vec![(1, 1, "5".into()), (2, 1, "1".into())] }).unwrap();
        let s = wb.scenario_summary(0, &[(3, 1)]).unwrap();
        assert_eq!(wb.sheet_names()[s as usize], "시나리오 요약");
        assert_eq!(wb.model().get_formatted_cell_value(s, 3, 4).unwrap(), "낙관");
        assert_eq!(wb.input(s, 8, 4).unwrap(), "100");
        assert_eq!(wb.input(s, 8, 5).unwrap(), "5");
        // The model is as it was.
        assert_eq!(wb.value(0, 3, 1).unwrap(), Scalar::Number(30.0));
        wb.show_scenario(0, "낙관").unwrap();
        assert_eq!(wb.value(0, 3, 1).unwrap(), Scalar::Number(100.0));
        wb.undo().unwrap();
        assert_eq!(wb.value(0, 3, 1).unwrap(), Scalar::Number(30.0));
        let dir = crate::project::new_work_dir().unwrap();
        let path = dir.join("sc.xlsx");
        wb.save_xlsx(&path).unwrap();
        let back = Workbook::open_xlsx(&path).unwrap();
        let list = back.scenarios(0);
        assert_eq!(list.len(), 2);
        assert_eq!((list[1].name.as_str(), list[1].comment.as_str(), list[1].cells[0].2.as_str()), ("비관", "최악", "5"));
    }
}
