//! Tauri commands. The UI never talks to IronCalc or DuckDB directly: every call
//! goes through these functions, which run on Tauri's worker threads.

use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use cellx_core::data::{ColumnFilter, Page, Sort, TableInfo};
use cellx_core::project;
use cellx_core::refs::Axis;
use cellx_core::workbook::{CellDetail, CellRange, Direction, RecalcStats, SelectionStats, SheetLayout, Viewport};
use cellx_core::Workbook;
use serde::Serialize;
use tauri::State;

use crate::recovery::RecoveryState;
use cellx_core::project::{SavedConnector, SavedPipeline, SavedTransformRun};

pub struct Book {
    pub(crate) wb: Workbook,
    pub(crate) path: Option<PathBuf>,
    pub(crate) dirty: bool,
    /// Counts changes, so autosave knows whether there is anything new.
    pub(crate) edits: u64,
    pub(crate) pipelines: Vec<SavedPipeline>,
    pub(crate) connectors: Vec<SavedConnector>,
    pub(crate) transform_runs: Vec<SavedTransformRun>,
}

impl Book {
    pub fn new() -> cellx_core::Result<Self> {
        Ok(Book { wb: Workbook::new()?, path: None, dirty: false, edits: 0, pipelines: Vec::new(), connectors: Vec::new(), transform_runs: Vec::new() })
    }

    pub fn open(path: &Path) -> cellx_core::Result<Self> {
        let (mut wb, pipelines, connectors, transform_runs) = if is_project(path) {
            project::open_project_with_execution_history(path)?
        } else {
            (Workbook::open_xlsx(path)?, Vec::new(), Vec::new(), Vec::new())
        };
        wb.recalc()?;
        Ok(Book { wb, path: Some(path.to_path_buf()), dirty: false, edits: 0, pipelines, connectors, transform_runs })
    }
}

use crate::data_jobs::{self, DataJobState};

pub struct AppState(pub Arc<Mutex<Book>>);

pub(crate) type CmdResult<T> = Result<T, String>;

static ACTIVE_LOCALE: OnceLock<RwLock<String>> = OnceLock::new();

fn active_locale() -> String {
    ACTIVE_LOCALE
        .get_or_init(|| RwLock::new("ko-KR".to_string()))
        .read()
        .map(|locale| locale.clone())
        .unwrap_or_else(|_| "ko-KR".to_string())
}

pub(crate) fn err(e: impl std::fmt::Display + 'static) -> String {
    if let Some(core_error) = (&e as &dyn Any).downcast_ref::<cellx_core::CellxError>() {
        core_error.localized_message(&active_locale())
    } else {
        e.to_string()
    }
}

#[tauri::command]
pub fn set_locale(locale: String) -> CmdResult<()> {
    let normalized = if locale.to_ascii_lowercase().starts_with("ko") {
        "ko-KR"
    } else if locale.to_ascii_lowercase().starts_with("ja") {
        "ja-JP"
    } else if locale.eq_ignore_ascii_case("zh-TW")
        || locale.to_ascii_lowercase().starts_with("zh-hant")
        || locale.eq_ignore_ascii_case("zh-HK")
    {
        "zh-TW"
    } else if locale.to_ascii_lowercase().starts_with("zh") {
        "zh-CN"
    } else {
        "en-US"
    };
    let mut current = ACTIVE_LOCALE
        .get_or_init(|| RwLock::new("ko-KR".to_string()))
        .write()
        .map_err(|_| "Unable to update application language".to_string())?;
    *current = normalized.to_string();
    Ok(())
}

pub(crate) fn lock<'a>(state: &'a State<'_, AppState>) -> CmdResult<std::sync::MutexGuard<'a, Book>> {
    state.0.lock().map_err(|_| "내부 상태를 잠글 수 없습니다. 앱을 다시 시작하세요".to_string())
}

/// Runs an edit and marks the book as changed.
fn edit<T>(state: &State<'_, AppState>, f: impl FnOnce(&mut Workbook) -> cellx_core::Result<T>) -> CmdResult<T> {
    let mut book = lock(state)?;
    let out = f(&mut book.wb).map_err(err)?;
    book.dirty = true;
    book.edits += 1;
    Ok(out)
}

fn read<T>(state: &State<'_, AppState>, f: impl FnOnce(&mut Workbook) -> cellx_core::Result<T>) -> CmdResult<T> {
    let mut book = lock(state)?;
    f(&mut book.wb).map_err(err)
}

#[derive(Serialize)]
pub struct BookInfo {
    sheets: Vec<String>,
    tables: Vec<TableInfo>,
    path: Option<String>,
    dirty: bool,
    can_undo: bool,
    can_redo: bool,
    font: (String, f64),
    /// Per sheet: is it protected (시트 보호)?
    protected: Vec<bool>,
    /// Per sheet: hidden, tab color.
    tabs: Vec<cellx_core::sheets::SheetTab>,
    /// 통합 문서 보호 (구조), and whether it has a password.
    book_protected: bool,
    book_password: bool,
    /// 1904 날짜 체계.
    date1904: bool,
}

pub(crate) fn info(book: &mut Book) -> CmdResult<BookInfo> {
    Ok(BookInfo {
        sheets: book.wb.sheet_names(),
        tables: book.wb.tables().map_err(err)?,
        path: book.path.as_ref().map(|p| p.display().to_string()),
        dirty: book.dirty,
        can_undo: book.wb.can_undo(),
        can_redo: book.wb.can_redo(),
        protected: (0..book.wb.sheet_names().len() as u32).map(|i| book.wb.protection(i).protected).collect(),
        font: book.wb.workbook_font(),
        tabs: book.wb.sheet_tabs(),
        book_protected: book.wb.book_protection().is_some(),
        book_password: book.wb.book_protection().is_some_and(|p| p.hash.is_some() || p.legacy.is_some()),
        date1904: book.wb.date1904(),
    })
}

