//! Background execution and cancellation for saved data pipelines.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use cellx_core::data::TableInfo;
use cellx_core::project::SavedTransformRun;
use cellx_core::transform::TransformPipeline;
use serde::Serialize;
use tauri::State;

use crate::commands::AppState;

static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);
const MAX_ACTIVE_JOBS: usize = 8;
const MAX_RETAINED_JOBS: usize = 100;

#[derive(Debug, Clone, Serialize)]
pub struct TransformJobSnapshot {
    pub id: String,
    pub pipeline_id: String,
    pub output_table: String,
    pub incremental: bool,
    pub pipeline: Option<TransformPipeline>,
    pub started_at_ms: u64,
    pub finished_at_ms: Option<u64>,
    pub result_rows: Option<u64>,
    pub status: String,
    pub phase: String,
    pub result: Option<TableInfo>,
    pub error: Option<String>,
}

struct JobControl {
    snapshot: TransformJobSnapshot,
    cancelled: Arc<AtomicBool>,
    interrupt: Arc<dyn Fn() + Send + Sync>,
}

#[derive(Default)]
pub struct JobState(pub Arc<Mutex<HashMap<String, JobControl>>>);

fn is_active_status(status: &str) -> bool {
    matches!(status, "queued" | "running")
}

fn update_job(
    jobs: &Arc<Mutex<HashMap<String, JobControl>>>,
    id: &str,
    update: impl FnOnce(&mut TransformJobSnapshot),
) {
    if let Ok(mut records) = jobs.lock() {
        if let Some(record) = records.get_mut(id) {
            update(&mut record.snapshot);
        }
    }
}

