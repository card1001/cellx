// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod recovery;
mod pipeline_commands;
mod connector_commands;
mod transform_jobs;

use std::sync::{Arc, Mutex};

use tauri::Manager;

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
        .plugin(tauri_plugin_opener::init())
        .manage(commands::AppState(Arc::new(Mutex::new(book))))
        .manage(transform_jobs::JobState::default())
        .setup(|app| {
            // 자동 복구 files live with the app's local data.
            let dir = app
                .path()
                .app_local_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("CellX"))
                .join("recovery");
            app.manage(recovery::RecoveryState(Mutex::new(recovery::Recovery::new(dir))));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::book_info,
            commands::set_locale,
            recovery::autosave,
            recovery::recovery_list,
            recovery::recovery_open,
            recovery::recovery_discard,
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
            pipeline_commands::profile_data_table,
            transform_jobs::start_transform_job,
            transform_jobs::get_transform_job,
            transform_jobs::cancel_transform_job,
            pipeline_commands::transform_pipelines,
            pipeline_commands::save_transform_pipeline,
            pipeline_commands::delete_transform_pipeline,
            pipeline_commands::preview_transform,
            pipeline_commands::execute_transform_pipeline,
            connector_commands::data_connectors,
            connector_commands::save_data_connector,
            connector_commands::refresh_data_connector,
            connector_commands::delete_data_connector,
            commands::data_page,
            commands::update_data_cell,
            commands::export_table,
            commands::drop_table,
            commands::memos,
            commands::set_memo,
            commands::clear_memos,
            commands::move_sheet,
            commands::copy_sheet,
            commands::set_sheet_color,
            commands::set_sheet_hidden,
            commands::pictures,
            commands::picture_data,
            commands::add_picture_file,
            commands::add_picture_data,
            commands::update_picture,
            commands::delete_picture,
            commands::trace_precedents,
            commands::trace_dependents,
            commands::formula_steps,
            commands::shapes,
            commands::add_shape,
            commands::update_shape,
            commands::delete_shape,
            commands::sparklines,
            commands::sparkline_groups,
            commands::add_sparklines,
            commands::update_sparkline_group,
            commands::delete_sparklines,
            commands::slicers,
            commands::add_slicer,
            commands::update_slicer,
            commands::delete_slicer,
            commands::slicer_select,
            commands::add_data_table,
            commands::delete_data_table,
            commands::data_table_at,
            commands::scenarios,
            commands::set_scenario,
            commands::delete_scenario,
            commands::show_scenario,
            commands::scenario_summary,
            commands::protect_book,
            commands::set_date1904,
            commands::unprotect_book,
            commands::custom_views,
            commands::add_custom_view,
            commands::delete_custom_view,
            commands::show_custom_view,
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
            commands::sort_range,
            commands::toggle_auto_filter,
            commands::auto_filter,
            commands::set_filter_column,
            commands::clear_filters,
            commands::apply_filter,
            commands::filter_values,
            commands::text_to_columns,
            commands::remove_duplicates,
            commands::validation_at,
            commands::set_validation,
            commands::clear_validation,
            commands::validate_input,
            commands::list_items,
            commands::flash_fill,
            commands::replace_all,
            commands::replace_in_cell,
            commands::names,
            commands::add_name,
            commands::update_name,
            commands::delete_name,
            commands::link_at,
            commands::set_link,
            commands::delete_link,
            commands::goal_seek,
            commands::open_link,
            commands::print_page,
            commands::outline,
            commands::group,
            commands::ungroup,
            commands::clear_outline,
            commands::toggle_group,
            commands::show_outline_level,
            commands::subtotal,
            commands::remove_subtotals,
            commands::tables_on,
            commands::create_table,
            commands::update_table,
            commands::convert_table_to_range,
            commands::protection,
            commands::protect_sheet,
            commands::unprotect_sheet,
            commands::set_locked,
            commands::page_setup,
            commands::set_page_setup,
            commands::insert_page_break,
            commands::remove_page_break,
            commands::reset_page_breaks,
        ])
        .build(tauri::generate_context!())
        .expect("CellX를 실행하지 못했습니다")
        .run(|app, event| {
            // A clean exit leaves no recovery file behind.
            if let tauri::RunEvent::Exit = event {
                if let Some(r) = app.try_state::<recovery::RecoveryState>() {
                    if let Ok(mut r) = r.0.lock() {
                        r.finish();
                    }
                }
            }
        });
}