fn is_project(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cellx"))
}

// ---------------------------------------------------------------- book and files

#[tauri::command]
pub fn book_info(state: State<'_, AppState>) -> CmdResult<BookInfo> {
    let mut book = lock(&state)?;
    info(&mut book)
}

#[tauri::command]
pub fn new_book(state: State<'_, AppState>, recovery: State<'_, RecoveryState>) -> CmdResult<BookInfo> {
    let mut book = lock(&state)?;
    *book = Book::new().map_err(err)?;
    if let Ok(mut r) = recovery.0.lock() {
        r.clear();
    }
    info(&mut book)
}

#[tauri::command]
pub async fn open_file(state: State<'_, AppState>, recovery: State<'_, RecoveryState>, path: String) -> CmdResult<BookInfo> {
    let opened = Book::open(Path::new(&path)).map_err(err)?;
    let mut book = lock(&state)?;
    *book = opened;
    if let Ok(mut r) = recovery.0.lock() {
        r.clear();
    }
    info(&mut book)
}

#[tauri::command]
pub async fn save_file(state: State<'_, AppState>, recovery: State<'_, RecoveryState>, path: String) -> CmdResult<BookInfo> {
    let p = PathBuf::from(&path);
    let mut book = lock(&state)?;
    if is_project(&p) {
        let pipelines = book.pipelines.clone();
        let connectors = book.connectors.clone();
        let transform_runs = book.transform_runs.clone();
        project::save_project_with_execution_history(&mut book.wb, &p, &pipelines, &connectors, &transform_runs).map_err(err)?;
    } else {
        book.wb.save_xlsx(&p).map_err(err)?;
    }
    book.path = Some(p);
    book.dirty = false;
    if let Ok(mut r) = recovery.0.lock() {
        r.clear();
    }
    info(&mut book)
}

#[tauri::command]
pub fn function_names() -> Vec<String> {
    Workbook::function_names()
}

// ---------------------------------------------------------------- sheets

#[tauri::command]
pub fn add_sheet(state: State<'_, AppState>) -> CmdResult<BookInfo> {
    let mut book = lock(&state)?;
    book.wb.add_sheet().map_err(err)?;
    book.dirty = true;
    book.edits += 1;
    info(&mut book)
}

#[tauri::command]
pub fn rename_sheet(state: State<'_, AppState>, sheet: u32, name: String) -> CmdResult<BookInfo> {
    let mut book = lock(&state)?;
    book.wb.rename_sheet(sheet, &name).map_err(err)?;
    book.dirty = true;
    book.edits += 1;
    info(&mut book)
}

#[tauri::command]
pub fn delete_sheet(state: State<'_, AppState>, sheet: u32) -> CmdResult<BookInfo> {
    let mut book = lock(&state)?;
    book.wb.delete_sheet(sheet).map_err(err)?;
    book.dirty = true;
    book.edits += 1;
    info(&mut book)
}

// ---------------------------------------------------------------- reading the grid

#[tauri::command]
pub fn layout(state: State<'_, AppState>, sheet: u32) -> CmdResult<SheetLayout> {
    read(&state, |wb| wb.layout(sheet))
}

#[tauri::command]
pub fn viewport(
    state: State<'_, AppState>,
    sheet: u32,
    row0: i32,
    col0: i32,
    row1: i32,
    col1: i32,
    formulas: Option<bool>,
) -> CmdResult<Viewport> {
    read(&state, |wb| {
        if formulas == Some(true) {
            wb.viewport_formulas(sheet, row0, col0, row1, col1)
        } else {
            wb.viewport(sheet, row0, col0, row1, col1)
        }
    })
}

#[tauri::command]
pub fn cell_detail(state: State<'_, AppState>, sheet: u32, row: i32, col: i32) -> CmdResult<CellDetail> {
    read(&state, |wb| wb.detail(sheet, row, col))
}

#[tauri::command]
pub fn selection_stats(state: State<'_, AppState>, sheet: u32, range: CellRange) -> CmdResult<SelectionStats> {
    read(&state, |wb| wb.selection_stats(sheet, range))
}

#[tauri::command]
pub fn navigate_edge(state: State<'_, AppState>, sheet: u32, row: i32, col: i32, dir: Direction) -> CmdResult<(i32, i32)> {
    read(&state, |wb| wb.navigate_edge(sheet, row, col, dir))
}

#[tauri::command]
pub fn current_region(state: State<'_, AppState>, sheet: u32, row: i32, col: i32) -> CmdResult<CellRange> {
    read(&state, |wb| wb.current_region(sheet, row, col))
}

#[tauri::command]
pub fn autosum_formula(state: State<'_, AppState>, sheet: u32, row: i32, col: i32) -> CmdResult<String> {
    read(&state, |wb| wb.autosum_formula(sheet, row, col))
}

#[tauri::command]
pub fn find(state: State<'_, AppState>, sheet: u32, query: String, row: i32, col: i32, match_case: bool) -> CmdResult<Option<(i32, i32)>> {
    read(&state, |wb| wb.find(sheet, &query, (row, col), match_case))
}

// ---------------------------------------------------------------- editing

#[tauri::command]
pub fn set_cell(state: State<'_, AppState>, sheet: u32, row: i32, col: i32, input: String) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.set_input(sheet, row, col, &input))
}

