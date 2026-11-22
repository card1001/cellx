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

// ---------------------------------------------------------------- memos, charts, conditional formats, pivots

use cellx_core::extras::{Chart, ChartData, ChartKind, PivotDef, PivotSource};
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
