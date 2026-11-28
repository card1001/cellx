//! Progress and cancel for long data-sheet commands (import, export,
//! query-to-table).
//!
//! The command itself still runs with the workbook locked; what lives here is
//! kept outside that lock, so the UI can poll `data_job_progress` and call
//! `cancel_data_job` while the command runs.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use cellx_core::job::{JobMonitor, JobProgress};
use serde::Serialize;
use tauri::State;

use crate::commands::{AppState, CmdResult};

struct Running {
    kind: &'static str,
    label: String,
    started: Instant,
    monitor: Arc<JobMonitor>,
}

#[derive(Default)]
pub struct DataJobState(Mutex<Option<Running>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataJobStatus {
    /// `import`, `export` or `query`.
    kind: &'static str,
    /// File or data-sheet name, for the progress window.
    label: String,
    elapsed_ms: u64,
    progress: JobProgress,
}

/// Runs `job` with the workbook locked and registered as the running data
/// job, so that progress can be read and the job cancelled meanwhile.
pub(crate) fn run<T>(
    state: &State<'_, AppState>,
    jobs: &State<'_, DataJobState>,
    kind: &'static str,
    label: String,
    mark_dirty: bool,
    job: impl FnOnce(&mut cellx_core::Workbook) -> cellx_core::Result<T>,
) -> CmdResult<T> {
    let mut book = crate::commands::lock(state)?;
    let monitor = book.wb.data().monitor();
    set(jobs, Some(Running { kind, label, started: Instant::now(), monitor }));
    let result = job(&mut book.wb);
    set(jobs, None);
    let out = result.map_err(crate::commands::err)?;
    if mark_dirty {
        book.dirty = true;
        book.edits += 1;
    }
    Ok(out)
}

fn set(jobs: &State<'_, DataJobState>, value: Option<Running>) {
    if let Ok(mut slot) = jobs.0.lock() {
        *slot = value;
    }
}

/// The running data job, if any. Never waits for the workbook.
#[tauri::command]
pub fn data_job_progress(jobs: State<'_, DataJobState>) -> Option<DataJobStatus> {
    let slot = jobs.0.lock().ok()?;
    slot.as_ref().map(|running| DataJobStatus {
        kind: running.kind,
        label: running.label.clone(),
        elapsed_ms: running.started.elapsed().as_millis() as u64,
        progress: running.monitor.snapshot(),
    })
}

/// Asks the running data job to stop; the command then fails with a
/// "cancelled" error and leaves the data sheets as they were.
#[tauri::command]
pub fn cancel_data_job(jobs: State<'_, DataJobState>) -> bool {
    match jobs.0.lock() {
        Ok(slot) => match slot.as_ref() {
            Some(running) => {
                running.monitor.cancel();
                true
            }
            None => false,
        },
        Err(_) => false,
    }
}
