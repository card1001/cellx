// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

use std::sync::Mutex;

fn main() {
    let book = commands::Book::new().expect("CellX 엔진을 시작하지 못했습니다");
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::AppState(Mutex::new(book)))
        .invoke_handler(tauri::generate_handler![
            commands::book_info,
            commands::new_book,
            commands::open_file,
            commands::save_file,
            commands::add_sheet,
            commands::rename_sheet,
            commands::dimensions,
            commands::viewport,
            commands::cell_detail,
            commands::set_cell,
            commands::set_cells,
            commands::set_column_width,
            commands::undo,
            commands::redo,
            commands::import_data,
            commands::query_to_table,
            commands::data_page,
            commands::update_data_cell,
            commands::export_table,
            commands::drop_table,
        ])
        .run(tauri::generate_context!())
        .expect("CellX를 실행하지 못했습니다");
}