#[tauri::command]
pub fn set_cells(state: State<'_, AppState>, sheet: u32, cells: Vec<(i32, i32, String)>) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.set_inputs(sheet, &cells))
}

#[tauri::command]
pub fn fill_range(state: State<'_, AppState>, sheet: u32, range: CellRange, input: String) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.fill_range_with(sheet, range, &input))
}

#[tauri::command]
pub fn clear(state: State<'_, AppState>, sheet: u32, range: CellRange, what: String) -> CmdResult<RecalcStats> {
    edit(&state, |wb| match what.as_str() {
        "formats" => wb.clear_formatting(sheet, range),
        "all" => wb.clear_all(sheet, range),
        _ => wb.clear_contents(sheet, range),
    })
}

#[tauri::command]
pub fn copy(state: State<'_, AppState>, sheet: u32, range: CellRange, cut: bool) -> CmdResult<String> {
    read(&state, |wb| wb.copy(sheet, range, cut))
}

#[tauri::command]
pub fn clear_clipboard(state: State<'_, AppState>) -> CmdResult<()> {
    read(&state, |wb| {
        wb.clear_clipboard();
        Ok(())
    })
}

#[tauri::command]
pub fn paste(state: State<'_, AppState>, sheet: u32, row: i32, col: i32, text: String, internal: bool) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.paste_with(sheet, row, col, &text, internal))
}

#[tauri::command]
pub fn auto_fill(state: State<'_, AppState>, sheet: u32, source: CellRange, target: CellRange) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.auto_fill(sheet, source, target))
}

#[tauri::command]
pub fn undo(state: State<'_, AppState>) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.undo())
}

#[tauri::command]
pub fn redo(state: State<'_, AppState>) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.redo())
}

// ---------------------------------------------------------------- rows and columns

#[tauri::command]
pub fn insert_rows(state: State<'_, AppState>, sheet: u32, at: i32, count: i32) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.insert_rows(sheet, at, count))
}

#[tauri::command]
pub fn delete_rows(state: State<'_, AppState>, sheet: u32, at: i32, count: i32) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.delete_rows(sheet, at, count))
}

#[tauri::command]
pub fn insert_columns(state: State<'_, AppState>, sheet: u32, at: i32, count: i32) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.insert_columns(sheet, at, count))
}

#[tauri::command]
pub fn delete_columns(state: State<'_, AppState>, sheet: u32, at: i32, count: i32) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.delete_columns(sheet, at, count))
}

#[tauri::command]
pub fn set_column_width(state: State<'_, AppState>, sheet: u32, c0: i32, c1: i32, px: f64) -> CmdResult<()> {
    edit(&state, |wb| wb.set_column_width(sheet, c0, c1, px))
}

#[tauri::command]
pub fn set_row_height(state: State<'_, AppState>, sheet: u32, r0: i32, r1: i32, px: f64) -> CmdResult<()> {
    edit(&state, |wb| wb.set_row_height(sheet, r0, r1, px))
}

#[tauri::command]
pub fn set_hidden(state: State<'_, AppState>, sheet: u32, rows: bool, from: i32, to: i32, hidden: bool) -> CmdResult<()> {
    let axis = if rows { Axis::Row } else { Axis::Column };
    edit(&state, |wb| wb.set_hidden(sheet, axis, from, to, hidden))
}

#[tauri::command]
pub fn set_show_grid_lines(state: State<'_, AppState>, sheet: u32, show: bool) -> CmdResult<()> {
    edit(&state, |wb| wb.set_show_grid_lines(sheet, show))
}

#[tauri::command]
pub fn set_frozen(state: State<'_, AppState>, sheet: u32, rows: i32, cols: i32) -> CmdResult<()> {
    edit(&state, |wb| wb.set_frozen(sheet, rows, cols))
}

// ---------------------------------------------------------------- formatting

#[tauri::command]
pub fn set_style(state: State<'_, AppState>, sheet: u32, range: CellRange, path: String, value: String) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.set_style(sheet, range, &path, &value))
}

#[tauri::command]
pub fn set_border(
    state: State<'_, AppState>,
    sheet: u32,
    range: CellRange,
    kind: String,
    style: String,
    color: String,
) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.set_border(sheet, range, &kind, &style, &color))
}

#[tauri::command]
pub fn merge(state: State<'_, AppState>, sheet: u32, range: CellRange, center: bool) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.merge(sheet, range, center))
}

#[tauri::command]
pub fn unmerge(state: State<'_, AppState>, sheet: u32, range: CellRange) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.unmerge(sheet, range))
}

// ---------------------------------------------------------------- data sheets

#[tauri::command]
pub async fn import_data(
    state: State<'_, AppState>,
    jobs: State<'_, DataJobState>,
    path: String,
    name: String,
    locale: String,
    encoding: Option<String>,
) -> CmdResult<TableInfo> {
    let label = file_label(&path);
    data_jobs::run(&state, &jobs, "import", label, true, |wb| {
        wb.import_data_with_encoding(Path::new(&path), &name, &locale, encoding.as_deref())
    })
}

fn file_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// Imports one worksheet of an Excel or OpenDocument file as a data sheet.
#[tauri::command]
pub async fn import_spreadsheet(
    state: State<'_, AppState>,
    jobs: State<'_, DataJobState>,
    path: String,
    name: String,
    sheet: Option<String>,
    header: bool,
) -> CmdResult<TableInfo> {
    let label = file_label(&path);
    data_jobs::run(&state, &jobs, "import", label, true, |wb| {
        wb.import_spreadsheet_data(Path::new(&path), &name, sheet.as_deref(), header)
    })
}

