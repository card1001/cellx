//! A CellX workbook: the Excel-compatible calculation engine (IronCalc) plus the
//! data engine (DuckDB), joined by the formula → SQL bridge.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Instant;

use ironcalc_base::types::{Color, HorizontalAlignment};
use ironcalc_base::Model;
use serde::{Deserialize, Serialize};

use crate::bridge::{self, BridgeValue, CellRef, CellResolver};
use crate::data::{ColumnFilter, DataStore, Page, Sort, TableInfo};
use crate::error::{CellxError, Result};
use crate::value::{from_ironcalc, Scalar};

pub const LOCALE: &str = "en";
pub const LANGUAGE: &str = "en";
pub const TIMEZONE: &str = "Asia/Seoul";

/// Passes of "evaluate → refresh bridge calls" before giving up on convergence.
const MAX_BRIDGE_PASSES: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeCell {
    /// What the user typed, e.g. `=SUMIFS(sales[amount], sales[region], B1)`.
    pub formula: String,
    /// What the calculation engine currently holds, e.g. `=1234`.
    #[serde(skip)]
    pub rewritten: Option<String>,
    #[serde(skip)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CellView {
    pub row: i32,
    pub col: i32,
    pub text: String,
    /// Numbers and dates are right-aligned in the grid, like Excel.
    pub numeric: bool,
    pub error: bool,
    pub bridge: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<CellStyle>,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct CellStyle {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Viewport {
    pub cells: Vec<CellView>,
    /// Pixel widths of the requested columns, in order.
    pub col_widths: Vec<f64>,
    /// Pixel heights of the requested rows, in order.
    pub row_heights: Vec<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CellDetail {
    pub input: String,
    pub text: String,
    pub bridge_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RecalcStats {
    pub passes: usize,
    pub bridge_calls: usize,
    pub bridge_cache_hits: usize,
    pub calc_ms: f64,
    pub bridge_ms: f64,
}

#[derive(Debug, Clone)]
struct Edit {
    sheet: u32,
    row: i32,
    col: i32,
    before: String,
    after: String,
}

struct ModelCells<'m, 'a> {
    model: &'m Model<'a>,
    sheet: u32,
}

impl CellResolver for ModelCells<'_, '_> {
    fn cell_value(&self, cell: &CellRef) -> Scalar {
        let sheet = match &cell.sheet {
            None => Some(self.sheet),
            Some(name) => self
                .model
                .workbook
                .get_worksheet_names()
                .iter()
                .position(|n| n.eq_ignore_ascii_case(name))
                .map(|i| i as u32),
        };
        match sheet {
            Some(s) => self
                .model
                .get_cell_value_by_index(s, cell.row, cell.column)
                .map(from_ironcalc)
                .unwrap_or(Scalar::Error("#REF!".into())),
            None => Scalar::Error("#REF!".into()),
        }
    }
}

pub struct Workbook {
    model: Model<'static>,
    data: DataStore,
    data_path: Option<PathBuf>,
    bridges: BTreeMap<(u32, i32, i32), BridgeCell>,
    cache: HashMap<String, BridgeValue>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    pub last_recalc: RecalcStats,
}

impl Workbook {
    /// A new, empty workbook with one sheet and an in-memory data engine.
    pub fn new() -> Result<Self> {
        let model = Model::new_empty("Book1", LOCALE, TIMEZONE, LANGUAGE)?;
        Self::with_model(model, DataStore::open_in_memory()?, None)
    }

    /// Opens an xlsx file. Cached values from Excel are kept until the first recalculation.
    pub fn open_xlsx(path: &Path) -> Result<Self> {
        let model = load_xlsx(path)?;
        Self::with_model(model, DataStore::open_in_memory()?, None)
    }

    pub(crate) fn with_model(
        model: Model<'static>,
        data: DataStore,
        data_path: Option<PathBuf>,
    ) -> Result<Self> {
        Ok(Workbook {
            model,
            data,
            data_path,
            bridges: BTreeMap::new(),
            cache: HashMap::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            last_recalc: RecalcStats::default(),
        })
    }

    pub(crate) fn set_bridges(&mut self, bridges: BTreeMap<(u32, i32, i32), BridgeCell>) {
        self.bridges = bridges;
    }

    pub(crate) fn bridges(&self) -> &BTreeMap<(u32, i32, i32), BridgeCell> {
        &self.bridges
    }

    pub fn model(&self) -> &Model<'static> {
        &self.model
    }

    pub fn data(&mut self) -> &mut DataStore {
        &mut self.data
    }

    pub fn data_path(&self) -> Option<&Path> {
        self.data_path.as_deref()
    }

    /// Moves the data engine from memory to a file (needed before saving a project).
    pub fn persist_data_to(&mut self, path: &Path) -> Result<()> {
        if self.data_path.as_deref() == Some(path) {
            return Ok(());
        }
        let file_store = DataStore::open(path)?;
        drop(file_store);
        // Copy every table through an ATTACH of the new file.
        let p = path.to_string_lossy().replace('\'', "''");
        let tables = self.data.table_names()?;
        let conn_sql = {
            let mut s = format!("ATTACH '{p}' AS _dest;");
            for t in &tables {
                let q = crate::data::quote_ident(t);
                s.push_str(&format!("CREATE OR REPLACE TABLE _dest.{q} AS SELECT * FROM {q};"));
            }
            s.push_str("DETACH _dest;");
            s
        };
        self.data.execute_internal(&conn_sql)?;
        self.data = DataStore::open(path)?;
        self.data_path = Some(path.to_path_buf());
        self.cache.clear();
        Ok(())
    }

    // ------------------------------------------------------------ sheets

    pub fn sheet_names(&self) -> Vec<String> {
        self.model.workbook.get_worksheet_names()
    }

    pub fn add_sheet(&mut self) -> Result<u32> {
        let (_, index) = self.model.new_sheet();
        Ok(index)
    }

    pub fn rename_sheet(&mut self, sheet: u32, name: &str) -> Result<()> {
        self.model.rename_sheet_by_index(sheet, name)?;
        Ok(())
    }

    // ------------------------------------------------------------ cells

    /// What the formula bar shows for a cell.
    pub fn input(&self, sheet: u32, row: i32, col: i32) -> Result<String> {
        if let Some(b) = self.bridges.get(&(sheet, row, col)) {
            return Ok(b.formula.clone());
        }
        Ok(self.model.get_localized_cell_content(sheet, row, col)?)
    }

    pub fn detail(&self, sheet: u32, row: i32, col: i32) -> Result<CellDetail> {
        Ok(CellDetail {
            input: self.input(sheet, row, col)?,
            text: self.model.get_formatted_cell_value(sheet, row, col)?,
            bridge_error: self
                .bridges
                .get(&(sheet, row, col))
                .and_then(|b| b.error.clone()),
        })
    }

    pub fn value(&self, sheet: u32, row: i32, col: i32) -> Result<Scalar> {
        Ok(from_ironcalc(self.model.get_cell_value_by_index(sheet, row, col)?))
    }

    /// Sets a cell from user input (a value or a formula) and recalculates.
    pub fn set_input(&mut self, sheet: u32, row: i32, col: i32, input: &str) -> Result<RecalcStats> {
        let before = self.input(sheet, row, col)?;
        if before == input {
            return Ok(RecalcStats::default());
        }
        self.write_input(sheet, row, col, input)?;
        self.undo.push(Edit { sheet, row, col, before, after: input.to_string() });
        self.redo.clear();
        self.recalc()
    }

    /// Sets many cells at once (paste, imports) with a single recalculation.
    pub fn set_inputs(&mut self, sheet: u32, cells: &[(i32, i32, String)]) -> Result<RecalcStats> {
        for (row, col, input) in cells {
            let before = self.input(sheet, *row, *col)?;
            self.write_input(sheet, *row, *col, input)?;
            self.undo.push(Edit { sheet, row: *row, col: *col, before, after: input.clone() });
        }
        self.redo.clear();
        self.recalc()
    }

    fn write_input(&mut self, sheet: u32, row: i32, col: i32, input: &str) -> Result<()> {
        let is_bridge = input.starts_with('=')
            && !bridge::find_calls(input, &|n| self.data.has_table(n)).is_empty();
        if is_bridge {
            self.bridges.insert(
                (sheet, row, col),
                BridgeCell { formula: input.to_string(), rewritten: None, error: None },
            );
            // Placeholder until the bridge pass fills in the value.
            self.model.set_user_input(sheet, row, col, "=0".to_string())?;
        } else {
            self.bridges.remove(&(sheet, row, col));
            self.model.set_user_input(sheet, row, col, input.to_string())?;
        }
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn undo(&mut self) -> Result<Option<(u32, i32, i32)>> {
        let Some(e) = self.undo.pop() else { return Ok(None) };
        self.write_input(e.sheet, e.row, e.col, &e.before)?;
        let at = (e.sheet, e.row, e.col);
        self.redo.push(e);
        self.recalc()?;
        Ok(Some(at))
    }

    pub fn redo(&mut self) -> Result<Option<(u32, i32, i32)>> {
        let Some(e) = self.redo.pop() else { return Ok(None) };
        self.write_input(e.sheet, e.row, e.col, &e.after)?;
        let at = (e.sheet, e.row, e.col);
        self.undo.push(e);
        self.recalc()?;
        Ok(Some(at))
    }

    // ------------------------------------------------------------ recalculation

    /// Evaluates the workbook, then refreshes every bridge call whose inputs changed,
    /// repeating until no bridge result changes.
    pub fn recalc(&mut self) -> Result<RecalcStats> {
        let mut stats = RecalcStats::default();
        for _ in 0..MAX_BRIDGE_PASSES {
            stats.passes += 1;
            let t = Instant::now();
            self.model.evaluate();
            stats.calc_ms += t.elapsed().as_secs_f64() * 1000.0;

            let t = Instant::now();
            let changed = self.refresh_bridges(&mut stats)?;
            stats.bridge_ms += t.elapsed().as_secs_f64() * 1000.0;
            if changed.is_empty() {
                break;
            }
            for ((sheet, row, col), text) in changed {
                self.model.set_user_input(sheet, row, col, text)?;
            }
        }
        self.last_recalc = stats.clone();
        Ok(stats)
    }

    fn refresh_bridges(&mut self, stats: &mut RecalcStats) -> Result<Vec<((u32, i32, i32), String)>> {
        let mut changed = Vec::new();
        let keys: Vec<(u32, i32, i32)> = self.bridges.keys().copied().collect();
        for key in keys {
            let formula = self.bridges[&key].formula.clone();
            let (rewritten, error) = self.rewrite(&formula, key.0, stats);
            let cell = self.bridges.get_mut(&key).expect("bridge cell exists");
            cell.error = error;
            if cell.rewritten.as_deref() != Some(rewritten.as_str()) {
                cell.rewritten = Some(rewritten.clone());
                changed.push((key, rewritten));
            }
        }
        Ok(changed)
    }

    /// Replaces every bridge call in `formula` by its current result.
    fn rewrite(&mut self, formula: &str, sheet: u32, stats: &mut RecalcStats) -> (String, Option<String>) {
        let data = &mut self.data;
        let calls = bridge::find_calls(formula, &|n| data.has_table(n));
        let mut out = String::with_capacity(formula.len());
        let mut last = 0;
        let mut first_error = None;
        for call in calls {
            let result = call.and_then(|call| {
                let resolver = ModelCells { model: &self.model, sheet };
                let compiled = bridge::compile(&call, &mut self.data, &resolver)?;
                let versions: Vec<u64> = compiled.tables.iter().map(|t| self.data.version(t)).collect();
                let key = format!("{}\u{1e}{versions:?}", compiled.cache_key());
                stats.bridge_calls += 1;
                let value = match self.cache.get(&key) {
                    Some(v) => {
                        stats.bridge_cache_hits += 1;
                        v.clone()
                    }
                    None => {
                        let v = bridge::execute(&compiled, &self.data)?;
                        if self.cache.len() > 50_000 {
                            self.cache.clear();
                        }
                        self.cache.insert(key, v.clone());
                        v
                    }
                };
                Ok((call.span, value))
            });
            match result {
                Ok((span, value)) => {
                    out.push_str(&formula[last..span.0]);
                    out.push_str(&value.to_formula_literal());
                    last = span.1;
                }
                Err(e) => {
                    if first_error.is_none() {
                        first_error = Some(e.to_string());
                    }
                }
            }
        }
        if let Some(err) = first_error {
            // One failed call makes the whole cell an error, with the reason kept for the UI.
            return ("=#VALUE!".to_string(), Some(err));
        }
        out.push_str(&formula[last..]);
        (out, None)
    }

    // ------------------------------------------------------------ viewport for the grid

    pub fn viewport(&self, sheet: u32, row0: i32, col0: i32, row1: i32, col1: i32) -> Result<Viewport> {
        let ws = self.model.workbook.worksheet(sheet)?;
        let mut cells = Vec::new();
        for row in row0..=row1 {
            for col in col0..=col1 {
                if ws.cell(row, col).is_none() {
                    continue;
                }
                let text = self.model.get_formatted_cell_value(sheet, row, col)?;
                let value = self.model.get_cell_value_by_index(sheet, row, col)?;
                let scalar = from_ironcalc(value);
                let style = self.model.get_style_for_cell(sheet, row, col).ok().and_then(|s| {
                    let mut cs = CellStyle {
                        bold: s.font.b,
                        italic: s.font.i,
                        underline: s.font.u,
                        color: rgb(&s.font.color),
                        fill: rgb(&s.fill.color),
                        align: None,
                    };
                    if let Some(a) = &s.alignment {
                        cs.align = match a.horizontal {
                            HorizontalAlignment::Left => Some("left".into()),
                            HorizontalAlignment::Right => Some("right".into()),
                            HorizontalAlignment::Center | HorizontalAlignment::CenterContinuous => {
                                Some("center".into())
                            }
                            _ => None,
                        };
                    }
                    (cs != CellStyle::default()).then_some(cs)
                });
                if text.is_empty() && style.is_none() {
                    continue;
                }
                cells.push(CellView {
                    row,
                    col,
                    text,
                    numeric: matches!(scalar, Scalar::Number(_)),
                    error: matches!(scalar, Scalar::Error(_)),
                    bridge: self.bridges.contains_key(&(sheet, row, col)),
                    style,
                });
            }
        }
        let col_widths = (col0..=col1)
            .map(|c| self.model.get_column_width(sheet, c).unwrap_or(90.0))
            .collect();
        let row_heights = (row0..=row1)
            .map(|r| self.model.get_row_height(sheet, r).unwrap_or(25.0))
            .collect();
        Ok(Viewport { cells, col_widths, row_heights })
    }

    /// Last used row and column of a sheet (for scroll extents).
    pub fn dimensions(&self, sheet: u32) -> Result<(i32, i32)> {
        let ws = self.model.workbook.worksheet(sheet)?;
        let mut max_row = 0;
        let mut max_col = 0;
        for (row, col, _) in ws.sheet_data.cells() {
            max_row = max_row.max(row);
            max_col = max_col.max(col);
        }
        Ok((max_row, max_col))
    }

    pub fn set_column_width(&mut self, sheet: u32, col: i32, px: f64) -> Result<()> {
        self.model.set_column_width(sheet, col, px)?;
        Ok(())
    }

    // ------------------------------------------------------------ data sheets

    pub fn import_data(&mut self, path: &Path, table: &str) -> Result<TableInfo> {
        let info = self.data.import_file(path, table)?;
        self.reclassify_bridges()?;
        self.recalc()?;
        Ok(info)
    }

    /// Runs a user SELECT and stores its result as a new data sheet.
    pub fn create_table_from_query(&mut self, table: &str, sql: &str) -> Result<TableInfo> {
        self.data.validate_user_select(sql)?;
        let info = self
            .data
            .create_table_from_sql(table, sql.trim().trim_end_matches(';'))?;
        self.reclassify_bridges()?;
        self.recalc()?;
        Ok(info)
    }

    pub fn drop_table(&mut self, table: &str) -> Result<()> {
        self.data.drop_table(table)?;
        self.recalc()?;
        Ok(())
    }

    pub fn tables(&mut self) -> Result<Vec<TableInfo>> {
        let names = self.data.table_names()?;
        names.iter().map(|n| self.data.table_info(n)).collect()
    }

    pub fn data_page(
        &mut self,
        table: &str,
        offset: u64,
        limit: u64,
        sort: Option<&Sort>,
        filters: &[ColumnFilter],
    ) -> Result<Page> {
        self.data.page(table, offset, limit, sort, filters)
    }

    pub fn update_data_cell(&mut self, table: &str, rid: i64, column: &str, text: &str) -> Result<RecalcStats> {
        self.data.update_cell(table, rid, column, text)?;
        self.recalc()
    }

    /// After a table appears, formulas typed earlier may now be bridge formulas.
    fn reclassify_bridges(&mut self) -> Result<()> {
        let cells: Vec<(u32, i32, i32)> = self
            .model
            .get_all_cells()
            .into_iter()
            .map(|c| (c.index, c.row, c.column))
            .collect();
        for (sheet, row, col) in cells {
            if self.bridges.contains_key(&(sheet, row, col)) {
                continue;
            }
            let content = self.model.get_localized_cell_content(sheet, row, col)?;
            if content.starts_with('=')
                && content.contains('[')
                && !bridge::find_calls(&content, &|n| self.data.has_table(n)).is_empty()
            {
                self.write_input(sheet, row, col, &content)?;
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ files

    /// Saves the workbook as xlsx. Bridge formulas are written with their current
    /// results inlined (`=ROUND(1234.5,0)`), so Excel shows the same values.
    pub fn save_xlsx(&self, path: &Path) -> Result<()> {
        save_xlsx(&self.model, path)
    }

    pub fn bridge_formula_count(&self) -> usize {
        self.bridges.len()
    }
}

fn rgb(c: &Color) -> Option<String> {
    match c {
        Color::Rgb(s) => {
            let s = s.trim_start_matches('#');
            // ARGB → RGB
            let s = if s.len() == 8 { &s[2..] } else { s };
            if s.eq_ignore_ascii_case("000000") || s.is_empty() {
                None
            } else {
                Some(format!("#{s}"))
            }
        }
        _ => None,
    }
}

pub fn load_xlsx(path: &Path) -> Result<Model<'static>> {
    let p = path
        .to_str()
        .ok_or_else(|| CellxError::Xlsx(format!("경로를 읽을 수 없습니다: {}", path.display())))?;
    ironcalc::import::load_from_xlsx(p, LOCALE, TIMEZONE, LANGUAGE)
        .map_err(|e| CellxError::Xlsx(e.to_string()))
}

pub fn save_xlsx(model: &Model, path: &Path) -> Result<()> {
    // IronCalc refuses to overwrite: write next to the target, then replace it.
    let tmp = path.with_extension("cellx-tmp.xlsx");
    if tmp.exists() {
        std::fs::remove_file(&tmp)?;
    }
    let p = tmp
        .to_str()
        .ok_or_else(|| CellxError::Xlsx(format!("경로를 쓸 수 없습니다: {}", path.display())))?;
    ironcalc::export::save_to_xlsx(model, p).map_err(|e| CellxError::Xlsx(e.to_string()))?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book() -> Workbook {
        let mut wb = Workbook::new().unwrap();
        wb.data()
            .create_table_from_sql(
                "sales",
                "SELECT * FROM (VALUES ('서울', 100.0::DOUBLE), ('부산', 250.0), ('서울', 300.0)) AS t(region, amount)",
            )
            .unwrap();
        wb
    }

    #[test]
    fn plain_formulas_use_the_calculation_engine() {
        let mut wb = Workbook::new().unwrap();
        wb.set_input(0, 1, 1, "2").unwrap();
        wb.set_input(0, 2, 1, "=A1*21").unwrap();
        assert_eq!(wb.value(0, 2, 1).unwrap(), Scalar::Number(42.0));
        assert_eq!(wb.input(0, 2, 1).unwrap(), "=A1*21");
    }

    #[test]
    fn bridge_formula_follows_cell_inputs() {
        let mut wb = book();
        wb.set_input(0, 1, 2, "서울").unwrap(); // B1
        wb.set_input(0, 2, 2, "=SUMIFS(sales[amount], sales[region], B1) * 2").unwrap(); // B2
        assert_eq!(wb.value(0, 2, 2).unwrap(), Scalar::Number(800.0));
        assert_eq!(wb.input(0, 2, 2).unwrap(), "=SUMIFS(sales[amount], sales[region], B1) * 2");

        // A regular formula depending on the bridge cell.
        wb.set_input(0, 3, 2, "=B2+1").unwrap();
        assert_eq!(wb.value(0, 3, 2).unwrap(), Scalar::Number(801.0));

        // Changing the criterion cell refreshes the chain.
        wb.set_input(0, 1, 2, "부산").unwrap();
        assert_eq!(wb.value(0, 2, 2).unwrap(), Scalar::Number(500.0));
        assert_eq!(wb.value(0, 3, 2).unwrap(), Scalar::Number(501.0));
    }

    #[test]
    fn bridge_criteria_can_come_from_other_formulas() {
        let mut wb = book();
        wb.set_input(0, 1, 1, "=\"서\"&\"울\"").unwrap(); // A1 computed by the calc engine
        wb.set_input(0, 2, 1, "=COUNTIFS(sales[region], A1)").unwrap();
        assert_eq!(wb.value(0, 2, 1).unwrap(), Scalar::Number(2.0));
    }

    #[test]
    fn data_edits_invalidate_results() {
        let mut wb = book();
        wb.set_input(0, 1, 1, "=SUM(sales[amount])").unwrap();
        assert_eq!(wb.value(0, 1, 1).unwrap(), Scalar::Number(650.0));
        wb.update_data_cell("sales", 1, "amount", "1000").unwrap();
        assert_eq!(wb.value(0, 1, 1).unwrap(), Scalar::Number(1550.0));
    }

    #[test]
    fn broken_bridge_call_reports_a_reason() {
        let mut wb = book();
        wb.set_input(0, 1, 1, "=SUM(sales[nope])").unwrap();
        assert!(matches!(wb.value(0, 1, 1).unwrap(), Scalar::Error(_)));
        assert!(wb.detail(0, 1, 1).unwrap().bridge_error.unwrap().contains("nope"));
    }

    #[test]
    fn dquery_spills_into_the_sheet() {
        let mut wb = book();
        wb.set_input(0, 1, 1, "=DQUERY(\"SELECT region, SUM(amount) AS total FROM sales GROUP BY 1 ORDER BY 1\")")
            .unwrap();
        assert_eq!(wb.value(0, 1, 1).unwrap(), Scalar::Text("region".into()));
        assert_eq!(wb.value(0, 2, 1).unwrap(), Scalar::Text("부산".into()));
        assert_eq!(wb.value(0, 3, 2).unwrap(), Scalar::Number(400.0));
    }

    #[test]
    fn undo_and_redo() {
        let mut wb = Workbook::new().unwrap();
        wb.set_input(0, 1, 1, "1").unwrap();
        wb.set_input(0, 1, 1, "2").unwrap();
        wb.undo().unwrap();
        assert_eq!(wb.value(0, 1, 1).unwrap(), Scalar::Number(1.0));
        wb.redo().unwrap();
        assert_eq!(wb.value(0, 1, 1).unwrap(), Scalar::Number(2.0));
    }

    #[test]
    fn xlsx_round_trip_keeps_bridge_results() {
        let mut wb = book();
        wb.set_input(0, 1, 1, "=SUM(sales[amount])").unwrap();
        wb.set_input(0, 2, 1, "=A1/2").unwrap();
        let dir = std::env::temp_dir().join(format!("cellx-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.xlsx");
        wb.save_xlsx(&path).unwrap();
        wb.save_xlsx(&path).unwrap(); // overwriting works
        let reopened = Workbook::open_xlsx(&path).unwrap();
        assert_eq!(reopened.value(0, 1, 1).unwrap(), Scalar::Number(650.0));
        assert_eq!(reopened.value(0, 2, 1).unwrap(), Scalar::Number(325.0));
        std::fs::remove_dir_all(&dir).ok();
    }
}
