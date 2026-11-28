//! Tauri commands for reusable, local-only file connectors.
use std::io::Read;
use std::path::Path;
use std::time::UNIX_EPOCH;

use cellx_core::data::TableInfo;
use cellx_core::project::SavedConnector;
use sha2::{Digest, Sha256};
use tauri::State;

use crate::commands::AppState;

type CmdResult<T> = Result<T, String>;

/// Content-based fingerprint. Size/mtime alone misses same-size replacements and
/// files whose timestamps are preserved by copy/sync tools.
fn file_fingerprint(path: &Path) -> CmdResult<String> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| format!("로컬 파일을 열 수 없습니다: {e}"))?;
    let before = file.metadata()
        .map_err(|e| format!("로컬 파일 정보를 읽을 수 없습니다: {e}"))?;
    let modified = before.modified().ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos().to_string())
        .unwrap_or_else(|| "unknown".into());

    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)
            .map_err(|e| format!("로컬 파일을 읽을 수 없습니다: {e}"))?;
        if read == 0 { break; }
        hasher.update(&buffer[..read]);
    }

    // Refuse a digest if the source changed while it was being read.
    let after = file.metadata()
        .map_err(|e| format!("로컬 파일 정보를 다시 읽을 수 없습니다: {e}"))?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err("파일을 읽는 동안 원본 파일이 변경되었습니다. 다시 시도해 주세요.".into());
    }

    let digest = hasher.finalize().iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    // Legacy size:mtime fingerprints safely appear changed and are refreshed once.
    Ok(format!("sha256:{}:{modified}:{digest}", before.len()))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConnectorStatus {
    pub id: String,
    pub available: bool,
    /// True when the source content differs from the last successful import.
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
    let schema = book.wb.data().inspect_file_schema(Path::new(&connector.path))
        .map_err(|e| e.to_string())?;
    connector.schema = schema.iter().map(|column| (column.name.clone(), column.sql_type.clone())).collect();
    // Store the pre-import digest. If the file changes during import, the next
    // status poll detects the mismatch instead of silently blessing new content.
    connector.fingerprint = file_fingerprint(Path::new(&connector.path))?;
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
    let schema = book.wb.data().inspect_file_schema(Path::new(&connector.path))
        .map_err(|e| e.to_string())?;
    let current_schema = schema.iter().map(|column| (column.name.clone(), column.sql_type.clone())).collect::<Vec<_>>();
    if !connector.schema.is_empty() && connector.schema != current_schema && !accept_schema_change {
        return Err(format!(
            "SCHEMA_DRIFT: 원본 파일의 열 구조가 변경되었습니다. 기존 {}개 열, 현재 {}개 열입니다. 구조 변경을 수락하고 새로 고치시겠습니까?",
            connector.schema.len(), current_schema.len()
        ));
    }
    let fingerprint = file_fingerprint(Path::new(&connector.path))?;
    let table = book.wb.import_data_with_locale(
        Path::new(&connector.path),
        &connector.table_name,
        &connector.locale,
    ).map_err(|e| e.to_string())?;
    connector.schema = current_schema;
    connector.fingerprint = fingerprint;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_detects_same_size_content_replacement() {
        let path = std::env::temp_dir().join(format!("cellx-fingerprint-{}.csv", std::process::id()));
        std::fs::write(&path, b"alpha,1\n").unwrap();
        let first = file_fingerprint(&path).unwrap();
        std::fs::write(&path, b"bravo,2\n").unwrap();
        let second = file_fingerprint(&path).unwrap();
        assert_ne!(first, second, "same-size source replacement must be detected");
        assert!(second.starts_with("sha256:"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn fingerprint_is_stable_for_unchanged_content() {
        let path = std::env::temp_dir().join(format!("cellx-fingerprint-stable-{}.csv", std::process::id()));
        std::fs::write(&path, b"alpha,1\n").unwrap();
        let first = file_fingerprint(&path).unwrap();
        let second = file_fingerprint(&path).unwrap();
        assert_eq!(first, second);
        let _ = std::fs::remove_file(path);
    }
}