/// The worksheets of an Excel or OpenDocument file, for the import dialog.
#[tauri::command]
pub async fn spreadsheet_sheets(path: String) -> CmdResult<Vec<String>> {
    cellx_core::data::spreadsheet_sheets(Path::new(&path)).map_err(err)
}

/// The first lines of a data file as the chosen (or detected) encoding reads them.
#[tauri::command]
pub async fn preview_data_file(
    path: String,
    locale: String,
    encoding: Option<String>,
) -> CmdResult<cellx_core::data::FilePreview> {
    cellx_core::data::DataStore::preview_file(Path::new(&path), &locale, encoding.as_deref(), 12).map_err(err)
}

#[tauri::command]
pub async fn query_to_table(
    state: State<'_, AppState>,
    jobs: State<'_, DataJobState>,
    sql: String,
    name: String,
) -> CmdResult<TableInfo> {
    let label = name.clone();
    data_jobs::run(&state, &jobs, "query", label, true, |wb| wb.create_table_from_query(&name, &sql))
}

#[tauri::command]
pub async fn data_page(
    state: State<'_, AppState>,
    table: String,
    offset: u64,
    limit: u64,
    sort: Option<Sort>,
    filters: Vec<ColumnFilter>,
) -> CmdResult<Page> {
    read(&state, |wb| wb.data_page(&table, offset, limit.min(5000), sort.as_ref(), &filters))
}

#[tauri::command]
pub async fn data_page_after_rid(
    state: State<'_, AppState>,
    table: String,
    after_rid: Option<i64>,
    limit: u64,
    filters: Vec<ColumnFilter>,
) -> CmdResult<Page> {
    read(&state, |wb| wb.data_page_after_rid(&table, after_rid, limit.min(5000), &filters))
}

#[tauri::command]
pub async fn update_data_cell(state: State<'_, AppState>, table: String, rid: i64, column: String, text: String) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.update_data_cell(&table, rid, &column, &text))
}

#[tauri::command]
pub async fn export_table(
    state: State<'_, AppState>,
    jobs: State<'_, DataJobState>,
    table: String,
    path: String,
    locale: String,
    encoding: Option<String>,
) -> CmdResult<()> {
    let _ = locale;
    let label = file_label(&path);
    data_jobs::run(&state, &jobs, "export", label, false, |wb| {
        wb.data().export_with_encoding(&table, Path::new(&path), encoding.as_deref())
    })
}

#[tauri::command]
pub fn drop_table(state: State<'_, AppState>, table: String) -> CmdResult<BookInfo> {
    let mut book = lock(&state)?;
    book.wb.drop_table(&table).map_err(err)?;
    book.dirty = true;
    book.edits += 1;
    info(&mut book)
}

// ---------------------------------------------------------------- memos, charts, conditional formats, pivots

use cellx_core::extras::{Anchor, Chart, ChartData, ChartKind, PivotDef, PivotSource};
use cellx_core::objects::{CfRuleInput, ConditionalFormattingView, Memo, PasteSpecial};

#[tauri::command]
pub fn memos(state: State<'_, AppState>, sheet: u32) -> CmdResult<Vec<Memo>> {
    read(&state, |wb| wb.memos(sheet))
}

#[tauri::command]
pub fn set_memo(state: State<'_, AppState>, sheet: u32, row: i32, col: i32, text: Option<String>) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.set_memo(sheet, row, col, text.as_deref()))
}

#[tauri::command]
pub fn clear_memos(state: State<'_, AppState>, sheet: u32, range: CellRange) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.clear_memos(sheet, range))
}

#[tauri::command]
pub fn move_sheet(state: State<'_, AppState>, from: u32, to: u32) -> CmdResult<BookInfo> {
    let mut book = lock(&state)?;
    book.wb.move_sheet(from, to).map_err(err)?;
    book.dirty = true;
    book.edits += 1;
    info(&mut book)
}

#[tauri::command]
pub fn copy_sheet(state: State<'_, AppState>, sheet: u32, to: u32) -> CmdResult<BookInfo> {
    let mut book = lock(&state)?;
    book.wb.copy_sheet(sheet, to).map_err(err)?;
    book.dirty = true;
    book.edits += 1;
    info(&mut book)
}

#[tauri::command]
pub fn set_sheet_color(state: State<'_, AppState>, sheet: u32, color: Option<String>) -> CmdResult<BookInfo> {
    let mut book = lock(&state)?;
    book.wb.set_sheet_color(sheet, color).map_err(err)?;
    book.dirty = true;
    book.edits += 1;
    info(&mut book)
}

#[tauri::command]
pub fn set_sheet_hidden(state: State<'_, AppState>, sheet: u32, hidden: bool) -> CmdResult<BookInfo> {
    let mut book = lock(&state)?;
    book.wb.set_sheet_hidden(sheet, hidden).map_err(err)?;
    book.dirty = true;
    book.edits += 1;
    info(&mut book)
}

#[tauri::command]
pub fn pictures(state: State<'_, AppState>, sheet: u32) -> CmdResult<Vec<cellx_core::pictures::Picture>> {
    read(&state, |wb| Ok(wb.pictures(sheet)))
}

#[tauri::command]
pub fn picture_data(state: State<'_, AppState>, id: u32) -> CmdResult<String> {
    read(&state, |wb| wb.picture_data_url(id))
}

#[tauri::command]
pub fn add_picture_file(state: State<'_, AppState>, sheet: u32, row: i32, col: i32, path: String) -> CmdResult<cellx_core::pictures::Picture> {
    let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    edit(&state, |wb| wb.add_picture(sheet, row, col, bytes))
}

