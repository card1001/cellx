//! Background execution and cancellation for saved data pipelines.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use cellx_core::data::{TableInfo};
use serde::Serialize;
use tauri::State;

use crate::commands::AppState;

static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize)]
pub struct TransformJobSnapshot {
    pub id: String,
    pub status: String,
    pub phase: String,
    pub result: Option<TableInfo>,
    pub error: Option<String>,
}

pub(crate) struct JobControl {
    snapshot: TransformJobSnapshot,
    cancelled: Arc<AtomicBool>,
    interrupt: Arc<dyn Fn() + Send + Sync>,
}

#[derive(Default)]
pub struct JobState(pub Arc<Mutex<HashMap<String, JobControl>>>);

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
) -> Result<TransformJobSnapshot, String> {
    let (pipeline, interrupt) = {
        let mut book = state.0.lock().map_err(|_| "통합 문서 상태를 잠글 수 없습니다".to_string())?;
        let saved = book.pipelines.iter().find(|item| item.id == pipeline_id)
            .ok_or_else(|| "저장된 파이프라인을 찾을 수 없습니다".to_string())?;
        let pipeline = saved.pipeline.clone();
        book.wb.data().plan_transform(&pipeline).map_err(|e| e.to_string())?;
        let handle = book.wb.data().interrupt_handle();
        let interrupt: Arc<dyn Fn() + Send + Sync> = Arc::new(move || handle.interrupt());
        (pipeline, interrupt)
    };

    let sequence = NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)
        .unwrap_or_default().as_millis();
    let id = format!("transform-{timestamp}-{sequence}");
    let cancelled = Arc::new(AtomicBool::new(false));
    let snapshot = TransformJobSnapshot {
        id: id.clone(),
        status: "queued".into(),
        phase: "대기 중".into(),
        result: None,
        error: None,
    };
    let jobs_map = jobs.0.clone();
    {
        let mut records = jobs_map.lock().map_err(|_| "작업 상태를 잠글 수 없습니다".to_string())?;
        // Bound in-memory job history for long-running desktop sessions.
        if records.len() >= 100 {
            // IDs begin with a millisecond timestamp, so lexical order is chronological.
            if let Some(oldest) = records.keys().min().cloned() {
                records.remove(&oldest);
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
                job.phase = "데이터 변환 실행 중".into();
            });

            let result = if cancelled.load(Ordering::Acquire) {
                Err("작업이 취소되었습니다".to_string())
            } else {
                match app_state.lock() {
                    Ok(mut book) => {
                        if cancelled.load(Ordering::Acquire) {
                            Err("작업이 취소되었습니다".to_string())
                        } else {
                            book.wb.data()
                                .execute_transform(&output_table, &pipeline)
                                .map_err(|e| e.to_string())
                                .map(|table| {
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
            });
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
    jobs: State<'_, JobState>,
) -> Result<Vec<TransformJobSnapshot>, String> {
    let records = jobs.0.lock().map_err(|_| "작업 상태를 잠글 수 없습니다".to_string())?;
    let mut snapshots = records.values().map(|record| record.snapshot.clone()).collect::<Vec<_>>();
    snapshots.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(snapshots)
}

#[tauri::command]
pub fn get_transform_job(
    jobs: State<'_, JobState>,
    id: String,
) -> Result<TransformJobSnapshot, String> {
    let records = jobs.0.lock().map_err(|_| "작업 상태를 잠글 수 없습니다".to_string())?;
    records.get(&id).map(|record| record.snapshot.clone())
        .ok_or_else(|| "작업을 찾을 수 없거나 작업 기록이 만료되었습니다".to_string())
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
