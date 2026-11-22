//! A CellX workbook: the Excel-compatible calculation engine (IronCalc's UserModel,
//! which brings Excel's editing operations) plus the data engine (DuckDB), joined by
//! the formula → SQL bridge.
//!
//! Undo: every user action is one [`Action`]. It records how many steps the action
//! pushed onto IronCalc's own history and the bridge formulas before and after, so
//! undoing restores both. Bridge *results* are written straight into the model
//! (outside IronCalc's history): they are recomputed, never undone.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Instant;

use ironcalc_base::expressions::types::Area;
use ironcalc_base::types::{BorderItem, Color, Comment, HorizontalAlignment, VerticalAlignment};
use ironcalc_base::worksheet::NavigationDirection;
use ironcalc_base::{Model, UserModel};
use serde::{Deserialize, Serialize};

use crate::bridge::{self, BridgeValue, CellRef, CellResolver};
use crate::data::{ColumnFilter, DataStore, Page, Sort, TableInfo};
use crate::error::{CellxError, Result};
use crate::datatools::{AutoFilter, Validation};
use crate::sheetopts::{Guard, PageSetup, SheetProtection};
use crate::extras::{Chart, PivotDef};
use crate::refs::{self, Axis};
use crate::value::{from_ironcalc, Scalar};

pub const LOCALE: &str = "en";
pub const LANGUAGE: &str = "en";
pub const TIMEZONE: &str = "Asia/Seoul";
pub const DEFAULT_FONT: &str = "맑은 고딕";
pub const DEFAULT_FONT_SIZE: i32 = 11;

/// Passes of "evaluate → refresh bridge calls" before giving up on convergence.
const MAX_BRIDGE_PASSES: usize = 8;

type Key = (u32, i32, i32);
type BridgeMap = BTreeMap<Key, BridgeCell>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BridgeCell {
    /// What the user typed, e.g. `=SUMIFS(sales[amount], sales[region], B1)`.
    pub formula: String,
    /// What the calculation engine currently holds, e.g. `=1234`.
    #[serde(skip)]
    pub rewritten: Option<String>,
    #[serde(skip)]
    pub error: Option<String>,
}