#[tauri::command]
pub fn add_picture_data(state: State<'_, AppState>, sheet: u32, row: i32, col: i32, data: String) -> CmdResult<cellx_core::pictures::Picture> {
    edit(&state, |wb| wb.add_picture_base64(sheet, row, col, &data))
}

#[tauri::command]
pub fn update_picture(state: State<'_, AppState>, id: u32, name: String, anchor: Anchor) -> CmdResult<()> {
    edit(&state, |wb| wb.update_picture(id, name, anchor))
}

#[tauri::command]
pub fn delete_picture(state: State<'_, AppState>, id: u32) -> CmdResult<()> {
    edit(&state, |wb| wb.delete_picture(id))
}

#[tauri::command]
pub fn charts(state: State<'_, AppState>, sheet: u32) -> CmdResult<Vec<Chart>> {
    read(&state, |wb| Ok(wb.charts(sheet)))
}

#[tauri::command]
pub fn add_chart(state: State<'_, AppState>, sheet: u32, kind: ChartKind, range: CellRange) -> CmdResult<Chart> {
    edit(&state, |wb| wb.add_chart(sheet, kind, range))
}

#[tauri::command]
pub fn update_chart(state: State<'_, AppState>, chart: Chart) -> CmdResult<()> {
    edit(&state, |wb| wb.update_chart(chart))
}

#[tauri::command]
pub fn delete_chart(state: State<'_, AppState>, id: u32) -> CmdResult<()> {
    edit(&state, |wb| wb.delete_chart(id))
}

#[tauri::command]
pub fn chart_data(state: State<'_, AppState>, id: u32) -> CmdResult<ChartData> {
    read(&state, |wb| wb.chart_data(id))
}

#[tauri::command]
pub fn conditional_formats(state: State<'_, AppState>, sheet: u32) -> CmdResult<Vec<ConditionalFormattingView>> {
    read(&state, |wb| wb.conditional_formats(sheet))
}

#[tauri::command]
pub fn add_conditional_format(state: State<'_, AppState>, sheet: u32, range: CellRange, rule: CfRuleInput) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.add_conditional_format(sheet, range, rule))
}

#[tauri::command]
pub fn delete_conditional_format(state: State<'_, AppState>, sheet: u32, index: u32) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.delete_conditional_format(sheet, index))
}

#[tauri::command]
pub fn clear_conditional_formats(state: State<'_, AppState>, sheet: u32, range: Option<CellRange>) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.clear_conditional_formats(sheet, range))
}

#[tauri::command]
pub fn paste_special(state: State<'_, AppState>, sheet: u32, row: i32, col: i32, options: PasteSpecial) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.paste_special(sheet, row, col, options))
}

#[tauri::command]
pub fn pivot_fields(state: State<'_, AppState>, source: PivotSource, sheet: u32) -> CmdResult<Vec<String>> {
    read(&state, |wb| wb.pivot_fields(&source, sheet))
}

#[tauri::command]
pub fn pivot_at(state: State<'_, AppState>, sheet: u32, row: i32, col: i32) -> CmdResult<Option<PivotDef>> {
    read(&state, |wb| Ok(wb.pivot_at(sheet, row, col)))
}

#[tauri::command]
pub fn save_pivot(state: State<'_, AppState>, def: PivotDef, new_sheet: bool) -> CmdResult<PivotDef> {
    edit(&state, |wb| wb.save_pivot(def, new_sheet))
}

#[tauri::command]
pub fn refresh_pivots(state: State<'_, AppState>) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.refresh_pivots())
}

#[tauri::command]
pub fn delete_pivot(state: State<'_, AppState>, id: u32) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.delete_pivot(id))
}

// ---------------------------------------------------------------- data tools

use cellx_core::datatools::{
    AutoFilter, ColumnFilter as SheetColumnFilter, DuplicateResult, FilterValue, GoalSeekResult, LinkView, NameView, SortKey, SplitOptions,
    Validation, ValidationFailure,
};

#[tauri::command]
pub fn sort_range(state: State<'_, AppState>, sheet: u32, range: CellRange, keys: Vec<SortKey>, has_header: bool) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.sort_range(sheet, range, &keys, has_header))
}

#[tauri::command]
pub fn toggle_auto_filter(state: State<'_, AppState>, sheet: u32, range: CellRange) -> CmdResult<Option<AutoFilter>> {
    edit(&state, |wb| wb.toggle_auto_filter(sheet, range))
}

#[tauri::command]
pub fn auto_filter(state: State<'_, AppState>, sheet: u32) -> CmdResult<Option<AutoFilter>> {
    read(&state, |wb| Ok(wb.auto_filter(sheet)))
}

#[tauri::command]
pub fn set_filter_column(state: State<'_, AppState>, sheet: u32, filter: SheetColumnFilter) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.set_filter_column(sheet, filter))
}

#[tauri::command]
pub fn clear_filters(state: State<'_, AppState>, sheet: u32) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.clear_filters(sheet))
}

#[tauri::command]
pub fn apply_filter(state: State<'_, AppState>, sheet: u32) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.apply_filter(sheet))
}

#[tauri::command]
pub fn filter_values(state: State<'_, AppState>, sheet: u32, col: i32) -> CmdResult<Vec<FilterValue>> {
    read(&state, |wb| wb.filter_values(sheet, col))
}

#[tauri::command]
pub fn text_to_columns(state: State<'_, AppState>, sheet: u32, range: CellRange, options: SplitOptions) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.text_to_columns(sheet, range, &options))
}

#[tauri::command]
pub fn remove_duplicates(state: State<'_, AppState>, sheet: u32, range: CellRange, cols: Vec<i32>, has_header: bool) -> CmdResult<DuplicateResult> {
    edit(&state, |wb| wb.remove_duplicates(sheet, range, &cols, has_header))
}