#[tauri::command]
pub fn start_transform_job(
    state: State<'_, AppState>,
    jobs: State<'_, JobState>,
    pipeline_id: String,
    output_table: String,
    incremental: bool,
    pipeline_override: Option<TransformPipeline>,
) -> Result<TransformJobSnapshot, String> {
    let (pipeline, interrupt, checkpoint) = {
        let mut book = state.0.lock().map_err(|_| "통합 문서 상태를 잠글 수 없습니다".to_string())?;
        let saved = book.pipelines.iter().find(|item| item.id == pipeline_id);
        let pipeline = match pipeline_override {
            Some(override_pipeline) => override_pipeline,
            None => saved.map(|item| item.pipeline.clone())
                .ok_or_else(|| "저장된 파이프라인을 찾을 수 없습니다".to_string())?,
        };
        let checkpoint = if incremental && saved.is_some_and(|item|
            item.pipeline == pipeline && item.incremental_output_table.as_deref() == Some(output_table.as_str())
        ) {
            saved.and_then(|item| item.last_incremental_source_rid)
        } else {
            None
        };
        book.wb.data().plan_transform(&pipeline).map_err(|e| e.to_string())?;
        let handle = book.wb.data().interrupt_handle();
        let interrupt: Arc<dyn Fn() + Send + Sync> = Arc::new(move || handle.interrupt());
        (pipeline, interrupt, checkpoint)
    };

    let sequence = NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)
        .unwrap_or_default().as_millis();
    let id = format!("transform-{timestamp}-{sequence}");
    let cancelled = Arc::new(AtomicBool::new(false));
    let snapshot = TransformJobSnapshot {
        id: id.clone(),
        pipeline_id: pipeline_id.clone(),
        output_table: output_table.clone(),
        incremental,
        pipeline: Some(pipeline.clone()),
        started_at_ms: timestamp as u64,
        finished_at_ms: None,
        result_rows: None,
        status: "queued".into(),
        phase: "대기 중".into(),
        result: None,
        error: None,
    };
    let jobs_map = jobs.0.clone();
    {
        let mut records = jobs_map.lock().map_err(|_| "작업 상태를 잠글 수 없습니다".to_string())?;
        // Prevent duplicate writes to the same output and unbounded thread
        // creation. The workbook lock serializes DuckDB work, so this is a
        // bounded waiting set rather than parallel database execution.
        let active = records.values().filter(|record| is_active_status(&record.snapshot.status)).count();
        if active >= MAX_ACTIVE_JOBS {
            return Err(format!("대기 또는 실행 중인 작업은 최대 {}개까지 허용됩니다", MAX_ACTIVE_JOBS));
        }
        if records.values().any(|record|
            is_active_status(&record.snapshot.status)
                && record.snapshot.output_table.eq_ignore_ascii_case(&output_table)
        ) {
            return Err("같은 결과 테이블을 대상으로 실행 중인 작업이 있습니다. 기존 작업이 끝난 뒤 다시 시도해 주세요.".into());
        }

        // Keep active records. Evict the oldest terminal record only.
        if records.len() >= MAX_RETAINED_JOBS {
            let oldest_terminal = records.iter()
                .filter(|(_, record)| !is_active_status(&record.snapshot.status))
                .min_by_key(|(_, record)| record.snapshot.started_at_ms)
                .map(|(id, _)| id.clone());
            if let Some(oldest) = oldest_terminal {
                records.remove(&oldest);
            } else {
                return Err("작업 이력이 가득 찼습니다. 실행 중인 작업이 완료된 뒤 다시 시도해 주세요.".into());
            }
        }
        records.insert(id.clone(), JobControl {
            snapshot: snapshot.clone(),
            cancelled: cancelled.clone(),
            interrupt: interrupt.clone(),
        });
    }

    let app_state = state.0.clone();
    let worker_jobs = jobs_map.clone();
    let worker_id = id.clone();
    let spawn = std::thread::Builder::new()
        .name("cellx-transform".into())
        .spawn(move || {
            update_job(&worker_jobs, &worker_id, |job| {
                job.status = "running".into();
                job.phase = if incremental { "증분 데이터 처리 중".into() } else { "전체 데이터 변환 중".into() };
            });

            let result = if cancelled.load(Ordering::Acquire) {
                Err("작업이 취소되었습니다".to_string())
            } else {
                match app_state.lock() {
                    Ok(mut book) => {
                        if cancelled.load(Ordering::Acquire) {
                            Err("작업이 취소되었습니다".to_string())
                        } else {
                            let execution = if incremental {
                                book.wb.data().execute_transform_incremental(&output_table, &pipeline, checkpoint)
                            } else {
                                book.wb.data().execute_transform(&output_table, &pipeline)
                                    .and_then(|table| book.wb.data().max_row_id(&pipeline.source_table).map(|watermark| (table, watermark)))
                            };
                            execution.map_err(|e| e.to_string()).map(|(table, watermark)| {
                                if let Some(saved) = book.pipelines.iter_mut().find(|item| item.id == pipeline_id) {
                                    saved.incremental_output_table = Some(output_table.clone());
                                    saved.last_incremental_source_rid = Some(watermark);
                                }
                                book.dirty = true;
                                book.edits = book.edits.saturating_add(1);
                                table
                            })
                        }
                    }
                    Err(_) => Err("통합 문서 상태를 잠글 수 없습니다".to_string()),
                }
            };

            update_job(&worker_jobs, &worker_id, |job| {
                match result {
                    Ok(table) => {
                        // A late cancel request must not hide a result that already committed.
                        job.status = "completed".into();
                        job.phase = "완료".into();
                        job.result_rows = Some(table.rows);
                        job.result = Some(table);
                        job.error = None;
                    }
                    Err(_error) if cancelled.load(Ordering::Acquire) => {
                        job.status = "cancelled".into();
                        job.phase = "취소됨".into();
                        job.error = None;
                        job.result = None;
                    }
                    Err(error) => {
                        job.status = "failed".into();
                        job.phase = "실패".into();
                        job.error = Some(error);
                    }
                }
                job.finished_at_ms = Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64);
            });
            let completed = worker_jobs.lock().ok()
                .and_then(|records| records.get(&worker_id).map(|record| record.snapshot.clone()));
            if let Some(snapshot) = completed {
                if let Ok(mut book) = app_state.lock() {
                    let saved = SavedTransformRun {
                        id: snapshot.id.clone(),
                        pipeline_id: snapshot.pipeline_id.clone(),
                        output_table: snapshot.output_table.clone(),
                        pipeline: snapshot.pipeline.clone(),
                        status: snapshot.status.clone(),
                        phase: snapshot.phase.clone(),
                        incremental: snapshot.incremental,
                        result_rows: snapshot.result_rows,
                        error: snapshot.error.clone(),
                        started_at_ms: snapshot.started_at_ms,
                        finished_at_ms: Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64),
                    };
                    book.transform_runs.retain(|item| item.id != saved.id);
                    book.transform_runs.push(saved);
                    book.transform_runs.sort_by_key(|item| item.started_at_ms);
                    if book.transform_runs.len() > 100 {
                        let remove = book.transform_runs.len() - 100;
                        book.transform_runs.drain(0..remove);
                    }
                    book.dirty = true;
                    book.edits = book.edits.saturating_add(1);
                }
            }
        });

    if let Err(error) = spawn {
        update_job(&jobs_map, &id, |job| {
            job.status = "failed".into();
            job.phase = "실패".into();
            job.error = Some(format!("백그라운드 작업을 시작하지 못했습니다: {error}"));
        });
        return Err(format!("백그라운드 작업을 시작하지 못했습니다: {error}"));
    }

    Ok(snapshot)
}

