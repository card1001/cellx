//! Tauri commands. The UI never talks to IronCalc or DuckDB directly: every call
//! goes through these functions, which run on Tauri's worker threads.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use cellx_core::data::{ColumnFilter, Page, Sort, TableInfo};
use cellx_core::project;
use cellx_core::refs::Axis;
use cellx_core::workbook::{CellDetail, CellRange, Direction, RecalcStats, SelectionStats, SheetLayout, Viewport};
use cellx_core::Workbook;
use serde::Serialize;
use tauri::State;

pub struct Book {
    wb: Workbook,
    path: Option<PathBuf>,
    dirty: bool,
}

impl Book {
    pub fn new() -> cellx_core::Result<Self> {
        Ok(Book { wb: Workbook::new()?, path: None, dirty: false })
    }

    pub fn open(path: &Path) -> cellx_core::Result<Self> {
        let mut wb = if is_project(path) {
            project::open_project(path)?
        } else {
            Workbook::open_xlsx(path)?
        };
        wb.recalc()?;
        Ok(Book { wb, path: Some(path.to_path_buf()), dirty: false })
    }
}

pub struct AppState(pub Mutex<Book>);

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn lock<'a>(state: &'a State<'_, AppState>) -> CmdResult<std::sync::MutexGuard<'a, Book>> {
    state.0.lock().map_err(|_| "내부 상태를 잠글 수 없습니다. 앱을 다시 시작하세요".to_string())
}

/// Runs an edit and marks the book as changed.
fn edit<T>(state: &State<'_, AppState>, f: impl FnOnce(&mut Workbook) -> cellx_core::Result<T>) -> CmdResult<T> {
    let mut book = lock(state)?;
    let out = f(&mut book.wb).map_err(err)?;
    book.dirty = true;
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
}

fn info(book: &mut Book) -> CmdResult<BookInfo> {
    Ok(BookInfo {
        sheets: book.wb.sheet_names(),
        tables: book.wb.tables().map_err(err)?,
        path: book.path.as_ref().map(|p| p.display().to_string()),
        dirty: book.dirty,
        can_undo: book.wb.can_undo(),
        can_redo: book.wb.can_redo(),
        font: book.wb.workbook_font(),
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
pub fn new_book(state: State<'_, AppState>) -> CmdResult<BookInfo> {
    let mut book = lock(&state)?;
    *book = Book::new().map_err(err)?;
    info(&mut book)
}

#[tauri::command]
pub async fn open_file(state: State<'_, AppState>, path: String) -> CmdResult<BookInfo> {
    let opened = Book::open(Path::new(&path)).map_err(err)?;
    let mut book = lock(&state)?;
    *book = opened;
    info(&mut book)
}

#[tauri::command]
pub async fn save_file(state: State<'_, AppState>, path: String) -> CmdResult<BookInfo> {
    let p = PathBuf::from(&path);
    let mut book = lock(&state)?;
    if is_project(&p) {
        project::save_project(&mut book.wb, &p).map_err(err)?;
    } else {
        book.wb.save_xlsx(&p).map_err(err)?;
    }
    book.path = Some(p);
    book.dirty = false;
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
    info(&mut book)
}

#[tauri::command]
pub fn rename_sheet(state: State<'_, AppState>, sheet: u32, name: String) -> CmdResult<BookInfo> {
    let mut book = lock(&state)?;
    book.wb.rename_sheet(sheet, &name).map_err(err)?;
    book.dirty = true;
    info(&mut book)
}

#[tauri::command]
pub fn delete_sheet(state: State<'_, AppState>, sheet: u32) -> CmdResult<BookInfo> {
    let mut book = lock(&state)?;
    book.wb.delete_sheet(sheet).map_err(err)?;
    book.dirty = true;
    info(&mut book)
}

// ---------------------------------------------------------------- reading the grid

#[tauri::command]
pub fn layout(state: State<'_, AppState>, sheet: u32) -> CmdResult<SheetLayout> {
    read(&state, |wb| wb.layout(sheet))
}

#[tauri::command]
pub fn viewport(state: State<'_, AppState>, sheet: u32, row0: i32, col0: i32, row1: i32, col1: i32) -> CmdResult<Viewport> {
    read(&state, |wb| wb.viewport(sheet, row0, col0, row1, col1))
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
pub fn paste(state: State<'_, AppState>, sheet: u32, row: i32, col: i32, text: String) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.paste(sheet, row, col, &text))
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
pub async fn import_data(state: State<'_, AppState>, path: String, name: String) -> CmdResult<TableInfo> {
    edit(&state, |wb| wb.import_data(Path::new(&path), &name))
}

#[tauri::command]
pub async fn query_to_table(state: State<'_, AppState>, sql: String, name: String) -> CmdResult<TableInfo> {
    edit(&state, |wb| wb.create_table_from_query(&name, &sql))
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
pub async fn update_data_cell(state: State<'_, AppState>, table: String, rid: i64, column: String, text: String) -> CmdResult<RecalcStats> {
    edit(&state, |wb| wb.update_data_cell(&table, rid, &column, &text))
}

#[tauri::command]
pub async fn export_table(state: State<'_, AppState>, table: String, path: String) -> CmdResult<()> {
    read(&state, |wb| wb.data().export(&table, Path::new(&path)))
}

#[tauri::command]
pub fn drop_table(state: State<'_, AppState>, table: String) -> CmdResult<BookInfo> {
    let mut book = lock(&state)?;
    book.wb.drop_table(&table).map_err(err)?;
    book.dirty = true;
    info(&mut book)
}