#[tauri::command]
pub fn validation_at(state: State<'_, AppState>, sheet: u32, row: i32, col: i32) -> CmdResult<Option<Validation>> {
    read(&state, |wb| Ok(wb.validation_at(sheet, row, col)))
}

#[tauri::command]
pub fn set_validation(state: State<'_, AppState>, sheet: u32, range: CellRange, validation: Validation) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.set_validation(sheet, range, validation))
}

#[tauri::command]
pub fn clear_validation(state: State<'_, AppState>, sheet: u32, range: CellRange) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.clear_validation(sheet, range))
}

#[tauri::command]
pub fn validate_input(state: State<'_, AppState>, sheet: u32, row: i32, col: i32, input: String) -> CmdResult<Option<ValidationFailure>> {
    read(&state, |wb| Ok(wb.validate_input(sheet, row, col, &input)))
}

#[tauri::command]
pub fn list_items(state: State<'_, AppState>, sheet: u32, row: i32, col: i32) -> CmdResult<Option<Vec<String>>> {
    read(&state, |wb| Ok(wb.list_items(sheet, row, col)))
}

#[tauri::command]
pub fn flash_fill(state: State<'_, AppState>, sheet: u32, row: i32, col: i32) -> CmdResult<(usize, RecalcStats)> {
    edit(&state, |wb| wb.flash_fill(sheet, row, col))
}

#[tauri::command]
pub fn replace_all(
    state: State<'_, AppState>,
    sheet: Option<u32>,
    find: String,
    replace: String,
    match_case: bool,
    whole_cell: bool,
) -> CmdResult<(usize, RecalcStats)> {
    edit(&state, |wb| wb.replace_all(sheet, &find, &replace, match_case, whole_cell))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn replace_in_cell(
    state: State<'_, AppState>,
    sheet: u32,
    row: i32,
    col: i32,
    find: String,
    replace: String,
    match_case: bool,
    whole_cell: bool,
) -> CmdResult<bool> {
    edit(&state, |wb| wb.replace_in_cell(sheet, row, col, &find, &replace, match_case, whole_cell))
}

#[tauri::command]
pub fn names(state: State<'_, AppState>) -> CmdResult<Vec<NameView>> {
    read(&state, |wb| Ok(wb.names()))
}

#[tauri::command]
pub fn add_name(state: State<'_, AppState>, name: String, scope: Option<u32>, formula: String) -> CmdResult<()> {
    edit(&state, |wb| wb.add_name(&name, scope, &formula))
}

#[tauri::command]
pub fn update_name(
    state: State<'_, AppState>,
    name: String,
    scope: Option<u32>,
    new_name: String,
    new_scope: Option<u32>,
    formula: String,
) -> CmdResult<()> {
    edit(&state, |wb| wb.update_name(&name, scope, &new_name, new_scope, &formula))
}

#[tauri::command]
pub fn delete_name(state: State<'_, AppState>, name: String, scope: Option<u32>) -> CmdResult<()> {
    edit(&state, |wb| wb.delete_name(&name, scope))
}

#[tauri::command]
pub fn link_at(state: State<'_, AppState>, sheet: u32, row: i32, col: i32) -> CmdResult<Option<LinkView>> {
    read(&state, |wb| wb.link_at(sheet, row, col))
}

#[tauri::command]
pub fn set_link(
    state: State<'_, AppState>,
    sheet: u32,
    row: i32,
    col: i32,
    target: String,
    internal: bool,
    text: Option<String>,
) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.set_link(sheet, row, col, &target, internal, text.as_deref()))
}

#[tauri::command]
pub fn delete_link(state: State<'_, AppState>, sheet: u32, row: i32, col: i32) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.delete_link(sheet, row, col))
}

#[tauri::command]
pub fn goal_seek(state: State<'_, AppState>, sheet: u32, set_cell: (i32, i32), target: f64, changing: (i32, i32)) -> CmdResult<GoalSeekResult> {
    edit(&state, |wb| wb.goal_seek(sheet, set_cell, target, changing))
}

/// Opens a hyperlink in the default browser or mail app. Only web and mail links:
/// a link in a file must never start a program.
#[tauri::command]
pub fn open_link(app: tauri::AppHandle, url: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let lower = url.trim().to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")) {
        return Err("웹 주소(http, https)와 메일 주소(mailto)만 열 수 있습니다".into());
    }
    app.opener().open_url(url.trim(), None::<&str>).map_err(err)
}

/// Ctrl+P: the system print dialog (PDF is one of its printers).
#[tauri::command]
pub fn print_page(window: tauri::WebviewWindow) -> CmdResult<()> {
    window.print().map_err(err)
}

// ---------------------------------------------------------------- outline, tables, protection, page setup

use cellx_core::outline::{OutlineView, SubtotalOptions};
use cellx_core::sheetopts::{PageSetup, ProtectOptions, ProtectionView, TableUpdate, TableView};

fn axis_of(rows: bool) -> Axis {
    if rows {
        Axis::Row
    } else {
        Axis::Column
    }
}

#[tauri::command]
pub fn outline(state: State<'_, AppState>, sheet: u32) -> CmdResult<OutlineView> {
    read(&state, |wb| Ok(wb.outline(sheet)))
}

#[tauri::command]
pub fn group(state: State<'_, AppState>, sheet: u32, rows: bool, from: i32, to: i32) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.group(sheet, axis_of(rows), from, to))
}

#[tauri::command]
pub fn ungroup(state: State<'_, AppState>, sheet: u32, rows: bool, from: i32, to: i32) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.ungroup(sheet, axis_of(rows), from, to))
}