#[tauri::command]
pub fn list_transform_jobs(
    state: State<'_, AppState>,
    jobs: State<'_, JobState>,
) -> Result<Vec<TransformJobSnapshot>, String> {
    let records = jobs.0.lock().map_err(|_| "작업 상태를 잠글 수 없습니다".to_string())?;
    let mut snapshots = records.values().map(|record| record.snapshot.clone()).collect::<Vec<_>>();
    drop(records);
    let book = state.0.lock().map_err(|_| "통합 문서 상태를 잠글 수 없습니다".to_string())?;
    for saved in &book.transform_runs {
        if !snapshots.iter().any(|item| item.id == saved.id) {
            snapshots.push(TransformJobSnapshot {
                id: saved.id.clone(),
                pipeline_id: saved.pipeline_id.clone(),
                output_table: saved.output_table.clone(),
                incremental: saved.incremental,
                pipeline: saved.pipeline.clone(),
                started_at_ms: saved.started_at_ms,
                finished_at_ms: saved.finished_at_ms,
                result_rows: saved.result_rows,
                status: saved.status.clone(),
                phase: saved.phase.clone(),
                result: None,
                error: saved.error.clone(),
            });
        }
    }
    snapshots.sort_by(|a, b| b.started_at_ms.cmp(&a.started_at_ms));
    snapshots.truncate(100);
    Ok(snapshots)
}

#[tauri::command]
pub fn get_transform_job(
    state: State<'_, AppState>,
    jobs: State<'_, JobState>,
    id: String,
) -> Result<TransformJobSnapshot, String> {
    let records = jobs.0.lock().map_err(|_| "작업 상태를 잠글 수 없습니다".to_string())?;
    if let Some(record) = records.get(&id) {
        return Ok(record.snapshot.clone());
    }
    drop(records);
    let book = state.0.lock().map_err(|_| "통합 문서 상태를 잠글 수 없습니다".to_string())?;
    book.transform_runs.iter().find(|item| item.id == id).map(|saved| TransformJobSnapshot {
        id: saved.id.clone(),
        pipeline_id: saved.pipeline_id.clone(),
        output_table: saved.output_table.clone(),
        incremental: saved.incremental,
        pipeline: saved.pipeline.clone(),
        started_at_ms: saved.started_at_ms,
        finished_at_ms: saved.finished_at_ms,
        result_rows: saved.result_rows,
        status: saved.status.clone(),
        phase: saved.phase.clone(),
        result: None,
        error: saved.error.clone(),
    }).ok_or_else(|| "작업을 찾을 수 없거나 작업 기록이 만료되었습니다".to_string())
}

#[tauri::command]
pub fn cancel_transform_job(
    jobs: State<'_, JobState>,
    id: String,
) -> Result<TransformJobSnapshot, String> {
    let mut records = jobs.0.lock().map_err(|_| "작업 상태를 잠글 수 없습니다".to_string())?;
    let record = records.get_mut(&id)
        .ok_or_else(|| "작업을 찾을 수 없거나 작업 기록이 만료되었습니다".to_string())?;
    if record.snapshot.status == "queued" || record.snapshot.status == "running" {
        record.cancelled.store(true, Ordering::Release);
        record.snapshot.status = "cancelling".into();
        record.snapshot.phase = "취소 요청됨".into();
        (record.interrupt)();
    }
    Ok(record.snapshot.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_jobs_are_not_treated_as_active() {
        assert!(is_active_status("queued"));
        assert!(is_active_status("running"));
        assert!(!is_active_status("completed"));
        assert!(!is_active_status("failed"));
        assert!(!is_active_status("cancelled"));
    }
}
