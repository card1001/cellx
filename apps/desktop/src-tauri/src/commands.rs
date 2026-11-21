//! Tauri commands. The UI never talks to IronCalc or DuckDB directly: every call
//! goes through these functions, which run on Tauri's worker threads.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use cellx_core::data::{ColumnFilter, Page, Sort, TableInfo};
use cellx_core::project;
use cellx_core::workbook::{CellDetail, RecalcStats, Viewport};
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

#[derive(Serialize)]
pub struct BookInfo {
    sheets: Vec<String>,
    tables: Vec<TableInfo>,
    path: Option<String>,
    dirty: bool,
}

fn info(book: &mut Book) -> CmdResult<BookInfo> {
    Ok(BookInfo {
        sheets: book.wb.sheet_names(),
        tables: book.wb.tables().map_err(err)?,
        path: book.path.as_ref().map(|p| p.display().to_string()),
        dirty: book.dirty,
    })
}

fn is_project(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cellx"))
}

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
pub fn dimensions(state: State<'_, AppState>, sheet: u32) -> CmdResult<(i32, i32)> {
    lock(&state)?.wb.dimensions(sheet).map_err(err)
}

#[tauri::command]
pub fn viewport(
    state: State<'_, AppState>,
    sheet: u32,
    row0: i32,
    col0: i32,
    row1: i32,
    col1: i32,
) -> CmdResult<Viewport> {
    lock(&state)?.wb.viewport(sheet, row0, col0, row1, col1).map_err(err)
}

#[tauri::command]
pub fn cell_detail(state: State<'_, AppState>, sheet: u32, row: i32, col: i32) -> CmdResult<CellDetail> {
    lock(&state)?.wb.detail(sheet, row, col).map_err(err)
}

#[tauri::command]
pub fn set_cell(state: State<'_, AppState>, sheet: u32, row: i32, col: i32, input: String) -> CmdResult<RecalcStats> {
    let mut book = lock(&state)?;
    let stats = book.wb.set_input(sheet, row, col, &input).map_err(err)?;
    book.dirty = true;
    Ok(stats)
}

#[tauri::command]
pub fn set_cells(state: State<'_, AppState>, sheet: u32, cells: Vec<(i32, i32, String)>) -> CmdResult<RecalcStats> {
    let mut book = lock(&state)?;
    let stats = book.wb.set_inputs(sheet, &cells).map_err(err)?;
    book.dirty = true;
    Ok(stats)
}

#[tauri::command]
pub fn set_column_width(state: State<'_, AppState>, sheet: u32, col: i32, px: f64) -> CmdResult<()> {
    let mut book = lock(&state)?;
    book.wb.set_column_width(sheet, col, px).map_err(err)?;
    book.dirty = true;
    Ok(())
}

#[tauri::command]
pub fn undo(state: State<'_, AppState>) -> CmdResult<Option<(u32, i32, i32)>> {
    let mut book = lock(&state)?;
    book.dirty = true;
    book.wb.undo().map_err(err)
}

#[tauri::command]
pub fn redo(state: State<'_, AppState>) -> CmdResult<Option<(u32, i32, i32)>> {
    let mut book = lock(&state)?;
    book.dirty = true;
    book.wb.redo().map_err(err)
}

#[tauri::command]
pub async fn import_data(state: State<'_, AppState>, path: String, name: String) -> CmdResult<TableInfo> {
    let mut book = lock(&state)?;
    let info = book.wb.import_data(Path::new(&path), &name).map_err(err)?;
    book.dirty = true;
    Ok(info)
}

#[tauri::command]
pub async fn query_to_table(state: State<'_, AppState>, sql: String, name: String) -> CmdResult<TableInfo> {
    let mut book = lock(&state)?;
    let info = book.wb.create_table_from_query(&name, &sql).map_err(err)?;
    book.dirty = true;
    Ok(info)
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
    let mut book = lock(&state)?;
    book.wb
        .data_page(&table, offset, limit.min(5000), sort.as_ref(), &filters)
        .map_err(err)
}

#[tauri::command]
pub async fn update_data_cell(
    state: State<'_, AppState>,
    table: String,
    rid: i64,
    column: String,
    text: String,
) -> CmdResult<RecalcStats> {
    let mut book = lock(&state)?;
    let stats = book.wb.update_data_cell(&table, rid, &column, &text).map_err(err)?;
    book.dirty = true;
    Ok(stats)
}

#[tauri::command]
pub async fn export_table(state: State<'_, AppState>, table: String, path: String) -> CmdResult<()> {
    let mut book = lock(&state)?;
    book.wb.data().export(&table, Path::new(&path)).map_err(err)
}

#[tauri::command]
pub fn drop_table(state: State<'_, AppState>, table: String) -> CmdResult<BookInfo> {
    let mut book = lock(&state)?;
    book.wb.drop_table(&table).map_err(err)?;
    book.dirty = true;
    info(&mut book)
}