#[tauri::command]
pub fn clear_outline(state: State<'_, AppState>, sheet: u32) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.clear_outline(sheet))
}

#[tauri::command]
pub fn toggle_group(state: State<'_, AppState>, sheet: u32, rows: bool, index: i32, level: u8, collapse: bool) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.toggle_group(sheet, axis_of(rows), index, level, collapse))
}

#[tauri::command]
pub fn show_outline_level(state: State<'_, AppState>, sheet: u32, rows: bool, level: u8) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.show_outline_level(sheet, axis_of(rows), level))
}

#[tauri::command]
pub fn subtotal(state: State<'_, AppState>, sheet: u32, range: CellRange, options: SubtotalOptions) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.subtotal(sheet, range, &options))
}

#[tauri::command]
pub fn remove_subtotals(state: State<'_, AppState>, sheet: u32, range: CellRange) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.remove_subtotals(sheet, range))
}

#[tauri::command]
pub fn tables_on(state: State<'_, AppState>, sheet: u32) -> CmdResult<Vec<TableView>> {
    read(&state, |wb| Ok(wb.tables_on(sheet)))
}

#[tauri::command]
pub fn create_table(state: State<'_, AppState>, sheet: u32, range: CellRange, has_header: bool, style: String) -> CmdResult<TableView> {
    edit(&state, |wb| wb.create_table(sheet, range, has_header, &style))
}

#[tauri::command]
pub fn update_table(state: State<'_, AppState>, update: TableUpdate) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.update_table(&update))
}

#[tauri::command]
pub fn convert_table_to_range(state: State<'_, AppState>, name: String) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.convert_table_to_range(&name))
}

#[tauri::command]
pub fn protection(state: State<'_, AppState>, sheet: u32) -> CmdResult<ProtectionView> {
    read(&state, |wb| Ok(wb.protection(sheet)))
}

#[tauri::command]
pub fn protect_sheet(state: State<'_, AppState>, sheet: u32, password: Option<String>, options: ProtectOptions) -> CmdResult<()> {
    edit(&state, |wb| wb.protect_sheet(sheet, password.as_deref(), options))
}

#[tauri::command]
pub fn unprotect_sheet(state: State<'_, AppState>, sheet: u32, password: Option<String>) -> CmdResult<()> {
    edit(&state, |wb| wb.unprotect_sheet(sheet, password.as_deref()))
}

#[tauri::command]
pub fn set_locked(state: State<'_, AppState>, sheet: u32, range: CellRange, locked: bool) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.set_locked(sheet, range, locked))
}

#[tauri::command]
pub fn page_setup(state: State<'_, AppState>, sheet: u32) -> CmdResult<PageSetup> {
    read(&state, |wb| Ok(wb.page_setup(sheet)))
}

#[tauri::command]
pub fn set_page_setup(state: State<'_, AppState>, setup: PageSetup) -> CmdResult<()> {
    edit(&state, |wb| wb.set_page_setup(setup))
}

#[tauri::command]
pub fn insert_page_break(state: State<'_, AppState>, sheet: u32, row: i32, col: i32) -> CmdResult<()> {
    edit(&state, |wb| wb.insert_page_break(sheet, row, col))
}

#[tauri::command]
pub fn remove_page_break(state: State<'_, AppState>, sheet: u32, row: i32, col: i32) -> CmdResult<()> {
    edit(&state, |wb| wb.remove_page_break(sheet, row, col))
}

#[tauri::command]
pub fn reset_page_breaks(state: State<'_, AppState>, sheet: u32) -> CmdResult<()> {
    edit(&state, |wb| wb.reset_page_breaks(sheet))
}

use cellx_core::audit::{EvalStep, TraceArea};

#[tauri::command]
pub async fn formula_dependency_graph(
    state: State<'_, AppState>,
) -> CmdResult<cellx_core::audit::FormulaDependencyGraph> {
    read(&state, |wb| wb.formula_dependency_graph())
}

#[tauri::command]
pub fn trace_precedents(state: State<'_, AppState>, sheet: u32, row: i32, col: i32) -> CmdResult<Vec<TraceArea>> {
    read(&state, |wb| Ok(wb.trace_precedents(sheet, row, col)))
}

#[tauri::command]
pub fn trace_dependents(state: State<'_, AppState>, sheet: u32, row: i32, col: i32) -> CmdResult<Vec<TraceArea>> {
    read(&state, |wb| Ok(wb.trace_dependents(sheet, row, col)))
}

#[tauri::command]
pub fn formula_steps(state: State<'_, AppState>, sheet: u32, row: i32, col: i32) -> CmdResult<Vec<EvalStep>> {
    read(&state, |wb| wb.formula_steps(sheet, row, col))
}

use cellx_core::shapes::{Shape, ShapeKind};

#[tauri::command]
pub fn shapes(state: State<'_, AppState>, sheet: u32) -> CmdResult<Vec<Shape>> {
    read(&state, |wb| Ok(wb.shapes(sheet)))
}

#[tauri::command]
pub fn add_shape(state: State<'_, AppState>, sheet: u32, row: i32, col: i32, kind: ShapeKind) -> CmdResult<Shape> {
    edit(&state, |wb| wb.add_shape(sheet, row, col, kind))
}

#[tauri::command]
pub fn update_shape(state: State<'_, AppState>, shape: Shape) -> CmdResult<()> {
    edit(&state, |wb| wb.update_shape(shape))
}

#[tauri::command]
pub fn delete_shape(state: State<'_, AppState>, id: u32) -> CmdResult<()> {
    edit(&state, |wb| wb.delete_shape(id))
}