impl BridgeCell {
    fn new(formula: String) -> Self {
        BridgeCell { formula, rewritten: None, error: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellRange {
    pub r0: i32,
    pub c0: i32,
    pub r1: i32,
    pub c1: i32,
}

impl CellRange {
    pub fn normalized(self) -> Self {
        CellRange {
            r0: self.r0.min(self.r1),
            c0: self.c0.min(self.c1),
            r1: self.r0.max(self.r1),
            c1: self.c0.max(self.c1),
        }
    }
    pub(crate) fn area(self, sheet: u32) -> Area {
        let r = self.normalized();
        Area { sheet, row: r.r0, column: r.c0, width: r.c1 - r.c0 + 1, height: r.r1 - r.r0 + 1 }
    }
    pub(crate) fn contains(self, row: i32, col: i32) -> bool {
        let r = self.normalized();
        row >= r.r0 && row <= r.r1 && col >= r.c0 && col <= r.c1
    }
    pub(crate) fn intersects(&self, other: &CellRange) -> bool {
        let (a, b) = (self.normalized(), other.normalized());
        a.r0 <= b.r1 && b.r0 <= a.r1 && a.c0 <= b.c1 && b.c0 <= a.c1
    }
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct BorderView {
    pub style: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct CellStyle {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub strike: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valign: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub wrap: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// Font size in points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_top: Option<BorderView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_right: Option<BorderView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_bottom: Option<BorderView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_left: Option<BorderView>,
    /// Conditional formatting data bar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bar: Option<BarView>,
    /// Conditional formatting icon (or rating).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<IconView>,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct BarView {
    /// Filled proportion of the cell width, 0..1.
    pub value: f64,
    /// Where zero sits, 0..1.
    pub axis: f64,
    pub color: String,
    pub negative: String,
    pub gradient: bool,
    pub show_value: bool,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct IconView {
    pub name: String,
    pub color: String,
    pub count: u32,
    pub max: u32,
    pub show_value: bool,
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
    /// The cell holds a hyperlink.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub link: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<CellStyle>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Viewport {
    pub cells: Vec<CellView>,
}

/// Everything the grid needs to lay a sheet out.
#[derive(Debug, Clone, Serialize)]
pub struct SheetLayout {
    pub default_row_height: f64,
    pub default_col_width: f64,
    /// (row, height px) for rows whose height differs from the default.
    pub rows: Vec<(i32, f64)>,
    pub hidden_rows: Vec<i32>,
    /// (first column, last column, width px).
    pub cols: Vec<(i32, i32, f64)>,
    pub hidden_cols: Vec<i32>,
    /// (row, col, height, width) of merged ranges.
    pub merges: Vec<(i32, i32, i32, i32)>,
    pub frozen_rows: i32,
    pub frozen_cols: i32,
    pub show_grid_lines: bool,
    pub last_row: i32,
    pub last_col: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CellDetail {
    pub input: String,
    pub text: String,
    pub bridge_error: Option<String>,
    pub style: CurrentStyle,
}

/// The style of the active cell, for the ribbon's toggle buttons and boxes.
#[derive(Debug, Clone, Serialize)]
pub struct CurrentStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub font: String,
    pub size: f64,
    pub align: String,
    pub valign: String,
    pub wrap: bool,
    pub num_fmt: String,
    pub merged: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RecalcStats {
    pub passes: usize,
    pub bridge_calls: usize,
    pub bridge_cache_hits: usize,
    pub calc_ms: f64,
    pub bridge_ms: f64,
    /// Formulas evaluated by an incremental recalculation; `None` for a full one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluated: Option<usize>,
}

/// What Excel's status bar shows for a selection.
#[derive(Debug, Clone, Serialize, Default)]
pub struct SelectionStats {
    pub count: u64,
    pub numeric_count: u64,
    pub sum: f64,
    pub average: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// State kept outside IronCalc's history, saved before and after each action.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Side {
    bridges: BridgeMap,
    charts: Vec<Chart>,
    pictures: Vec<crate::pictures::Picture>,
    shapes: Vec<crate::shapes::Shape>,
    sparklines: Vec<crate::sparklines::SparklineGroup>,
    slicers: Vec<crate::slicers::Slicer>,
    data_tables: Vec<crate::whatif::DataTable>,
    scenarios: Vec<crate::whatif::Scenario>,
    book_protection: Option<crate::book::BookProtection>,
    custom_views: Vec<crate::book::CustomView>,
    pivots: Vec<PivotDef>,
    comments: Vec<Vec<Comment>>,
    filters: Vec<AutoFilter>,
    validations: Vec<Validation>,
    outline_rows: BTreeMap<(u32, i32), u8>,
    outline_cols: BTreeMap<(u32, i32), u8>,
    protections: Vec<SheetProtection>,
    unlocked: Vec<(u32, CellRange)>,
    page_setups: Vec<PageSetup>,
    tables: Vec<(String, ironcalc_base::types::Table)>,
}

impl Side {
    /// Whether going from `self` to `other` can change what formulas compute:
    /// data engine formulas and tables (structured references). Filters,
    /// outline, comments, charts and the like cannot.
    fn affects_values(&self, other: &Side) -> bool {
        self.bridges != other.bridges || self.tables != other.tables || self.data_tables != other.data_tables
    }
}

#[derive(Debug, Clone)]
struct Action {
    steps: usize,
    before: Side,
    after: Side,
}

pub(crate) struct StoredClipboard {
    pub(crate) clip: ironcalc_base::Clipboard,
    /// Bridge formulas in the copied range, by offset from its top-left cell.
    pub(crate) bridges: Vec<(i32, i32, String)>,
    pub(crate) cut: bool,
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
    pub(crate) um: UserModel<'static>,
    data: DataStore,
    data_path: Option<PathBuf>,
    pub(crate) bridges: BridgeMap,
    cache: HashMap<String, BridgeValue>,
    undo: Vec<Action>,
    redo: Vec<Action>,
    pub(crate) clipboard: Option<StoredClipboard>,
    pub(crate) charts: Vec<Chart>,
    pub(crate) pictures: Vec<crate::pictures::Picture>,
    pub(crate) shapes: Vec<crate::shapes::Shape>,
    pub(crate) sparklines: Vec<crate::sparklines::SparklineGroup>,
    pub(crate) slicers: Vec<crate::slicers::Slicer>,
    pub(crate) data_tables: Vec<crate::whatif::DataTable>,
    pub(crate) scenarios: Vec<crate::whatif::Scenario>,
    pub(crate) book_protection: Option<crate::book::BookProtection>,
    pub(crate) custom_views: Vec<crate::book::CustomView>,
    pub(crate) pivots: Vec<PivotDef>,
    pub(crate) filters: Vec<AutoFilter>,
    pub(crate) validations: Vec<Validation>,
    pub(crate) outline_rows: BTreeMap<(u32, i32), u8>,
    pub(crate) outline_cols: BTreeMap<(u32, i32), u8>,
    pub(crate) protections: Vec<SheetProtection>,
    pub(crate) unlocked: Vec<(u32, CellRange)>,
    pub(crate) page_setups: Vec<PageSetup>,
    pub(crate) next_object_id: u32,
    pub last_recalc: RecalcStats,
    /// The model was changed outside its history (`model_mut`): the next
    /// recalculation has to be a full one.
    untracked: bool,
}

fn style_key(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

impl Workbook {
    /// A new, empty workbook with one sheet and an in-memory data engine.
    pub fn new() -> Result<Self> {
        let um = UserModel::new_empty("Book1", LOCALE, TIMEZONE, LANGUAGE)?;
        Self::from_user_model(um, DataStore::open_in_memory()?, None)
    }

    /// Opens an xlsx file. Cached values from Excel are kept until the first recalculation.
    pub fn open_xlsx(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Book1".into());
        let model = model_from_xlsx_bytes(&bytes, &name)?;
        let mut wb = Self::from_model(model, DataStore::open_in_memory()?, None)?;
        wb.import_extras(&bytes);
        Ok(wb)
    }

    pub(crate) fn from_model(model: Model<'static>, data: DataStore, data_path: Option<PathBuf>) -> Result<Self> {
        Self::from_user_model(UserModel::from_model(model), data, data_path)
    }

    fn from_user_model(mut um: UserModel<'static>, data: DataStore, data_path: Option<PathBuf>) -> Result<Self> {
        // CellX decides when to evaluate (it has to refresh bridge results in between).
        um.pause_evaluation();
        // No collaboration: the change queue would only grow and cost a copy of every change.
        um.set_send_queue_enabled(false);
        Ok(Workbook {
            um,
            data,
            data_path,
            bridges: BTreeMap::new(),
            cache: HashMap::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            clipboard: None,
            charts: Vec::new(),
            pictures: Vec::new(),
            shapes: Vec::new(),
            sparklines: Vec::new(),
            slicers: Vec::new(),
            data_tables: Vec::new(),
            scenarios: Vec::new(),
            book_protection: None,
            custom_views: Vec::new(),
            pivots: Vec::new(),
            filters: Vec::new(),
            validations: Vec::new(),
            outline_rows: BTreeMap::new(),
            outline_cols: BTreeMap::new(),
            protections: Vec::new(),
            unlocked: Vec::new(),
            page_setups: Vec::new(),
            next_object_id: 1,
            last_recalc: RecalcStats::default(),
            untracked: true,
        })
    }

    pub(crate) fn set_bridges(&mut self, bridges: BridgeMap) {
        self.bridges = bridges;
    }

    pub(crate) fn bridges(&self) -> &BridgeMap {
        &self.bridges
    }

    pub fn model(&self) -> &Model<'_> {
        self.um.get_model()
    }

    pub(crate) fn model_mut(&mut self) -> &mut Model<'static> {
        self.untracked = true;
        self.um.get_model_mut()
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
        drop(DataStore::open(path)?);
        let p = path.to_string_lossy().replace('\'', "''");
        let mut sql = format!("ATTACH '{p}' AS _dest;");
        for t in self.data.table_names()? {
            let q = crate::data::quote_ident(&t);
            sql.push_str(&format!("CREATE OR REPLACE TABLE _dest.{q} AS SELECT * FROM {q};"));
        }
        sql.push_str("DETACH _dest;");
        self.data.execute_internal(&sql)?;
        self.data = DataStore::open(path)?;
        self.data_path = Some(path.to_path_buf());
        self.cache.clear();
        Ok(())
    }

    // ------------------------------------------------------------ actions (undo)

    fn side(&self) -> Side {
        Side {
            bridges: self.bridges.clone(),
            charts: self.charts.clone(),
            pictures: self.pictures.clone(),
            shapes: self.shapes.clone(),
            sparklines: self.sparklines.clone(),
            slicers: self.slicers.clone(),
            data_tables: self.data_tables.clone(),
            scenarios: self.scenarios.clone(),
            book_protection: self.book_protection.clone(),
            custom_views: self.custom_views.clone(),
            pivots: self.pivots.clone(),
            comments: self.model().workbook.worksheets.iter().map(|w| w.comments.clone()).collect(),
            filters: self.filters.clone(),
            validations: self.validations.clone(),
            outline_rows: self.outline_rows.clone(),
            outline_cols: self.outline_cols.clone(),
            protections: self.protections.clone(),
            unlocked: self.unlocked.clone(),
            page_setups: self.page_setups.clone(),
            tables: {
                let mut t: Vec<(String, ironcalc_base::types::Table)> =
                    self.model().workbook.tables.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                t.sort_by(|a, b| a.0.cmp(&b.0));
                t
            },
        }
    }

    fn restore_side(&mut self, side: &Side) {
        self.bridges = side.bridges.clone();
        self.charts = side.charts.clone();
        self.pictures = side.pictures.clone();
        self.shapes = side.shapes.clone();
        self.sparklines = side.sparklines.clone();
        self.slicers = side.slicers.clone();
        // Data tables that go take their (untracked) results with them.
        let gone: Vec<CellRange> = self.data_tables.iter().filter(|t| !side.data_tables.iter().any(|x| x.id == t.id)).map(|t| t.results()).collect();
        for (t, r) in self.data_tables.iter().filter(|t| !side.data_tables.iter().any(|x| x.id == t.id)).map(|t| t.sheet).zip(gone) {
            let _ = self.um.get_model_mut().range_clear_contents(&r.area(t));
        }
        self.data_tables = side.data_tables.clone();
        self.scenarios = side.scenarios.clone();
        self.book_protection = side.book_protection.clone();
        self.custom_views = side.custom_views.clone();
        self.pivots = side.pivots.clone();
        self.filters = side.filters.clone();
        self.validations = side.validations.clone();
        self.outline_rows = side.outline_rows.clone();
        self.outline_cols = side.outline_cols.clone();
        self.protections = side.protections.clone();
        self.unlocked = side.unlocked.clone();
        self.page_setups = side.page_setups.clone();
        let tables: HashMap<String, ironcalc_base::types::Table> = side.tables.iter().cloned().collect();
        if self.model().workbook.tables != tables {
            let old = std::mem::replace(&mut self.model_mut().workbook.tables, tables);
            // A table that comes back under another name (undoing a rename):
            // its formulas follow it back.
            let first = |t: &ironcalc_base::types::Table| (t.sheet_name.clone(), t.reference.split(':').next().unwrap_or_default().to_string());
            let now = &self.model().workbook.tables;
            let renames: Vec<(String, String)> = now
                .values()
                .filter(|t| !old.contains_key(&t.name))
                .filter_map(|t| old.values().find(|o| !now.contains_key(&o.name) && first(o) == first(t)).map(|o| (o.name.clone(), t.name.clone())))
                .collect();
            for (from, to) in &renames {
                self.model_mut().restructure(Some((from, to)), None);
            }
            self.model_mut().restructure(None, None);
        }
        // Comments do not change values: no need to mark the model as changed untracked.
        for (ws, c) in self.um.get_model_mut().workbook.worksheets.iter_mut().zip(side.comments.iter()) {
            ws.comments = c.clone();
        }
    }

    pub(crate) fn begin(&self) -> (usize, Side) {
        (self.um.undo_depth(), self.side())
    }

    pub(crate) fn commit(&mut self, start: (usize, Side)) -> Result<RecalcStats> {
        let steps = self.um.undo_depth().saturating_sub(start.0);
        let after = self.side();
        let changes = if start.1.affects_values(&after) { None } else { self.um.content_changes_since(start.0) };
        if steps > 0 || start.1 != after {
            self.undo.push(Action { steps, before: start.1, after });
            self.redo.clear();
        }
        self.recalc_changes(changes)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> Result<RecalcStats> {
        let Some(a) = self.undo.pop() else { return Ok(RecalcStats::default()) };
        let changes = if a.before.affects_values(&a.after) { None } else { self.um.content_changes_on_top(a.steps, false) };
        for _ in 0..a.steps {
            self.um.undo()?;
        }
        self.restore_side(&a.before);
        self.redo.push(a);
        self.mark_bridges_dirty();
        self.recalc_changes(changes)
    }

    pub fn redo(&mut self) -> Result<RecalcStats> {
        let Some(a) = self.redo.pop() else { return Ok(RecalcStats::default()) };
        let changes = if a.before.affects_values(&a.after) { None } else { self.um.content_changes_on_top(a.steps, true) };
        for _ in 0..a.steps {
            self.um.redo()?;
        }
        self.restore_side(&a.after);
        self.undo.push(a);
        self.mark_bridges_dirty();
        self.recalc_changes(changes)
    }

    fn mark_bridges_dirty(&mut self) {
        for b in self.bridges.values_mut() {
            b.rewritten = None;
        }
    }

    // ------------------------------------------------------------ sheets

    pub fn sheet_names(&self) -> Vec<String> {
        self.model().workbook.get_worksheet_names()
    }

    pub fn add_sheet(&mut self) -> Result<u32> {
        self.book_guard()?;
        let start = self.begin();
        self.um.new_sheet()?;
        let index = self.sheet_names().len() as u32 - 1;
        self.commit(start)?;
        Ok(index)
    }

    pub fn rename_sheet(&mut self, sheet: u32, name: &str) -> Result<()> {
        self.book_guard()?;
        let old = self.sheet_names().get(sheet as usize).cloned().unwrap_or_default();
        let start = self.begin();
        self.um.rename_sheet(sheet, name)?;
        // Bridge formulas that name the sheet follow the new name, like Excel formulas do.
        if !old.is_empty() && old != name {
            let quoted_old = quote_sheet(&old);
            let quoted_new = quote_sheet(name);
            for b in self.bridges.values_mut() {
                if b.formula.contains(&format!("{quoted_old}!")) {
                    b.formula = b.formula.replace(&format!("{quoted_old}!"), &format!("{quoted_new}!"));
                    b.rewritten = None;
                }
            }
            self.rename_in_objects(&format!("{quoted_old}!"), &format!("{quoted_new}!"));
            for t in self.model_mut().workbook.tables.values_mut() {
                if t.sheet_name == old {
                    t.sheet_name = name.to_string();
                }
            }
            self.model_mut().refresh_parser_tables();
        }
        self.commit(start)?;
        Ok(())
    }

    pub fn delete_sheet(&mut self, sheet: u32) -> Result<()> {
        self.book_guard()?;
        if self.sheet_names().len() <= 1 {
            return Err(CellxError::Calc("통합 문서에는 시트가 하나 이상 있어야 합니다".into()));
        }
        let start = self.begin();
        let gone = self.sheet_names().get(sheet as usize).cloned().unwrap_or_default();
        self.model_mut().workbook.tables.retain(|_, t| t.sheet_name != gone);
        self.model_mut().refresh_parser_tables();
        self.um.delete_sheet(sheet)?;
        let old = std::mem::take(&mut self.bridges);
        for ((s, r, c), b) in old {
            if s < sheet {
                self.bridges.insert((s, r, c), b);
            } else if s > sheet {
                self.bridges.insert((s - 1, r, c), b);
            }
        }
        self.objects_after_sheet_delete(sheet);
        self.outline_after_sheet_delete(sheet);
        self.sheet_options_after_delete(sheet);
        self.commit(start)?;
        Ok(())
    }

    // ------------------------------------------------------------ cells

    /// What the formula bar shows for a cell.
    pub fn input(&self, sheet: u32, row: i32, col: i32) -> Result<String> {
        if let Some(b) = self.bridges.get(&(sheet, row, col)) {
            return Ok(b.formula.clone());
        }
        Ok(self.um.get_cell_content(sheet, row, col)?)
    }

    pub fn detail(&self, sheet: u32, row: i32, col: i32) -> Result<CellDetail> {
        let s = self.um.get_cell_style(sheet, row, col)?;
        let a = s.alignment.clone().unwrap_or_default();
        let merged = self
            .model()
            .get_merged_cells(sheet)
            .map(|m| m.iter().any(|m| row >= m.row && row < m.row + m.height && col >= m.column && col < m.column + m.width))
            .unwrap_or(false);
        Ok(CellDetail {
            input: self.input(sheet, row, col)?,
            text: self.model().get_formatted_cell_value(sheet, row, col)?,
            bridge_error: self.bridges.get(&(sheet, row, col)).and_then(|b| b.error.clone()),
            style: CurrentStyle {
                bold: s.font.b,
                italic: s.font.i,
                underline: s.font.u,
                strike: s.font.strike,
                font: s.font.name.clone(),
                size: s.font.sz as f64,
                align: format!("{:?}", a.horizontal).to_ascii_lowercase(),
                valign: format!("{:?}", a.vertical).to_ascii_lowercase(),
                wrap: a.wrap_text,
                num_fmt: s.num_fmt.clone(),
                merged,
            },
        })
    }

    pub fn value(&self, sheet: u32, row: i32, col: i32) -> Result<Scalar> {
        Ok(from_ironcalc(self.model().get_cell_value_by_index(sheet, row, col)?))
    }

    fn is_bridge_input(&self, input: &str) -> bool {
        input.starts_with('=') && !bridge::find_calls(input, &|n| self.data.has_table(n)).is_empty()
    }

    /// Writes one cell. `record` = through IronCalc's history (user edits) or not.
    pub(crate) fn write_input(&mut self, sheet: u32, row: i32, col: i32, input: &str, record: bool) -> Result<()> {
        self.guard(sheet, CellRange { r0: row, c0: col, r1: row, c1: col }, Guard::Content)?;
        if self.data_table_at(sheet, row, col).is_some() {
            return Err(CellxError::Calc("데이터 표의 일부를 변경할 수 없습니다.".into()));
        }
        let is_bridge = self.is_bridge_input(input);
        let text = if is_bridge {
            self.bridges.insert((sheet, row, col), BridgeCell::new(input.to_string()));
            // Placeholder until the bridge pass fills in the value.
            "=0".to_string()
        } else {
            self.bridges.remove(&(sheet, row, col));
            input.to_string()
        };
        if record {
            self.um.set_user_input(sheet, row, col, &text)?;
        } else {
            self.model_mut().set_user_input(sheet, row, col, text)?;
        }
        Ok(())
    }

    /// Sets a cell from user input (a value or a formula) and recalculates.
    pub fn set_input(&mut self, sheet: u32, row: i32, col: i32, input: &str) -> Result<RecalcStats> {
        if self.input(sheet, row, col)? == input {
            return Ok(RecalcStats::default());
        }
        let start = self.begin();
        self.grow_table_for(sheet, row, col, input);
        self.write_input(sheet, row, col, input, true)?;
        self.fill_calculated_column(sheet, row, col, input);
        self.commit(start)
    }

    /// Sets many cells as one action (typed paste, imports).
    pub fn set_inputs(&mut self, sheet: u32, cells: &[(i32, i32, String)]) -> Result<RecalcStats> {
        let start = self.begin();
        for (row, col, input) in cells {
            self.grow_table_for(sheet, *row, *col, input);
            self.write_input(sheet, *row, *col, input, true)?;
        }
        self.commit(start)
    }

    /// Ctrl+Enter: puts the same entry in every selected cell, moving relative references.
    pub fn fill_range_with(&mut self, sheet: u32, range: CellRange, input: &str) -> Result<RecalcStats> {
        let r = range.normalized();
        let start = self.begin();
        for row in r.r0..=r.r1 {
            for col in r.c0..=r.c1 {
                let text = if input.starts_with('=') {
                    refs::offset_refs(input, row - r.r0, col - r.c0)
                } else {
                    input.to_string()
                };
                self.write_input(sheet, row, col, &text, true)?;
            }
        }
        self.commit(start)
    }

    /// Delete key: clears contents, keeps formatting.
    pub fn clear_contents(&mut self, sheet: u32, range: CellRange) -> Result<RecalcStats> {
        self.guard(sheet, range, Guard::Content)?;
        // A data table goes only as a whole.
        let r = range.normalized();
        for t in self.data_tables.iter().filter(|t| t.sheet == sheet && t.results().intersects(&r)) {
            let res = t.results();
            if !(r.contains(res.r0, res.c0) && r.contains(res.r1, res.c1)) {
                return Err(CellxError::Calc("데이터 표의 일부를 변경할 수 없습니다.".into()));
            }
        }
        let start = self.begin();
        self.data_tables.retain(|t| !(t.sheet == sheet && r.contains(t.results().r0, t.results().c0) && r.contains(t.results().r1, t.results().c1)));
        self.um.range_clear_contents(&range.area(sheet))?;
        self.bridges.retain(|(s, r, c), _| !(*s == sheet && range.contains(*r, *c)));
        self.commit(start)
    }

    pub fn clear_formatting(&mut self, sheet: u32, range: CellRange) -> Result<RecalcStats> {
        self.guard(sheet, range, Guard::FormatCells)?;
        let start = self.begin();
        self.um.range_clear_formatting(&range.area(sheet))?;
        self.commit(start)
    }

    pub fn clear_all(&mut self, sheet: u32, range: CellRange) -> Result<RecalcStats> {
        self.guard(sheet, range, Guard::Content)?;
        self.guard(sheet, range, Guard::FormatCells)?;
        let start = self.begin();
        self.um.range_clear_all(&range.area(sheet))?;
        self.bridges.retain(|(s, r, c), _| !(*s == sheet && range.contains(*r, *c)));
        self.commit(start)
    }

    // ------------------------------------------------------------ clipboard

    pub(crate) fn select(&mut self, sheet: u32, range: CellRange) -> Result<()> {
        let r = range.normalized();
        self.um.set_selected_sheet(sheet)?;
        self.um.set_selected_cell(r.r0, r.c0)?;
        self.um.set_selected_range(r.r0, r.c0, r.r1, r.c1)?;
        Ok(())
    }

    /// Ctrl+C / Ctrl+X. Returns tab-separated text for the system clipboard.
    pub fn copy(&mut self, sheet: u32, range: CellRange, cut: bool) -> Result<String> {
        let r = range.normalized();
        self.select(sheet, r)?;
        let clip = self.um.copy_to_clipboard()?;
        let bridges = self
            .bridges
            .iter()
            .filter(|((s, row, col), _)| *s == sheet && r.contains(*row, *col))
            .map(|((_, row, col), b)| (row - r.r0, col - r.c0, b.formula.clone()))
            .collect();
        let text = clip.csv().to_string();
        self.clipboard = Some(StoredClipboard { clip, bridges, cut });
        Ok(text)
    }

    pub fn clear_clipboard(&mut self) {
        self.clipboard = None;
    }

    /// Ctrl+V. If `text` is what CellX itself copied, the cells are pasted with their
    /// formulas, formats and references moved (Excel's paste). Otherwise the text is
    /// entered as values, tab-separated like Excel's clipboard format.
    pub fn paste(&mut self, sheet: u32, at_row: i32, at_col: i32, text: &str) -> Result<RecalcStats> {
        self.paste_with(sheet, at_row, at_col, text, false)
    }

    /// Like [`Self::paste`]; `from_app` = the UI knows the text came from CellX's own copy
    /// (its rendering of the clipboard text may differ from the engine's).
    pub fn paste_with(&mut self, sheet: u32, at_row: i32, at_col: i32, text: &str, from_app: bool) -> Result<RecalcStats> {
        let normalize = |s: &str| s.replace("\r\n", "\n").trim_end_matches('\n').to_string();
        let internal = self
            .clipboard
            .as_ref()
            .is_some_and(|c| from_app || normalize(c.clip.csv()) == normalize(text));
        let start = self.begin();
        if internal {
            let (sr0, sc0, sr1, sc1) = self.clipboard.as_ref().expect("checked above").clip.range();
            let (h, w) = (sr1 - sr0, sc1 - sc0);
            let target = CellRange { r0: at_row, c0: at_col, r1: at_row + h, c1: at_col + w };
            self.guard(sheet, target, Guard::Content)?;
            let stored = self.clipboard.take().expect("checked above");
            self.select(sheet, target)?;
            self.um
                .paste_from_clipboard(stored.clip.sheet(), stored.clip.range(), stored.clip.data(), stored.cut)?;
            if stored.cut {
                let src_sheet = stored.clip.sheet();
                let src = CellRange { r0: sr0, c0: sc0, r1: sr1, c1: sc1 };
                self.bridges.retain(|(s, r, c), _| !(*s == src_sheet && src.contains(*r, *c)));
            }
            self.bridges.retain(|(s, r, c), _| !(*s == sheet && target.contains(*r, *c)));
            for (dr, dc, formula) in &stored.bridges {
                let moved = if stored.cut {
                    formula.clone()
                } else {
                    refs::offset_refs(formula, at_row - sr0, at_col - sc0)
                };
                self.bridges.insert((sheet, at_row + dr, at_col + dc), BridgeCell::new(moved));
            }
            if !stored.cut {
                self.clipboard = Some(stored); // a copy can be pasted again; a cut only once
            }
        } else {
            let mut cells = Vec::new();
            for (i, line) in text.replace("\r\n", "\n").trim_end_matches('\n').split('\n').enumerate() {
                for (j, v) in line.split('\t').enumerate() {
                    cells.push((at_row + i as i32, at_col + j as i32, v.to_string()));
                }
            }
            for (row, col, v) in cells {
                self.write_input(sheet, row, col, &v, true)?;
            }
        }
        self.commit(start)
    }

    /// The fill handle: extends `source` to `target` (which contains it), like dragging
    /// the small square at the corner of a selection. Ctrl+D / Ctrl+R use it too.
    pub fn auto_fill(&mut self, sheet: u32, source: CellRange, target: CellRange) -> Result<RecalcStats> {
        self.guard(sheet, target, Guard::Content)?;
        let s = source.normalized();
        let t = target.normalized();
        let start = self.begin();
        let src_area = s.area(sheet);
        let vertical = t.c0 == s.c0 && t.c1 == s.c1;
        if vertical {
            if t.r1 > s.r1 {
                self.um.auto_fill_rows(&src_area, t.r1)?;
            } else if t.r0 < s.r0 {
                self.um.auto_fill_rows(&src_area, t.r0)?;
            }
        } else if t.c1 > s.c1 {
            self.um.auto_fill_columns(&src_area, t.c1)?;
        } else if t.c0 < s.c0 {
            self.um.auto_fill_columns(&src_area, t.c0)?;
        }
        // Bridge formulas repeat the source pattern with moved references.
        let src: Vec<(i32, i32, String)> = self
            .bridges
            .iter()
            .filter(|((sh, r, c), _)| *sh == sheet && s.contains(*r, *c))
            .map(|((_, r, c), b)| (*r, *c, b.formula.clone()))
            .collect();
        self.bridges
            .retain(|(sh, r, c), _| !(*sh == sheet && t.contains(*r, *c) && !s.contains(*r, *c)));
        let (h, w) = (s.r1 - s.r0 + 1, s.c1 - s.c0 + 1);
        for row in t.r0..=t.r1 {
            for col in t.c0..=t.c1 {
                if s.contains(row, col) {
                    continue;
                }
                let sr = s.r0 + (row - s.r0).rem_euclid(h);
                let sc = s.c0 + (col - s.c0).rem_euclid(w);
                if let Some((_, _, f)) = src.iter().find(|(r, c, _)| *r == sr && *c == sc) {
                    let moved = refs::offset_refs(f, row - sr, col - sc);
                    self.bridges.insert((sheet, row, col), BridgeCell::new(moved));
                }
            }
        }
        self.commit(start)
    }

    // ------------------------------------------------------------ rows and columns

    fn shift_all(&mut self, sheet: u32, axis: Axis, at: i32, count: i32) {
        self.shift_bridges(sheet, axis, at, count);
        self.shift_objects(sheet, axis, at, count);
        self.shift_outline(sheet, axis, at, count);
        self.shift_tables(sheet, axis, at, count);
        self.shift_sheet_options(sheet, axis, at, count);
    }

    /// For helpers that insert rows inside a larger action (subtotals, tables).
    pub(crate) fn shift_all_pub(&mut self, sheet: u32, axis: Axis, at: i32, count: i32) {
        self.shift_all(sheet, axis, at, count);
    }

    fn shift_bridges(&mut self, sheet: u32, axis: Axis, at: i32, count: i32) {
        let names = self.sheet_names();
        let target = names.get(sheet as usize).cloned().unwrap_or_default();
        let old = std::mem::take(&mut self.bridges);
        for ((s, r, c), mut b) in old {
            let own = names.get(s as usize).cloned().unwrap_or_default();
            b.formula = refs::shift_refs(&b.formula, &own, &target, axis, at, count);
            b.rewritten = None;
            let mut key = (s, r, c);
            if s == sheet {
                let v = if axis == Axis::Row { r } else { c };
                let nv = if count > 0 {
                    if v >= at { v + count } else { v }
                } else if v >= at && v < at - count {
                    continue; // the cell was deleted
                } else if v >= at - count {
                    v + count
                } else {
                    v
                };
                key = if axis == Axis::Row { (s, nv, c) } else { (s, r, nv) };
            }
            self.bridges.insert(key, b);
        }
    }

    pub fn insert_rows(&mut self, sheet: u32, row: i32, count: i32) -> Result<RecalcStats> {
        self.guard(sheet, CellRange { r0: row, c0: 1, r1: row, c1: 1 }, Guard::InsertRows)?;
        let start = self.begin();
        self.um.insert_rows(sheet, row, count)?;
        self.shift_all(sheet, Axis::Row, row, count);
        self.commit(start)
    }

    pub fn delete_rows(&mut self, sheet: u32, row: i32, count: i32) -> Result<RecalcStats> {
        self.guard(sheet, CellRange { r0: row, c0: 1, r1: row, c1: 1 }, Guard::DeleteRows)?;
        let start = self.begin();
        self.um.delete_rows(sheet, row, count)?;
        self.shift_all(sheet, Axis::Row, row, -count);
        self.commit(start)
    }

    pub fn insert_columns(&mut self, sheet: u32, col: i32, count: i32) -> Result<RecalcStats> {
        self.guard(sheet, CellRange { r0: 1, c0: col, r1: 1, c1: col }, Guard::InsertColumns)?;
        let start = self.begin();
        self.um.insert_columns(sheet, col, count)?;
        self.shift_all(sheet, Axis::Column, col, count);
        self.commit(start)
    }

    pub fn delete_columns(&mut self, sheet: u32, col: i32, count: i32) -> Result<RecalcStats> {
        self.guard(sheet, CellRange { r0: 1, c0: col, r1: 1, c1: col }, Guard::DeleteColumns)?;
        let start = self.begin();
        self.um.delete_columns(sheet, col, count)?;
        self.shift_all(sheet, Axis::Column, col, -count);
        self.commit(start)
    }

    pub fn set_column_width(&mut self, sheet: u32, c0: i32, c1: i32, px: f64) -> Result<()> {
        self.guard(sheet, CellRange { r0: 1, c0, r1: 1, c1 }, Guard::FormatColumns)?;
        let start = self.begin();
        self.um.set_columns_width(sheet, c0.min(c1), c0.max(c1), px)?;
        self.commit(start)?;
        Ok(())
    }

    pub fn set_row_height(&mut self, sheet: u32, r0: i32, r1: i32, px: f64) -> Result<()> {
        self.guard(sheet, CellRange { r0, c0: 1, r1, c1: 1 }, Guard::FormatRows)?;
        let start = self.begin();
        self.um.set_rows_height(sheet, r0.min(r1), r0.max(r1), px)?;
        self.commit(start)?;
        Ok(())
    }

    pub fn set_hidden(&mut self, sheet: u32, axis: Axis, from: i32, to: i32, hidden: bool) -> Result<()> {
        let g = if axis == Axis::Row { Guard::FormatRows } else { Guard::FormatColumns };
        self.guard(sheet, CellRange { r0: from, c0: from, r1: from, c1: from }, g)?;
        let start = self.begin();
        match axis {
            Axis::Row => self.um.set_rows_hidden(sheet, from.min(to), from.max(to), hidden)?,
            Axis::Column => self.um.set_columns_hidden(sheet, from.min(to), from.max(to), hidden)?,
        }
        self.commit(start)?;
        Ok(())
    }

    pub fn set_show_grid_lines(&mut self, sheet: u32, show: bool) -> Result<()> {
        let start = self.begin();
        self.um.set_show_grid_lines(sheet, show)?;
        self.commit(start)?;
        Ok(())
    }

    pub fn set_frozen(&mut self, sheet: u32, rows: i32, cols: i32) -> Result<()> {
        let start = self.begin();
        self.um.set_frozen_rows_count(sheet, rows)?;
        self.um.set_frozen_columns_count(sheet, cols)?;
        self.commit(start)?;
        Ok(())
    }

    // ------------------------------------------------------------ formatting

    /// Applies one style property (`font.b`, `font.name`, `num_fmt`, `fill.fg_color`,
    /// `alignment.horizontal`, …) to a range.
    pub fn set_style(&mut self, sheet: u32, range: CellRange, path: &str, value: &str) -> Result<RecalcStats> {
        self.guard(sheet, range, Guard::FormatCells)?;
        let start = self.begin();
        self.um.update_range_style(&range.area(sheet), path, value)?;
        self.commit(start)
    }

    /// Sets borders on a range. `kind`: All, Inner, Outer, Top, Right, Bottom, Left,
    /// CenterH, CenterV or None (Excel's border menu). `style`: thin, medium, thick,
    /// double, dotted, …
    pub fn set_border(&mut self, sheet: u32, range: CellRange, kind: &str, style: &str, color: &str) -> Result<RecalcStats> {
        self.guard(sheet, range, Guard::FormatCells)?;
        let spec = serde_json::json!({
            "item": { "style": style, "color": color },
            "type": kind,
        });
        let border: ironcalc_base::BorderArea =
            serde_json::from_value(spec).map_err(|e| CellxError::Calc(format!("테두리 지정이 잘못되었습니다: {e}")))?;
        let start = self.begin();
        self.um.set_area_with_border(&range.area(sheet), &border)?;
        self.commit(start)
    }

    pub fn merge(&mut self, sheet: u32, range: CellRange, center: bool) -> Result<RecalcStats> {
        self.guard(sheet, range, Guard::FormatCells)?;
        let start = self.begin();
        let area = range.area(sheet);
        if center {
            self.um.merge_cells_center(&area)?;
        } else {
            self.um.merge_cells(&area)?;
        }
        self.commit(start)
    }

    pub fn unmerge(&mut self, sheet: u32, range: CellRange) -> Result<RecalcStats> {
        self.guard(sheet, range, Guard::FormatCells)?;
        let start = self.begin();
        self.um.unmerge_cells(&range.area(sheet))?;
        self.commit(start)
    }

    // ------------------------------------------------------------ navigation and search

    /// Ctrl+Arrow: jumps to the edge of the current data region, like Excel.
    pub fn navigate_edge(&mut self, sheet: u32, row: i32, col: i32, dir: Direction) -> Result<(i32, i32)> {
        self.um.set_selected_sheet(sheet)?;
        self.um.set_selected_cell(row, col)?;
        self.um.on_navigate_to_edge_in_direction(match dir {
            Direction::Up => NavigationDirection::Up,
            Direction::Down => NavigationDirection::Down,
            Direction::Left => NavigationDirection::Left,
            Direction::Right => NavigationDirection::Right,
        })?;
        let v = self.um.get_selected_view();
        Ok((v.row, v.column))
    }

    /// The block of non-empty cells around a cell (Ctrl+A, AutoSum).
    pub fn current_region(&self, sheet: u32, row: i32, col: i32) -> Result<CellRange> {
        let m = self.model();
        let filled = |r: i32, c: i32| r >= 1 && c >= 1 && !m.is_empty_cell(sheet, r, c).unwrap_or(true);
        let mut g = CellRange { r0: row, c0: col, r1: row, c1: col };
        loop {
            let mut grown = false;
            if (g.c0..=g.c1).any(|c| filled(g.r0 - 1, c) || filled(g.r0 - 1, c - 1) || filled(g.r0 - 1, c + 1)) {
                g.r0 -= 1;
                grown = true;
            }
            if (g.c0..=g.c1).any(|c| filled(g.r1 + 1, c) || filled(g.r1 + 1, c - 1) || filled(g.r1 + 1, c + 1)) {
                g.r1 += 1;
                grown = true;
            }
            if (g.r0..=g.r1).any(|r| filled(r, g.c0 - 1)) {
                g.c0 -= 1;
                grown = true;
            }
            if (g.r0..=g.r1).any(|r| filled(r, g.c1 + 1)) {
                g.c1 += 1;
                grown = true;
            }
            if !grown || (g.r1 - g.r0) > 1_000_000 {
                break;
            }
        }
        Ok(g)
    }

    /// Alt+=: the formula Excel's AutoSum would put in `(row, col)`.
    pub fn autosum_formula(&self, sheet: u32, row: i32, col: i32) -> Result<String> {
        let m = self.model();
        let is_num = |r: i32, c: i32| {
            matches!(m.get_cell_value_by_index(sheet, r, c), Ok(ironcalc_base::cell::CellValue::Number(_)))
        };
        let mut r = row - 1;
        while r >= 1 && is_num(r, col) {
            r -= 1;
        }
        if r < row - 1 {
            return Ok(format!("=SUM({}{}:{}{})", refs::column_name(col), r + 1, refs::column_name(col), row - 1));
        }
        let mut c = col - 1;
        while c >= 1 && is_num(row, c) {
            c -= 1;
        }
        if c < col - 1 {
            return Ok(format!("=SUM({}{}:{}{})", refs::column_name(c + 1), row, refs::column_name(col - 1), row));
        }
        Ok("=SUM()".to_string())
    }

    /// Ctrl+F: next cell (row-major, wrapping) whose shown text or formula contains `query`.
    pub fn find(&self, sheet: u32, query: &str, after: (i32, i32), match_case: bool) -> Result<Option<(i32, i32)>> {
        if query.is_empty() {
            return Ok(None);
        }
        let needle = if match_case { query.to_string() } else { query.to_lowercase() };
        let ws = self.model().workbook.worksheet(sheet)?;
        let mut cells: Vec<(i32, i32)> = ws.sheet_data.cells().map(|(r, c, _)| (r, c)).collect();
        cells.sort();
        let matches = |r: i32, c: i32| {
            let shown = self.model().get_formatted_cell_value(sheet, r, c).unwrap_or_default();
            let input = self.input(sheet, r, c).unwrap_or_default();
            let (a, b) = if match_case { (shown, input) } else { (shown.to_lowercase(), input.to_lowercase()) };
            a.contains(&needle) || b.contains(&needle)
        };
        let next = cells.iter().filter(|p| **p > after).chain(cells.iter().filter(|p| **p <= after)).find(|(r, c)| matches(*r, *c));
        Ok(next.copied())
    }

    /// Sum, average and count for the status bar.
    pub fn selection_stats(&self, sheet: u32, range: CellRange) -> Result<SelectionStats> {
        let r = range.normalized();
        let ws = self.model().workbook.worksheet(sheet)?;
        let mut st = SelectionStats::default();
        let area = (r.r1 - r.r0 + 1) as i64 * (r.c1 - r.c0 + 1) as i64;
        let positions: Vec<(i32, i32)> = if area > 200_000 {
            ws.sheet_data.cells().map(|(row, col, _)| (row, col)).filter(|(row, col)| r.contains(*row, *col)).collect()
        } else {
            (r.r0..=r.r1).flat_map(|row| (r.c0..=r.c1).map(move |col| (row, col))).collect()
        };
        for (row, col) in positions {
            match self.model().get_cell_value_by_index(sheet, row, col)? {
                ironcalc_base::cell::CellValue::None => {}
                ironcalc_base::cell::CellValue::Number(n) => {
                    st.count += 1;
                    st.numeric_count += 1;
                    st.sum += n;
                    st.min = Some(st.min.map_or(n, |m: f64| m.min(n)));
                    st.max = Some(st.max.map_or(n, |m: f64| m.max(n)));
                }
                ironcalc_base::cell::CellValue::String(s) if s.is_empty() => {}
                _ => st.count += 1,
            }
        }
        if st.numeric_count > 0 {
            st.average = Some(st.sum / st.numeric_count as f64);
        }
        Ok(st)
    }

    pub fn function_names() -> Vec<String> {
        let mut names = ironcalc_base::supported_function_names();
        names.push("DQUERY".into());
        names.sort();
        names
    }

    // ------------------------------------------------------------ recalculation

    /// Evaluates the workbook, then refreshes every bridge call whose inputs changed,
    /// repeating until no bridge result changes.
    pub fn recalc(&mut self) -> Result<RecalcStats> {
        self.recalc_changes(None)
    }

    /// Recalculates after the content of `changes` was set: only the formulas
    /// that depend on them, or everything (`None`, or after an untracked change).
    pub(crate) fn recalc_changes(&mut self, changes: Option<Vec<(u32, i32, i32)>>) -> Result<RecalcStats> {
        let mut stats = RecalcStats::default();
        let mut changes = if self.untracked { None } else { changes };
        self.untracked = false;
        for _ in 0..MAX_BRIDGE_PASSES {
            stats.passes += 1;
            let t = Instant::now();
            let model = self.um.get_model_mut();
            match changes.take() {
                Some(cells) => {
                    model.evaluate_changed(&cells);
                    stats.evaluated = match (model.last_partial_evaluation(), stats.evaluated, stats.passes) {
                        (Some(n), Some(before), p) if p > 1 => Some(before + n),
                        (Some(n), _, 1) => Some(n),
                        _ => None,
                    };
                }
                None => {
                    model.invalidate_dependencies();
                    model.evaluate();
                    stats.evaluated = None;
                }
            }
            stats.calc_ms += t.elapsed().as_secs_f64() * 1000.0;

            let t = Instant::now();
            let mut changed = self.refresh_bridges(&mut stats);
            stats.bridge_ms += t.elapsed().as_secs_f64() * 1000.0;
            // 데이터 표 results, worked out against the model as it now is.
            if !self.data_tables.is_empty() {
                changed.extend(self.compute_data_tables());
            }
            if changed.is_empty() {
                break;
            }
            // Data engine results are written outside the history; the next
            // pass re-evaluates what reads them.
            let mut written = Vec::with_capacity(changed.len());
            for ((sheet, row, col), text) in changed {
                self.um.get_model_mut().set_user_input(sheet, row, col, text)?;
                written.push((sheet, row, col));
            }
            changes = Some(written);
        }
        self.last_recalc = stats.clone();
        Ok(stats)
    }

    fn refresh_bridges(&mut self, stats: &mut RecalcStats) -> Vec<(Key, String)> {
        let mut changed = Vec::new();
        let keys: Vec<Key> = self.bridges.keys().copied().collect();
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
        changed
    }

    /// Replaces every bridge call in `formula` by its current result.
    fn rewrite(&mut self, formula: &str, sheet: u32, stats: &mut RecalcStats) -> (String, Option<String>) {
        let data = &self.data;
        let calls = bridge::find_calls(formula, &|n| data.has_table(n));
        let mut out = String::with_capacity(formula.len());
        let mut last = 0;
        let mut first_error = None;
        for call in calls {
            let result = call.and_then(|call| {
                let resolver = ModelCells { model: self.um.get_model(), sheet };
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

    // ------------------------------------------------------------ grid data

    pub fn layout(&self, sheet: u32) -> Result<SheetLayout> {
        let m = self.model();
        let ws = m.workbook.worksheet(sheet)?;
        let default_row_height = m.get_row_height(sheet, refs::LAST_ROW)?;
        let default_col_width = m.get_column_width(sheet, refs::LAST_COL)?;
        let mut rows = Vec::new();
        let mut hidden_rows = Vec::new();
        for r in &ws.rows {
            if r.hidden {
                hidden_rows.push(r.r);
            }
            let h = m.get_row_height(sheet, r.r)?;
            if (h - default_row_height).abs() > 0.01 {
                rows.push((r.r, h));
            }
        }
        let mut cols = Vec::new();
        let mut hidden_cols = Vec::new();
        for c in &ws.cols {
            let w = m.get_column_width(sheet, c.min)?;
            if c.hidden {
                hidden_cols.extend(c.min..=c.max.min(refs::LAST_COL));
            }
            if (w - default_col_width).abs() > 0.01 {
                cols.push((c.min, c.max.min(refs::LAST_COL), w));
            }
        }
        let merges = m
            .get_merged_cells(sheet)?
            .iter()
            .map(|mc| (mc.row, mc.column, mc.height, mc.width))
            .collect();
        let (last_row, last_col) = self.dimensions(sheet)?;
        Ok(SheetLayout {
            default_row_height,
            default_col_width,
            rows,
            hidden_rows,
            cols,
            hidden_cols,
            merges,
            frozen_rows: self.um.get_frozen_rows_count(sheet)?,
            frozen_cols: self.um.get_frozen_columns_count(sheet)?,
            show_grid_lines: self.um.get_show_grid_lines(sheet)?,
            last_row,
            last_col,
        })
    }

    fn cell_style(&self, sheet: u32, row: i32, col: i32, default_font: &(String, i32)) -> Option<CellStyle> {
        let ext = self.um.get_extended_cell_style(sheet, row, col).ok()?;
        let s = &ext.style;
        let color = |c: &Color| -> Option<String> {
            match c {
                Color::None => None,
                other => {
                    let rgb = self.um.resolve_color(other);
                    let rgb = rgb.trim_start_matches('#');
                    let rgb = if rgb.len() == 8 { &rgb[2..] } else { rgb };
                    (!rgb.is_empty()).then(|| format!("#{rgb}"))
                }
            }
        };
        let border = |b: &Option<BorderItem>| -> Option<BorderView> {
            b.as_ref().map(|i| BorderView {
                style: format!("{}", i.style),
                color: color(&i.color).unwrap_or_else(|| "#000000".into()),
            })
        };
        let mut cs = CellStyle {
            bold: s.font.b,
            italic: s.font.i,
            underline: s.font.u,
            strike: s.font.strike,
            color: color(&s.font.color).filter(|c| !c.eq_ignore_ascii_case("#000000")),
            fill: color(&s.fill.color),
            border_top: border(&s.border.top),
            border_right: border(&s.border.right),
            border_bottom: border(&s.border.bottom),
            border_left: border(&s.border.left),
            ..Default::default()
        };
        if style_key(&s.font.name) != style_key(&default_font.0) {
            cs.font = Some(s.font.name.clone());
        }
        if s.font.sz != default_font.1 {
            cs.size = Some(s.font.sz as f64);
        }
        if let Some(a) = &s.alignment {
            cs.align = match a.horizontal {
                HorizontalAlignment::Left => Some("left".into()),
                HorizontalAlignment::Right => Some("right".into()),
                HorizontalAlignment::Center | HorizontalAlignment::CenterContinuous => Some("center".into()),
                _ => None,
            };
            cs.valign = match a.vertical {
                VerticalAlignment::Top => Some("top".into()),
                VerticalAlignment::Center => Some("center".into()),
                _ => None,
            };
            cs.wrap = a.wrap_text;
        }
        if let Some(bar) = &ext.data_bar {
            cs.bar = Some(BarView {
                value: bar.value,
                axis: bar.axis_position,
                color: color(&bar.positive_color).unwrap_or_else(|| "#638EC6".into()),
                negative: color(&bar.negative_color).unwrap_or_else(|| "#FF0000".into()),
                gradient: bar.is_gradient,
                show_value: bar.show_value,
            });
        }
        if let Some(icon) = &ext.icon {
            cs.icon = Some(IconView {
                name: format!("{:?}", icon.icon),
                color: color(&icon.color).unwrap_or_else(|| "#000000".into()),
                count: 1,
                max: 1,
                show_value: icon.show_value,
            });
        }
        if let Some(r) = &ext.rating {
            cs.icon = Some(IconView {
                name: format!("{:?}", r.icon),
                color: color(&r.color).unwrap_or_else(|| "#000000".into()),
                count: r.count,
                max: r.max,
                show_value: r.show_value,
            });
        }
        (cs != CellStyle::default()).then_some(cs)
    }

    fn default_font(&self) -> (String, i32) {
        self.model()
            .workbook
            .styles
            .fonts
            .first()
            .map(|f| (f.name.clone(), f.sz))
            .unwrap_or((DEFAULT_FONT.into(), DEFAULT_FONT_SIZE))
    }

    pub fn workbook_font(&self) -> (String, f64) {
        let (n, s) = self.default_font();
        (n, s as f64)
    }

    pub fn viewport(&self, sheet: u32, row0: i32, col0: i32, row1: i32, col1: i32) -> Result<Viewport> {
        let ws = self.model().workbook.worksheet(sheet)?;
        let default_font = self.default_font();
        let links = self.um.get_links(sheet)?;
        let tables = self.tables_on(sheet);
        let mut cells = Vec::new();
        let in_table = |row: i32, col: i32| tables.iter().any(|t| t.range.contains(row, col));
        for row in row0..=row1 {
            for col in col0..=col1 {
                let exists = ws.cell(row, col).is_some();
                if !exists && !in_table(row, col) {
                    continue;
                }
                let (text, scalar, mut style) = if exists {
                    (
                        self.model().get_formatted_cell_value(sheet, row, col)?,
                        from_ironcalc(self.model().get_cell_value_by_index(sheet, row, col)?),
                        self.cell_style(sheet, row, col, &default_font),
                    )
                } else {
                    (String::new(), Scalar::Empty, None)
                };
                if in_table(row, col) {
                    let mut cs = style.unwrap_or_default();
                    self.table_style_for(&tables, row, col, &mut cs);
                    style = (cs != CellStyle::default()).then_some(cs);
                }
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
                    link: links.contains_key(&(row, col)),
                    style,
                });
            }
        }
        Ok(Viewport { cells })
    }

    /// Last used row and column of a sheet (Ctrl+End, scroll extents).
    pub fn dimensions(&self, sheet: u32) -> Result<(i32, i32)> {
        let ws = self.model().workbook.worksheet(sheet)?;
        let mut max_row = 0;
        let mut max_col = 0;
        for (row, col, _) in ws.sheet_data.cells() {
            max_row = max_row.max(row);
            max_col = max_col.max(col);
        }
        Ok((max_row, max_col))
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
        let info = self.data.create_table_from_sql(table, sql.trim().trim_end_matches(';'))?;
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
        let cells: Vec<Key> = self.model().get_all_cells().into_iter().map(|c| (c.index, c.row, c.column)).collect();
        for (sheet, row, col) in cells {
            if self.bridges.contains_key(&(sheet, row, col)) {
                continue;
            }
            let content = self.um.get_cell_content(sheet, row, col)?;
            if content.starts_with('=') && content.contains('[') && self.is_bridge_input(&content) {
                self.write_input(sheet, row, col, &content, false)?;
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ files

    /// Saves the workbook as xlsx. Bridge formulas are written with their current
    /// results inlined (`=ROUND(1234.5,0)`), so Excel shows the same values.
    pub fn save_xlsx(&self, path: &Path) -> Result<()> {
        let bytes = self.xlsx_bytes()?;
        let tmp = path.with_extension("cellx-tmp.xlsx");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// The workbook as xlsx bytes, with memos and charts.
    pub fn xlsx_bytes(&self) -> Result<Vec<u8>> {
        let mut xlsx = std::io::Cursor::new(Vec::new());
        ironcalc::export::save_xlsx_to_writer(self.model(), &mut xlsx).map_err(|e| CellxError::Xlsx(e.to_string()))?;
        let book = crate::ooxml::BookParts { protection: self.book_protection_xml(), views: self.custom_workbook_views_xml() };
        crate::ooxml::add_extras_with(xlsx.get_ref(), &self.sheet_parts(), &self.sheet_names(), &book)
    }

    pub fn bridge_formula_count(&self) -> usize {
        self.bridges.len()
    }
}

pub(crate) fn quote_sheet(name: &str) -> String {
    if name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

pub(crate) fn model_from_xlsx_bytes(bytes: &[u8], name: &str) -> Result<Model<'static>> {
    let workbook = ironcalc::import::load_from_xlsx_bytes(bytes, name, LOCALE, TIMEZONE)
        .map_err(|e| CellxError::Xlsx(e.to_string()))?;
    Ok(Model::from_workbook(workbook, LANGUAGE)?)
}

pub fn load_xlsx(path: &Path) -> Result<Model<'static>> {
    let p = path
        .to_str()
        .ok_or_else(|| CellxError::Xlsx(format!("경로를 읽을 수 없습니다: {}", path.display())))?;
    ironcalc::import::load_from_xlsx(p, LOCALE, TIMEZONE, LANGUAGE).map_err(|e| CellxError::Xlsx(e.to_string()))
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

    fn range(r0: i32, c0: i32, r1: i32, c1: i32) -> CellRange {
        CellRange { r0, c0, r1, c1 }
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
    fn new_workbooks_use_korean_excel_defaults() {
        let wb = Workbook::new().unwrap();
        assert_eq!(wb.workbook_font(), (DEFAULT_FONT.to_string(), 11.0));
        let l = wb.layout(0).unwrap();
        assert_eq!((l.default_col_width, l.default_row_height), (72.0, 22.0));
    }

    #[test]
    fn bridge_formula_follows_cell_inputs() {
        let mut wb = book();
        wb.set_input(0, 1, 2, "서울").unwrap();
        wb.set_input(0, 2, 2, "=SUMIFS(sales[amount], sales[region], B1) * 2").unwrap();
        assert_eq!(wb.value(0, 2, 2).unwrap(), Scalar::Number(800.0));
        wb.set_input(0, 3, 2, "=B2+1").unwrap();
        assert_eq!(wb.value(0, 3, 2).unwrap(), Scalar::Number(801.0));
        wb.set_input(0, 1, 2, "부산").unwrap();
        assert_eq!(wb.value(0, 2, 2).unwrap(), Scalar::Number(500.0));
        assert_eq!(wb.value(0, 3, 2).unwrap(), Scalar::Number(501.0));
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
    fn undo_redo_covers_cells_and_bridges() {
        let mut wb = book();
        wb.set_input(0, 1, 1, "1").unwrap();
        wb.set_input(0, 1, 1, "=SUM(sales[amount])").unwrap();
        assert_eq!(wb.value(0, 1, 1).unwrap(), Scalar::Number(650.0));
        wb.undo().unwrap();
        assert_eq!(wb.value(0, 1, 1).unwrap(), Scalar::Number(1.0));
        assert_eq!(wb.input(0, 1, 1).unwrap(), "1");
        wb.redo().unwrap();
        assert_eq!(wb.value(0, 1, 1).unwrap(), Scalar::Number(650.0));
        assert_eq!(wb.input(0, 1, 1).unwrap(), "=SUM(sales[amount])");
    }

    #[test]
    fn copy_paste_moves_references_in_both_engines() {
        let mut wb = book();
        wb.set_inputs(0, &[(1, 1, "서울".into()), (2, 1, "부산".into()), (1, 3, "10".into()), (2, 3, "20".into())])
            .unwrap();
        wb.set_input(0, 1, 2, "=SUMIFS(sales[amount], sales[region], A1)").unwrap(); // B1
        wb.set_input(0, 1, 4, "=C1*2").unwrap(); // D1
        let text = wb.copy(0, range(1, 2, 1, 4), false).unwrap();
        wb.paste(0, 2, 2, &text).unwrap(); // to B2:D2
        assert_eq!(wb.input(0, 2, 2).unwrap(), "=SUMIFS(sales[amount], sales[region], A2)");
        assert_eq!(wb.value(0, 2, 2).unwrap(), Scalar::Number(250.0));
        assert_eq!(wb.input(0, 2, 4).unwrap(), "=C2*2");
        // C1 (10) was pasted over C2 as part of the copied range, as in Excel.
        assert_eq!(wb.value(0, 2, 4).unwrap(), Scalar::Number(20.0));
        // One undo removes the whole paste.
        wb.undo().unwrap();
        assert_eq!(wb.input(0, 2, 2).unwrap(), "");
    }

    #[test]
    fn pasting_external_text_enters_values() {
        let mut wb = Workbook::new().unwrap();
        wb.paste(0, 1, 1, "1\t2\r\n3\t=A1+B1\r\n").unwrap();
        assert_eq!(wb.value(0, 2, 2).unwrap(), Scalar::Number(3.0));
    }

    #[test]
    fn fill_handle_repeats_bridge_formulas() {
        let mut wb = book();
        wb.set_inputs(0, &[(1, 1, "서울".into()), (2, 1, "부산".into()), (3, 1, "서울".into())]).unwrap();
        wb.set_input(0, 1, 2, "=COUNTIFS(sales[region], A1)").unwrap();
        wb.auto_fill(0, range(1, 2, 1, 2), range(1, 2, 3, 2)).unwrap();
        assert_eq!(wb.input(0, 3, 2).unwrap(), "=COUNTIFS(sales[region], A3)");
        assert_eq!(wb.value(0, 2, 2).unwrap(), Scalar::Number(1.0));
        assert_eq!(wb.value(0, 3, 2).unwrap(), Scalar::Number(2.0));
    }

    #[test]
    fn inserting_rows_moves_bridge_cells_and_their_references() {
        let mut wb = book();
        wb.set_input(0, 1, 1, "서울").unwrap();
        wb.set_input(0, 3, 2, "=SUMIFS(sales[amount], sales[region], A1)").unwrap();
        wb.insert_rows(0, 1, 2).unwrap();
        assert_eq!(wb.input(0, 5, 2).unwrap(), "=SUMIFS(sales[amount], sales[region], A3)");
        assert_eq!(wb.value(0, 5, 2).unwrap(), Scalar::Number(400.0));
        wb.undo().unwrap();
        assert_eq!(wb.input(0, 3, 2).unwrap(), "=SUMIFS(sales[amount], sales[region], A1)");
        wb.delete_rows(0, 1, 1).unwrap();
        assert!(matches!(wb.value(0, 2, 2).unwrap(), Scalar::Error(_) | Scalar::Number(_)));
        assert!(wb.input(0, 2, 2).unwrap().contains("#REF!"));
    }

    #[test]
    fn styles_and_status_bar() {
        let mut wb = Workbook::new().unwrap();
        wb.set_inputs(0, &[(1, 1, "1".into()), (2, 1, "2".into()), (3, 1, "x".into())]).unwrap();
        wb.set_style(0, range(1, 1, 1, 1), "font.b", "true").unwrap();
        wb.set_style(0, range(1, 1, 1, 1), "font.name", "굴림").unwrap();
        let d = wb.detail(0, 1, 1).unwrap();
        assert!(d.style.bold);
        assert_eq!(d.style.font, "굴림");
        let st = wb.selection_stats(0, range(1, 1, 3, 1)).unwrap();
        assert_eq!((st.count, st.numeric_count, st.sum, st.average), (3, 2, 3.0, Some(1.5)));
        wb.undo().unwrap();
        assert_eq!(wb.detail(0, 1, 1).unwrap().style.font, DEFAULT_FONT);
    }

    #[test]
    fn navigation_and_autosum() {
        let mut wb = Workbook::new().unwrap();
        wb.set_inputs(0, &[(1, 1, "1".into()), (2, 1, "2".into()), (3, 1, "3".into())]).unwrap();
        assert_eq!(wb.navigate_edge(0, 1, 1, Direction::Down).unwrap(), (3, 1));
        assert_eq!(wb.autosum_formula(0, 4, 1).unwrap(), "=SUM(A1:A3)");
        assert_eq!(wb.current_region(0, 2, 1).unwrap(), range(1, 1, 3, 1));
        assert_eq!(wb.find(0, "2", (1, 1), false).unwrap(), Some((2, 1)));
    }

    #[test]
    fn ctrl_enter_moves_relative_references() {
        let mut wb = Workbook::new().unwrap();
        wb.set_inputs(0, &[(1, 1, "1".into()), (2, 1, "2".into())]).unwrap();
        wb.fill_range_with(0, range(1, 2, 2, 2), "=A1*10").unwrap();
        assert_eq!(wb.value(0, 2, 2).unwrap(), Scalar::Number(20.0));
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
        wb.save_xlsx(&path).unwrap();
        let reopened = Workbook::open_xlsx(&path).unwrap();
        assert_eq!(reopened.value(0, 1, 1).unwrap(), Scalar::Number(650.0));
        assert_eq!(reopened.value(0, 2, 1).unwrap(), Scalar::Number(325.0));
        std::fs::remove_dir_all(&dir).ok();
    }
}
