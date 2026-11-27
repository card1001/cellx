//! Tauri commands for saved data-transformation recipes.
use cellx_core::data::TableInfo;
use cellx_core::project::SavedPipeline;
use cellx_core::transform::{TransformPipeline, TransformValue};
use cellx_core::value::Scalar;
use serde::Serialize;
use tauri::State;

use crate::commands::AppState;

type CmdResult<T> = Result<T, String>;

#[derive(Debug, Clone, Serialize)]
pub struct TransformPreview {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Scalar>>,
    pub truncated: bool,
}

fn parameters(pipeline: &TransformPipeline, state: &mut crate::commands::Book) -> CmdResult<(String, Vec<duckdb::types::Value>)> {
    let plan = state.wb.data().plan_transform(pipeline).map_err(|e| e.to_string())?;
    let params = plan.parameters.iter().map(|value| match value {
        TransformValue::Text(value) => duckdb::types::Value::Text(value.clone()),
        TransformValue::Number(value) => duckdb::types::Value::Double(*value),
        TransformValue::Boolean(value) => duckdb::types::Value::Boolean(*value),
    }).collect();
    Ok((plan.sql, params))
}

#[tauri::command]
pub fn transform_pipelines(state: State<'_, AppState>) -> CmdResult<Vec<SavedPipeline>> {
    let book = state.0.lock().map_err(|_| "내부 상태를 잠글 수 없습니다".to_string())?;
    Ok(book.pipelines.clone())
}

#[tauri::command]
pub fn save_transform_pipeline(state: State<'_, AppState>, saved: SavedPipeline) -> CmdResult<Vec<SavedPipeline>> {
    if saved.id.trim().is_empty() || saved.id.len() > 80 {
        return Err("파이프라인 ID가 올바르지 않습니다".into());
    }
    if saved.name.trim().is_empty() || saved.name.len() > 120 {
        return Err("파이프라인 이름은 1~120자여야 합니다".into());
    }
    let mut book = state.0.lock().map_err(|_| "내부 상태를 잠글 수 없습니다".to_string())?;
    // Validate the complete recipe against the live source schema before persisting it.
    book.wb.data().plan_transform(&saved.pipeline).map_err(|e| e.to_string())?;
    if let Some(existing) = book.pipelines.iter_mut().find(|item| item.id == saved.id) {
        *existing = saved;
    } else {
        book.pipelines.push(saved);
    }
    book.pipelines.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    book.dirty = true;
    book.edits = book.edits.saturating_add(1);
    Ok(book.pipelines.clone())
}

#[tauri::command]
pub fn delete_transform_pipeline(state: State<'_, AppState>, id: String) -> CmdResult<Vec<SavedPipeline>> {
    let mut book = state.0.lock().map_err(|_| "내부 상태를 잠글 수 없습니다".to_string())?;
    let before = book.pipelines.len();
    book.pipelines.retain(|item| item.id != id);
    if before != book.pipelines.len() {
        book.dirty = true;
        book.edits = book.edits.saturating_add(1);
    }
    Ok(book.pipelines.clone())
}

#[tauri::command]
pub fn preview_transform(state: State<'_, AppState>, pipeline: TransformPipeline) -> CmdResult<TransformPreview> {
    let mut book = state.0.lock().map_err(|_| "내부 상태를 잠글 수 없습니다".to_string())?;
    let (sql, params) = parameters(&pipeline, &mut book)?;
    let preview_sql = format!("SELECT * FROM ({sql}) AS \"_cellx_preview\" LIMIT 101");
    let (mut columns, mut rows, _) = book.wb.data().rows(&preview_sql, &params, 101).map_err(|e| e.to_string())?;
    // _rid is an internal row identity and is never displayed as a user column.
    if columns.first().is_some_and(|name| name == "_rid") {
        columns.remove(0);
        for row in &mut rows {
            if !row.is_empty() { row.remove(0); }
        }
    }
    let truncated = rows.len() > 100;
    rows.truncate(100);
    Ok(TransformPreview { columns, rows, truncated })
}

#[tauri::command]
pub fn execute_transform_pipeline(state: State<'_, AppState>, id: String, output_table: String) -> CmdResult<TableInfo> {
    let mut book = state.0.lock().map_err(|_| "내부 상태를 잠글 수 없습니다".to_string())?;
    let saved = book.pipelines.iter().find(|item| item.id == id)
        .ok_or_else(|| "저장된 파이프라인을 찾을 수 없습니다".to_string())?;
    let pipeline = saved.pipeline.clone();
    let result = book.wb.data().execute_transform(&output_table, &pipeline).map_err(|e| e.to_string())?;
    book.dirty = true;
    book.edits = book.edits.saturating_add(1);
    Ok(result)
}
