// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

use std::sync::Mutex;

fn main() {
    // A file passed on the command line (double-click on a .cellx/.xlsx file) opens at start.
    let initial = std::env::args().nth(1).map(std::path::PathBuf::from);
    let book = match initial.as_deref() {
        Some(path) => commands::Book::open(path).unwrap_or_else(|e| {
            eprintln!("{}: {e}", path.display());
            commands::Book::new().expect("CellX 엔진을 시작하지 못했습니다")
        }),
        None => commands::Book::new().expect("CellX 엔진을 시작하지 못했습니다"),
    };
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::AppState(Mutex::new(book)))
        .invoke_handler(tauri::generate_handler![
            commands::book_info,
            commands::new_book,
            commands::open_file,
            commands::save_file,
            commands::function_names,
            commands::add_sheet,
            commands::rename_sheet,
            commands::delete_sheet,
            commands::layout,
            commands::viewport,
            commands::cell_detail,
            commands::selection_stats,
            commands::navigate_edge,
            commands::current_region,
            commands::autosum_formula,
            commands::find,
            commands::set_cell,
            commands::set_cells,
            commands::fill_range,
            commands::clear,
            commands::copy,
            commands::clear_clipboard,
            commands::paste,
            commands::auto_fill,
            commands::undo,
            commands::redo,
            commands::insert_rows,
            commands::delete_rows,
            commands::insert_columns,
            commands::delete_columns,
            commands::set_column_width,
            commands::set_row_height,
            commands::set_hidden,
            commands::set_frozen,
            commands::set_show_grid_lines,
            commands::set_style,
            commands::set_border,
            commands::merge,
            commands::unmerge,
            commands::import_data,
            commands::query_to_table,
            commands::data_page,
            commands::update_data_cell,
            commands::export_table,
            commands::drop_table,
            commands::memos,
            commands::set_memo,
            commands::clear_memos,
            commands::charts,
            commands::add_chart,
            commands::update_chart,
            commands::delete_chart,
            commands::chart_data,
            commands::conditional_formats,
            commands::add_conditional_format,
            commands::delete_conditional_format,
            commands::clear_conditional_formats,
            commands::paste_special,
            commands::pivot_fields,
            commands::pivot_at,
            commands::save_pivot,
            commands::refresh_pivots,
            commands::delete_pivot,
        ])
        .run(tauri::generate_context!())
        .expect("CellX를 실행하지 못했습니다");
}
