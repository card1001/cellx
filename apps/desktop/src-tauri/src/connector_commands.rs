//! Tauri commands for reusable, local-only file connectors.
use std::path::Path;

use cellx_core::data::TableInfo;
use cellx_core::project::SavedConnector;
use tauri::State;

use crate::commands::AppState;

type CmdResult<T> = Result<T, String>;

#[tauri::command]
pub fn data_connectors(state: State<'_, AppState>) -> CmdResult<Vec<SavedConnector>> {
    let book = state.0.lock().map_err(|_| "내부 상태를 잠글 수 없습니다".to_string())?;
    Ok(book.connectors.clone())
}

#[tauri::command]
pub fn save_data_connector(state: State<'_, AppState>, connector: SavedConnector) -> CmdResult<TableInfo> {
    if connector.id.trim().is_empty() || connector.id.len() > 80 {
        return Err("연결 ID가 올바르지 않습니다".into());
    }
    if connector.name.trim().is_empty() || connector.name.len() > 120 {
        return Err("연결 이름은 1~120자여야 합니다".into());
    }
    if connector.path.trim().is_empty() {
        return Err("로컬 파일 경로가 비어 있습니다".into());
    }
    let mut book = state.0.lock().map_err(|_| "내부 상태를 잠글 수 없습니다".to_string())?;
    // The user-selected local path is passed to the existing parameterized import path.
    let table = book.wb.import_data_with_locale(
        Path::new(&connector.path),
        &connector.table_name,
        &connector.locale,
    ).map_err(|e| e.to_string())?;
    if let Some(existing) = book.connectors.iter_mut().find(|item| item.id == connector.id) {
        *existing = connector;
    } else {
        book.connectors.push(connector);
    }
    book.connectors.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    book.dirty = true;
    book.edits = book.edits.saturating_add(1);
    Ok(table)
}

#[tauri::command]
pub fn refresh_data_connector(state: State<'_, AppState>, id: String) -> CmdResult<TableInfo> {
    let mut book = state.0.lock().map_err(|_| "내부 상태를 잠글 수 없습니다".to_string())?;
    let connector = book.connectors.iter().find(|item| item.id == id)
        .ok_or_else(|| "저장된 데이터 연결을 찾을 수 없습니다".to_string())?
        .clone();
    let table = book.wb.import_data_with_locale(
        Path::new(&connector.path),
        &connector.table_name,
        &connector.locale,
    ).map_err(|e| e.to_string())?;
    book.dirty = true;
    book.edits = book.edits.saturating_add(1);
    Ok(table)
}

#[tauri::command]
pub fn delete_data_connector(state: State<'_, AppState>, id: String) -> CmdResult<Vec<SavedConnector>> {
    let mut book = state.0.lock().map_err(|_| "내부 상태를 잠글 수 없습니다".to_string())?;
    let before = book.connectors.len();
    book.connectors.retain(|item| item.id != id);
    if before != book.connectors.len() {
        // Removing a connector does not delete the imported data sheet.
        book.dirty = true;
        book.edits = book.edits.saturating_add(1);
    }
    Ok(book.connectors.clone())
}
