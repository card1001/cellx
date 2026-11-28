//! Progress and cancellation for long data-sheet jobs (import, export,
//! query-to-table).
//!
//! A [`JobMonitor`] belongs to one [`crate::data::DataStore`]. The job runs on
//! the thread that holds the workbook; the UI reads [`JobMonitor::snapshot`]
//! and calls [`JobMonitor::cancel`] from another thread without taking the
//! workbook lock. Byte counts cover the steps CellX does itself (converting a
//! file's text encoding); DuckDB's own estimate covers the query that reads or
//! writes the file.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;

use duckdb::{InterruptHandle, ProgressHandle};
use serde::Serialize;

use crate::error::{CellxError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobPhase {
    Idle,
    /// Converting text between a legacy encoding and UTF-8.
    Converting,
    /// DuckDB is reading or writing the data.
    Querying,
    /// Updating schema information after the data is in place.
    Finishing,
}

impl JobPhase {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Converting,
            2 => Self::Querying,
            3 => Self::Finishing,
            _ => Self::Idle,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobProgress {
    pub phase: JobPhase,
    /// Fraction done in the current phase, 0..=1, when it can be estimated.
    pub fraction: Option<f64>,
    /// Work done in the converting phase, in `unit`s.
    pub bytes_done: u64,
    pub bytes_total: u64,
    /// `bytes` for text conversion, `rows` for reading a spreadsheet.
    pub unit: &'static str,
    pub rows_processed: u64,
    pub cancelled: bool,
}

pub struct JobMonitor {
    phase: AtomicU8,
    cancelled: AtomicBool,
    bytes_done: AtomicU64,
    bytes_total: AtomicU64,
    unit_rows: AtomicBool,
    interrupt: Arc<InterruptHandle>,
    progress: ProgressHandle,
}

impl JobMonitor {
    pub(crate) fn new(interrupt: Arc<InterruptHandle>, progress: ProgressHandle) -> Self {
        Self {
            phase: AtomicU8::new(0),
            cancelled: AtomicBool::new(false),
            bytes_done: AtomicU64::new(0),
            bytes_total: AtomicU64::new(0),
            unit_rows: AtomicBool::new(false),
            interrupt,
            progress,
        }
    }

    /// Starts a new job: clears an earlier cancel request and the counters.
    pub fn begin(&self) {
        self.cancelled.store(false, Ordering::Release);
        self.bytes_done.store(0, Ordering::Relaxed);
        self.bytes_total.store(0, Ordering::Relaxed);
        self.set_phase(JobPhase::Idle);
    }

    pub fn end(&self) {
        self.set_phase(JobPhase::Idle);
    }

    pub fn set_phase(&self, phase: JobPhase) {
        self.phase.store(phase as u8, Ordering::Release);
    }

    pub(crate) fn set_bytes(&self, done: u64, total: u64) {
        self.unit_rows.store(false, Ordering::Relaxed);
        self.bytes_total.store(total, Ordering::Relaxed);
        self.bytes_done.store(done, Ordering::Relaxed);
    }

    pub(crate) fn set_rows(&self, done: u64, total: u64) {
        self.unit_rows.store(true, Ordering::Relaxed);
        self.bytes_total.store(total, Ordering::Relaxed);
        self.bytes_done.store(done, Ordering::Relaxed);
    }

    /// Asks the running job to stop. A DuckDB query is interrupted at once;
    /// CellX's own loops stop at their next check (at most a megabyte later).
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.interrupt.interrupt();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Returns `Err(Cancelled)` once a cancel was requested.
    pub(crate) fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(CellxError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Turns the error of an interrupted query into `Cancelled`.
    pub(crate) fn map_err<T>(&self, result: Result<T>) -> Result<T> {
        match result {
            Err(_) if self.is_cancelled() => Err(CellxError::Cancelled),
            other => other,
        }
    }

    pub fn snapshot(&self) -> JobProgress {
        let phase = JobPhase::from_u8(self.phase.load(Ordering::Acquire));
        let bytes_done = self.bytes_done.load(Ordering::Relaxed);
        let bytes_total = self.bytes_total.load(Ordering::Relaxed);
        let mut rows_processed = 0;
        let fraction = match phase {
            JobPhase::Converting if bytes_total > 0 => Some((bytes_done as f64 / bytes_total as f64).min(1.0)),
            JobPhase::Querying => {
                let query = self.progress.query_progress();
                rows_processed = query.rows_processed();
                query.percentage().map(|p| (p / 100.0).clamp(0.0, 1.0))
            }
            _ => None,
        };
        JobProgress {
            phase,
            fraction,
            bytes_done,
            bytes_total,
            unit: if self.unit_rows.load(Ordering::Relaxed) { "rows" } else { "bytes" },
            rows_processed,
            cancelled: self.is_cancelled(),
        }
    }
}
