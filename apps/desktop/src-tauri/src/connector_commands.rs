//! Tauri commands for reusable, local-only file connectors.
use std::path::Path;
use std::time::UNIX_EPOCH;

use cellx_core::data::TableInfo;
use cellx_core::project::SavedConnector;
use tauri::State;

use crate::commands::AppState;

type CmdResult<T> = Result<T, String>;

fn file_fingerprint(path: &Path) -> CmdResult<String> {
    let metadata = std::fs::metadata(path).map_err(|e| format!("로컬 파일 정보를 읽을 수 없습니다: {e}"))?;
    let modified = metadata.modified().ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos().to_string())
        .unwrap_or_else(|| "unknown".into());
    Ok(format!("{}:{modified}", metadata.len()))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConnectorStatus {
    pub id: String,
    pub available: bool,
    /// True when file size or modification time differs from the last successful import.
    pub changed: bool,
}

#[tauri::command]
pub fn data_connector_statuses(state: State<'_, AppState>) -> CmdResult<Vec<ConnectorStatus>> {
    let book = state.0.lock().map_err(|_| "내부 상태를 잠글 수 없습니다".to_string())?;
    Ok(book.connectors.iter().map(|connector| {
        match file_fingerprint(Path::new(&connector.path)) {
            Ok(current) => ConnectorStatus {
                id: connector.id.clone(),
                available: true,
                changed: connector.fingerprint.is_empty() || connector.fingerprint != current,
            },
            Err(_) => ConnectorStatus {
                id: connector.id.clone(),
                available: false,
                changed: true,
            },
        }
    }).collect())
}

#[tauri::command]
pub fn data_connectors(state: State<'_, AppState>) -> CmdResult<Vec<SavedConnector>> {
    let book = state.0.lock().map_err(|_| "내부 상태를 잠글 수 없습니다".to_string())?;
    Ok(book.connectors.clone())
}

#[tauri::command]
pub fn save_data_connector(state: State<'_, AppState>, mut connector: SavedConnector) -> CmdResult<TableInfo> {
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
    // Inspect the source before import and persist a baseline for future drift checks.
    let schema = book.wb.data().inspect_file_schema(Path::new(&connector.path), &connector.locale)
        .map_err(|e| e.to_string())?;
    connector.schema = schema.iter().map(|column| (column.name.clone(), column.sql_type.clone())).collect();
    connector.fingerprint = file_fingerprint(Path::new(&connector.path))?;
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
pub fn refresh_data_connector(state: State<'_, AppState>, id: String, accept_schema_change: bool) -> CmdResult<TableInfo> {
    let mut book = state.0.lock().map_err(|_| "내부 상태를 잠글 수 없습니다".to_string())?;
    let mut connector = book.connectors.iter().find(|item| item.id == id)
        .ok_or_else(|| "저장된 데이터 연결을 찾을 수 없습니다".to_string())?
        .clone();
    let schema = book.wb.data().inspect_file_schema(Path::new(&connector.path), &connector.locale)
        .map_err(|e| e.to_string())?;
    let current_schema = schema.iter().map(|column| (column.name.clone(), column.sql_type.clone())).collect::<Vec<_>>();
    if !connector.schema.is_empty() && connector.schema != current_schema && !accept_schema_change {
        return Err(format!(
            "SCHEMA_DRIFT: 원본 파일의 열 구조가 변경되었습니다. 기존 {}개 열, 현재 {}개 열입니다. 구조 변경을 수락하고 새로 고치시겠습니까?",
            connector.schema.len(), current_schema.len()
        ));
    }
    connector.schema = current_schema;
    connector.fingerprint = file_fingerprint(Path::new(&connector.path))?;
    let table = book.wb.import_data_with_locale(
        Path::new(&connector.path),
        &connector.table_name,
        &connector.locale,
    ).map_err(|e| e.to_string())?;
    if let Some(saved) = book.connectors.iter_mut().find(|item| item.id == connector.id) {
        *saved = connector;
    }
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