use cellx_core::slicers::{Slicer, SlicerView};
use cellx_core::sparklines::{SparkKind, SparklineGroup, SparklineView};

#[tauri::command]
pub fn sparklines(state: State<'_, AppState>, sheet: u32) -> CmdResult<Vec<SparklineView>> {
    read(&state, |wb| Ok(wb.sparklines(sheet)))
}

#[tauri::command]
pub fn sparkline_groups(state: State<'_, AppState>, sheet: u32) -> CmdResult<Vec<SparklineGroup>> {
    read(&state, |wb| Ok(wb.sparkline_groups(sheet)))
}

#[tauri::command]
pub fn add_sparklines(state: State<'_, AppState>, sheet: u32, kind: SparkKind, data: String, location: CellRange) -> CmdResult<SparklineGroup> {
    edit(&state, |wb| wb.add_sparklines(sheet, kind, &data, location))
}

#[tauri::command]
pub fn update_sparkline_group(state: State<'_, AppState>, group: SparklineGroup) -> CmdResult<()> {
    edit(&state, |wb| wb.update_sparkline_group(group))
}

#[tauri::command]
pub fn delete_sparklines(state: State<'_, AppState>, sheet: u32, range: CellRange) -> CmdResult<()> {
    edit(&state, |wb| wb.delete_sparklines(sheet, range))
}

#[tauri::command]
pub fn slicers(state: State<'_, AppState>, sheet: u32) -> CmdResult<Vec<SlicerView>> {
    read(&state, |wb| Ok(wb.slicers(sheet)))
}

#[tauri::command]
pub fn add_slicer(state: State<'_, AppState>, table: String, column: String, sheet: u32) -> CmdResult<Slicer> {
    edit(&state, |wb| wb.add_slicer(&table, &column, sheet))
}

#[tauri::command]
pub fn update_slicer(state: State<'_, AppState>, slicer: Slicer) -> CmdResult<()> {
    edit(&state, |wb| wb.update_slicer(slicer))
}

#[tauri::command]
pub fn delete_slicer(state: State<'_, AppState>, id: u32) -> CmdResult<()> {
    edit(&state, |wb| wb.delete_slicer(id))
}

#[tauri::command]
pub fn slicer_select(state: State<'_, AppState>, id: u32, values: Option<Vec<String>>) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.slicer_select(id, values))
}

use cellx_core::whatif::Scenario;

#[tauri::command]
pub fn add_data_table(
    state: State<'_, AppState>,
    sheet: u32,
    range: CellRange,
    row_input: Option<String>,
    col_input: Option<String>,
) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.add_data_table(sheet, range, row_input.as_deref(), col_input.as_deref()))
}

#[tauri::command]
pub fn delete_data_table(state: State<'_, AppState>, sheet: u32, row: i32, col: i32) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.delete_data_table(sheet, row, col))
}

#[tauri::command]
pub fn data_table_at(state: State<'_, AppState>, sheet: u32, row: i32, col: i32) -> CmdResult<bool> {
    read(&state, |wb| Ok(wb.data_table_at(sheet, row, col).is_some()))
}

#[tauri::command]
pub fn scenarios(state: State<'_, AppState>, sheet: u32) -> CmdResult<Vec<Scenario>> {
    read(&state, |wb| Ok(wb.scenarios(sheet)))
}

#[tauri::command]
pub fn set_scenario(state: State<'_, AppState>, scenario: Scenario) -> CmdResult<()> {
    edit(&state, |wb| wb.set_scenario(scenario))
}

#[tauri::command]
pub fn delete_scenario(state: State<'_, AppState>, sheet: u32, name: String) -> CmdResult<()> {
    edit(&state, |wb| wb.delete_scenario(sheet, &name))
}

#[tauri::command]
pub fn show_scenario(state: State<'_, AppState>, sheet: u32, name: String) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.show_scenario(sheet, &name))
}

#[tauri::command]
pub fn scenario_summary(state: State<'_, AppState>, sheet: u32, results: Vec<(i32, i32)>) -> CmdResult<u32> {
    edit(&state, |wb| wb.scenario_summary(sheet, &results))
}

use cellx_core::book::CustomView;

#[tauri::command]
pub fn set_date1904(state: State<'_, AppState>, on: bool) -> CmdResult<cellx_core::workbook::RecalcStats> {
    edit(&state, |wb| wb.set_date1904(on))
}

#[tauri::command]
pub fn protect_book(state: State<'_, AppState>, password: Option<String>) -> CmdResult<()> {
    edit(&state, |wb| wb.protect_book(password.as_deref()))
}

#[tauri::command]
pub fn unprotect_book(state: State<'_, AppState>, password: Option<String>) -> CmdResult<()> {
    edit(&state, |wb| wb.unprotect_book(password.as_deref()))
}

#[tauri::command]
pub fn custom_views(state: State<'_, AppState>) -> CmdResult<Vec<CustomView>> {
    read(&state, |wb| Ok(wb.custom_views()))
}

#[tauri::command]
pub fn add_custom_view(state: State<'_, AppState>, name: String, active: u32) -> CmdResult<CustomView> {
    edit(&state, |wb| wb.add_custom_view(&name, active))
}

#[tauri::command]
pub fn delete_custom_view(state: State<'_, AppState>, name: String) -> CmdResult<()> {
    edit(&state, |wb| wb.delete_custom_view(&name))
}

#[tauri::command]
pub fn show_custom_view(state: State<'_, AppState>, name: String) -> CmdResult<u32> {
    edit(&state, |wb| wb.show_custom_view(&name))
}
