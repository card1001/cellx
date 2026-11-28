//! Data engine: large tables ("data sheets") stored in DuckDB.
//!
//! Every imported table gets a stable `_rid` column so that rows keep their
//! identity after sorting or filtering.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use duckdb::{params_from_iter, Connection, InterruptHandle};
use serde::Serialize;

use crate::error::{CellxError, Result};
use crate::value::{from_duckdb, Scalar};
use crate::job::{JobMonitor, JobPhase};
use crate::transform::{TransformPipeline, TransformPlan, TransformValue};

mod spreadsheet;
pub use spreadsheet::{is_spreadsheet, spreadsheet_sheets, SPREADSHEET_EXTENSIONS};

/// Column kind as the bridge needs it to translate Excel criteria.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ColumnKind {
    Number,
    Text,
    Bool,
    Date,
    Timestamp,
    Other,
}

impl ColumnKind {
    fn from_duckdb_type(t: &str) -> Self {
        let t = t.to_ascii_uppercase();
        if t == "BOOLEAN" {
            ColumnKind::Bool
        } else if t == "DATE" {
            ColumnKind::Date
        } else if t.starts_with("TIMESTAMP") {
            ColumnKind::Timestamp
        } else if t.starts_with("DECIMAL")
            || matches!(
                t.as_str(),
                "TINYINT" | "SMALLINT" | "INTEGER" | "BIGINT" | "HUGEINT" | "UTINYINT"
                    | "USMALLINT" | "UINTEGER" | "UBIGINT" | "UHUGEINT" | "FLOAT" | "DOUBLE"
            )
        {
            ColumnKind::Number
        } else if t == "VARCHAR" || t.starts_with("VARCHAR") || t == "UUID" {
            ColumnKind::Text
        } else {
            ColumnKind::Other
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ColumnInfo {
    pub name: String,
    pub sql_type: String,
    pub kind: ColumnKind,
}

#[derive(Debug, Clone, Serialize)]
pub struct TableInfo {
    pub name: String,
    pub rows: u64,
    pub columns: Vec<ColumnInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ColumnProfile {
    pub name: String,
    pub sql_type: String,
    pub null_count: u64,
    pub empty_count: u64,
    pub distinct_count: u64,
    pub min: Option<Scalar>,
    pub max: Option<Scalar>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TableProfile {
    pub table: String,
    pub row_count: u64,
    pub columns: Vec<ColumnProfile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Page {
    pub columns: Vec<String>,
    /// Row ids (`_rid`) of the returned rows, for editing and selection.
    pub row_ids: Vec<i64>,
    pub rows: Vec<Vec<Scalar>>,
    /// Number of rows matching the filter (not just this page).
    pub total: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Sort {
    pub column: String,
    pub descending: bool,
}

/// A simple filter entered in the column header UI: an Excel-style criterion
/// such as `">=100"`, `"서울"` or `"*카드*"` applied to one column.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ColumnFilter {
    pub column: String,
    pub criterion: String,
}

/// Hard cap for one grid request; callers cannot force unbounded result allocation.
const MAX_PAGE_SIZE: u64 = 5_000;

pub struct DataStore {
    conn: Connection,
    schema_cache: HashMap<String, TableInfo>,
    /// Bumped on every change to a table; lets the bridge invalidate cached results.
    versions: HashMap<String, u64>,
    /// Monotonic generation across table creation, replacement and deletion.
    /// Prevents a dropped/recreated table from reusing an old cache version.
    version_clock: u64,
    /// Progress and cancellation of the long job running on this store.
    monitor: Arc<JobMonitor>,
}

pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Table names must be usable inside Excel structured references (`sales[amount]`).
pub fn is_valid_table_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {}
        _ => return false,
    }
    name.len() <= 64 && chars.all(|c| c.is_alphanumeric() || c == '_')
}

impl DataStore {
    /// Opens (or creates) a project database file.
    pub fn open(path: &Path) -> Result<Self> {
        Self::configure(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::configure(Connection::open_in_memory()?)
    }

    fn configure(conn: Connection) -> Result<Self> {
        // No silent downloads: CellX must work offline and never fetch code.
        conn.execute_batch(
            "SET autoinstall_known_extensions = false;
             SET autoload_known_extensions = false;",
        )?;
        // Lets the UI show how far an import or export is. Failing to turn it
        // on only loses the estimate.
        let _ = conn.set_query_progress_tracking(true);
        let monitor = Arc::new(JobMonitor::new(conn.interrupt_handle(), conn.progress_handle()));
        let mut store = DataStore {
            monitor,
            conn,
            schema_cache: HashMap::new(),
            versions: HashMap::new(),
            version_clock: 0,
        };
        for name in store.table_names()? {
            store.version_clock = store.version_clock.saturating_add(1);
            store.versions.insert(name, store.version_clock);
        }
        Ok(store)
    }

    pub fn duckdb_version(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row("SELECT version()", [], |r| r.get::<_, String>(0))?)
    }

    /// Progress and cancellation for the long job running on this store; safe
    /// to read and cancel from another thread while the job holds the store.
    pub fn monitor(&self) -> Arc<JobMonitor> {
        self.monitor.clone()
    }

    /// Runs `job` as one monitored job: an earlier cancel is cleared first,
    /// and an interrupted query comes back as `Cancelled`.
    fn monitored<T>(&mut self, job: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let monitor = self.monitor.clone();
        monitor.begin();
        let result = job(self);
        monitor.end();
        monitor.map_err(result)
    }

    pub fn interrupt_handle(&self) -> Arc<InterruptHandle> {
        self.conn.interrupt_handle()
    }

    pub fn table_names(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT table_name FROM information_schema.tables
             WHERE table_schema = 'main' AND table_name NOT LIKE '\\_cellx\\_%' ESCAPE '\\'
             ORDER BY table_name",
        )?;
        let names = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(names)
    }

    pub fn has_table(&self, name: &str) -> bool {
        self.versions
            .keys()
            .any(|t| t.eq_ignore_ascii_case(name))
    }

    /// Canonical spelling of a table name (structured references are case-insensitive).
    pub fn canonical_table(&self, name: &str) -> Option<String> {
        self.versions
            .keys()
            .find(|t| t.eq_ignore_ascii_case(name))
            .cloned()
    }

    pub fn version(&self, table: &str) -> u64 {
        self.versions.get(table).copied().unwrap_or(0)
    }

    /// Imports a CSV, TSV or Parquet file.
    pub fn import_file(&mut self, path: &Path, table: &str) -> Result<TableInfo> {
        self.import_file_with_locale(path, table, "ko-KR")
    }

    /// Dates are detected from the file itself (ISO and the other common
    /// forms), not forced to the UI language, so a file imports the same way
    /// in every language. A CSV that is not UTF-8 or UTF-16 is read in the
    /// legacy encoding of `locale` (Korean: CP949/EUC-KR, Japanese:
    /// Shift_JIS, Simplified Chinese: GBK, Traditional Chinese: Big5,
    /// otherwise Windows-1252), the way Excel saves CSV files on Windows.
    pub fn import_file_with_locale(&mut self, path: &Path, table: &str, locale: &str) -> Result<TableInfo> {
        self.import_file_with_encoding(path, table, locale, None)
    }

    /// Imports with an explicit text encoding (`utf-8`, `utf-16`, `cp949`,
    /// `shift_jis`, `gbk`, `big5`, `windows-1252`), or `None`/`auto` to detect it.
    pub fn import_file_with_encoding(
        &mut self,
        path: &Path,
        table: &str,
        locale: &str,
        encoding: Option<&str>,
    ) -> Result<TableInfo> {
        self.monitored(|store| store.import_inner(path, table, locale, encoding))
    }

    fn import_inner(
        &mut self,
        path: &Path,
        table: &str,
        locale: &str,
        encoding: Option<&str>,
    ) -> Result<TableInfo> {
        if !is_valid_table_name(table) {
            return Err(CellxError::InvalidName(table.to_string()));
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext == "parquet" {
            return self.import_with_reader(path, table, "read_parquet(?)".to_string());
        }
        let csv_reader = |encoding: &str| {
            let delimiter = if ext == "tsv" { ", delim = '\\t'" } else { "" };
            format!(
                "read_csv(?, auto_detect = true, sample_size = 20480, ignore_errors = false, encoding = '{encoding}', decimal_separator = '.'{delimiter})"
            )
        };
        let chosen = match encoding.map(|e| e.trim().to_ascii_lowercase()) {
            None => None,
            Some(e) if e.is_empty() || e == "auto" => None,
            Some(e) => Some(TextEncoding::from_name(&e).ok_or_else(|| CellxError::CsvEncoding(e.clone()))?),
        };
        match chosen.unwrap_or_else(|| TextEncoding::Auto) {
            TextEncoding::Utf8 => return self.import_with_reader(path, table, csv_reader("utf-8")),
            TextEncoding::Utf16 => return self.import_with_reader(path, table, csv_reader("utf-16")),
            TextEncoding::Legacy(enc) => return self.import_transcoded(path, table, enc, csv_reader("utf-8")),
            TextEncoding::Auto => {}
        }

        let mut head = Vec::with_capacity(64 * 1024);
        File::open(path)?.take(64 * 1024).read_to_end(&mut head)?;
        match detect_encoding(&head, locale)? {
            TextEncoding::Utf16 => self.import_with_reader(path, table, csv_reader("utf-16")),
            TextEncoding::Legacy(enc) => self.import_transcoded(path, table, enc, csv_reader("utf-8")),
            _ => match self.import_with_reader(path, table, csv_reader("utf-8")) {
                // The first 64 KB were UTF-8 but a later part is not: guess from that part.
                Err(CellxError::CsvEncoding(_)) => {
                    let sample = non_utf8_sample(path)?;
                    let enc = match detect_encoding(&sample, locale)? {
                        TextEncoding::Legacy(enc) => enc,
                        _ => legacy_encoding(locale),
                    };
                    self.import_transcoded(path, table, enc, csv_reader("utf-8"))
                }
                other => other,
            },
        }
    }

    /// The first lines of a text file as they would be read with `encoding`
    /// (or the detected one), for the import dialog's preview.
    pub fn preview_file(path: &Path, locale: &str, encoding: Option<&str>, lines: usize) -> Result<FilePreview> {
        let mut head = Vec::with_capacity(64 * 1024);
        File::open(path)?.take(64 * 1024).read_to_end(&mut head)?;
        let detected = match encoding.map(|e| e.trim().to_ascii_lowercase()) {
            Some(e) if !e.is_empty() && e != "auto" => {
                TextEncoding::from_name(&e).ok_or_else(|| CellxError::CsvEncoding(e.clone()))?
            }
            _ => detect_encoding(&head, locale)?,
        };
        let text = match detected {
            TextEncoding::Utf16 => {
                let (enc, skip) = encoding_rs::Encoding::for_bom(&head).unwrap_or((encoding_rs::UTF_16LE, 0));
                enc.decode_without_bom_handling(&head[skip..]).0.into_owned()
            }
            TextEncoding::Legacy(enc) => enc.decode_without_bom_handling(&head).0.into_owned(),
            _ => {
                let body = head.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&head);
                String::from_utf8_lossy(body).into_owned()
            }
        };
        let mut out: Vec<String> = text.lines().take(lines + 1).map(str::to_string).collect();
        // The last line of the sample may be cut off.
        if out.len() > lines {
            out.truncate(lines);
        }
        Ok(FilePreview { encoding: detected.name().to_string(), lines: out })
    }

    fn import_with_reader(&mut self, path: &Path, table: &str, reader: String) -> Result<TableInfo> {
        let path_text = path
            .to_str()
            .ok_or_else(|| CellxError::InvalidName(path.display().to_string()))?;
        let sql = format!(
            "CREATE OR REPLACE TABLE {t} AS SELECT row_number() OVER () AS _rid, * FROM {reader}",
            t = quote_ident(table)
        );
        self.monitor.check()?;
        self.monitor.set_phase(JobPhase::Querying);
        if let Err(error) = self.conn.execute(&sql, [path_text]) {
            self.monitor.check()?;
            let diagnostic = error.to_string().to_ascii_lowercase();
            if diagnostic.contains("utf-8") || diagnostic.contains("utf8") || diagnostic.contains("unicode") {
                return Err(CellxError::CsvEncoding("non-UTF-8 input".into()));
            }
            return Err(error.into());
        }
        self.monitor.set_phase(JobPhase::Finishing);
        self.touch(table);
        self.table_info(table)
    }

    /// Inspects a local CSV, TSV, or Parquet file without importing or mutating a data sheet.
    /// The schema is read directly from DuckDB's file readers using a bound path parameter.
    /// The text encoding is detected as on import; a file in a legacy encoding is
    /// read through a converted sample of its first 8 MB.
    pub fn inspect_file_schema(&self, path: &Path, locale: &str) -> Result<Vec<ColumnInfo>> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let describe = |file: &Path, reader: String| -> Result<Vec<ColumnInfo>> {
            let path_text = file
                .to_str()
                .ok_or_else(|| CellxError::InvalidName(file.display().to_string()))?;
            let mut stmt = self.conn.prepare(&format!("DESCRIBE SELECT * FROM {reader}"))?;
            let columns = stmt
                .query_map([path_text], |row| {
                    let name: String = row.get(0)?;
                    let sql_type: String = row.get(1)?;
                    Ok(ColumnInfo { kind: ColumnKind::from_duckdb_type(&sql_type), name, sql_type })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(columns)
        };
        if ext == "parquet" {
            return describe(path, "read_parquet(?)".to_string());
        }
        let csv_reader = |encoding: &str| {
            let delimiter = if ext == "tsv" { ", delim = '\\t'" } else { "" };
            format!(
                "read_csv(?, auto_detect = true, sample_size = 20480, ignore_errors = false, encoding = '{encoding}', decimal_separator = '.'{delimiter})"
            )
        };
        let mut head = Vec::with_capacity(64 * 1024);
        File::open(path)?.take(64 * 1024).read_to_end(&mut head)?;
        match detect_encoding(&head, locale)? {
            TextEncoding::Utf16 => describe(path, csv_reader("utf-16")),
            TextEncoding::Legacy(enc) => {
                static INSPECT_ID: AtomicU64 = AtomicU64::new(1);
                let id = INSPECT_ID.fetch_add(1, Ordering::Relaxed);
                let temp = std::env::temp_dir().join(format!("cellx-inspect-{}-{id}.{ext}", std::process::id()));
                let result = (|| -> Result<Vec<ColumnInfo>> {
                    let mut sample = Vec::with_capacity(8 << 20);
                    File::open(path)?.take(8 << 20).read_to_end(&mut sample)?;
                    // Whole lines only.
                    if let Some(end) = sample.iter().rposition(|&b| b == b'\n') {
                        sample.truncate(end + 1);
                    }
                    let (text, _) = enc.decode_without_bom_handling(&sample);
                    fs::write(&temp, text.as_bytes())?;
                    describe(&temp, csv_reader("utf-8"))
                })();
                let _ = fs::remove_file(&temp);
                result
            }
            _ => describe(path, csv_reader("utf-8")),
        }
    }

    /// Converts a CSV from a legacy encoding to a temporary UTF-8 file, a
    /// megabyte at a time, and imports that. Bytes that are not valid in the
    /// encoding make it an encoding error rather than silent replacement.
    fn import_transcoded(
        &mut self,
        path: &Path,
        table: &str,
        encoding: &'static encoding_rs::Encoding,
        reader: String,
    ) -> Result<TableInfo> {
        static TRANSCODE_ID: AtomicU64 = AtomicU64::new(1);
        let id = TRANSCODE_ID.fetch_add(1, Ordering::Relaxed);
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("csv");
        let temp = std::env::temp_dir().join(format!("cellx-import-{}-{id}.{ext}", std::process::id()));
        let result = (|| -> Result<TableInfo> {
            transcode_to_utf8(path, &temp, encoding, &self.monitor)?;
            self.import_with_reader(&temp, table, reader)
        })();
        let _ = fs::remove_file(&temp);
        result
    }

    /// Creates a table from rows already in memory (used by tests and pasted data).
    pub fn create_table_from_sql(&mut self, table: &str, select_sql: &str) -> Result<TableInfo> {
        if !is_valid_table_name(table) {
            return Err(CellxError::InvalidName(table.to_string()));
        }
        let sql = format!(
            "CREATE OR REPLACE TABLE {t} AS SELECT row_number() OVER () AS _rid, * FROM ({select_sql})",
            t = quote_ident(table)
        );
        self.monitored(|store| {
            store.monitor.set_phase(JobPhase::Querying);
            store.conn.execute_batch(&sql)?;
            store.monitor.set_phase(JobPhase::Finishing);
            store.touch(table);
            store.table_info(table)
        })
    }

    pub fn drop_table(&mut self, table: &str) -> Result<()> {
        let canonical = self.canonical_table(table).unwrap_or_else(|| table.to_string());
        self.conn
            .execute_batch(&format!("DROP TABLE IF EXISTS {}", quote_ident(table)))?;
        self.versions.remove(&canonical);
        self.schema_cache.remove(&canonical);
        Ok(())
    }

    fn touch(&mut self, table: &str) {
        let canonical = self.canonical_table(table).unwrap_or_else(|| table.to_string());
        self.version_clock = self.version_clock.saturating_add(1);
        self.versions.insert(canonical.clone(), self.version_clock);
        self.schema_cache.remove(&canonical);
    }

    pub fn table_info(&mut self, table: &str) -> Result<TableInfo> {
        if let Some(info) = self.schema_cache.get(table) {
            return Ok(info.clone());
        }
        let mut stmt = self.conn.prepare(
            "SELECT column_name, data_type FROM information_schema.columns
             WHERE table_schema = 'main' AND table_name = ? ORDER BY ordinal_position",
        )?;
        let columns = stmt
            .query_map([table], |r| {
                let name: String = r.get(0)?;
                let sql_type: String = r.get(1)?;
                Ok(ColumnInfo {
                    kind: ColumnKind::from_duckdb_type(&sql_type),
                    name,
                    sql_type,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if columns.is_empty() {
            return Err(CellxError::UnknownTable(table.to_string()));
        }
        let rows: i64 = self.conn.query_row(
            &format!("SELECT count(*) FROM {}", quote_ident(table)),
            [],
            |r| r.get(0),
        )?;
        let info = TableInfo {
            name: table.to_string(),
            rows: rows as u64,
            columns: columns.into_iter().filter(|c| c.name != "_rid").collect(),
        };
        self.schema_cache.insert(table.to_string(), info.clone());
        Ok(info)
    }

    pub fn column(&mut self, table: &str, column: &str) -> Result<ColumnInfo> {
        let info = self.table_info(table)?;
        info.columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(column))
            .cloned()
            .ok_or_else(|| CellxError::UnknownColumn(format!("{table}[{column}]")))
    }

    /// Computes bounded, column-level data quality statistics without loading table rows.
    /// All identifiers come from the inspected schema and are quoted before SQL construction.
    pub fn profile(&mut self, table: &str) -> Result<TableProfile> {
        let info = self.table_info(table)?;
        let mut expressions = Vec::with_capacity(info.columns.len() * 5);
        for column in &info.columns {
            let ident = quote_ident(&column.name);
            let empty_expr = if column.kind == ColumnKind::Text {
                format!("count(*) FILTER (WHERE {ident} = '')")
            } else {
                "CAST(0 AS BIGINT)".to_string()
            };
            expressions.push(format!("count(*) FILTER (WHERE {ident} IS NULL)"));
            expressions.push(empty_expr);
            expressions.push(format!("count(DISTINCT {ident})"));
            expressions.push(format!("min({ident})"));
            expressions.push(format!("max({ident})"));
        }
        // One aggregate query avoids rescanning a large table once per column.
        let sql = format!(
            "SELECT {} FROM {}",
            expressions.join(", "),
            quote_ident(table)
        );
        let (_, rows, _) = self.rows(&sql, &[], 1)?;
        let values = rows.into_iter().next().ok_or_else(|| {
            CellxError::Bridge("데이터 프로파일을 계산하지 못했습니다".into())
        })?;
        let count = |value: &Scalar| match value {
            Scalar::Number(n) if n.is_finite() && *n >= 0.0 => *n as u64,
            _ => 0,
        };
        let mut columns = Vec::with_capacity(info.columns.len());
        for (index, column) in info.columns.iter().enumerate() {
            let offset = index * 5;
            columns.push(ColumnProfile {
                name: column.name.clone(),
                sql_type: column.sql_type.clone(),
                null_count: count(&values[offset]),
                empty_count: count(&values[offset + 1]),
                distinct_count: count(&values[offset + 2]),
                min: if values[offset + 3] == Scalar::Empty { None } else { Some(values[offset + 3].clone()) },
                max: if values[offset + 4] == Scalar::Empty { None } else { Some(values[offset + 4].clone()) },
            });
        }
        Ok(TableProfile { table: info.name, row_count: info.rows, columns })
    }


    /// Builds a validated transformation plan against the current source schema.
    ///
    /// Planning does not execute SQL or modify the source table. The caller can
    /// inspect the generated SQL and bound parameters before scheduling execution.
    pub fn plan_transform(&mut self, pipeline: &TransformPipeline) -> Result<TransformPlan> {
        let info = self.table_info(&pipeline.source_table)?;
        let columns = info.columns.iter().map(|column| column.name.clone()).collect::<Vec<_>>();
        let mut joined_schemas = HashMap::new();
        for table in pipeline.required_join_tables() {
            let joined = self.table_info(&table)?;
            joined_schemas.insert(
                joined.name,
                joined.columns.iter().map(|column| column.name.clone()).collect(),
            );
        }
        pipeline.compile_with_schemas(&columns, &joined_schemas)
    }

    /// Returns a bounded preview without materializing or modifying a table.
    pub fn preview_transform(
        &mut self,
        pipeline: &TransformPipeline,
        max_rows: usize,
    ) -> Result<(Vec<String>, Vec<Vec<Scalar>>, bool)> {
        let plan = self.plan_transform(pipeline)?;
        let parameters = plan.parameters.iter().map(|value| match value {
            TransformValue::Text(value) => duckdb::types::Value::Text(value.clone()),
            TransformValue::Number(value) => duckdb::types::Value::Double(*value),
            TransformValue::Boolean(value) => duckdb::types::Value::Boolean(*value),
        }).collect::<Vec<_>>();
        let visible_limit = max_rows.clamp(1, 500);
        let limit = visible_limit.saturating_add(1);
        let sql = format!(
            "SELECT * FROM ({}) AS {} LIMIT {}",
            plan.sql,
            quote_ident("_cellx_preview"),
            limit
        );
        let (mut columns, mut rows, engine_truncated) = self.rows(&sql, &parameters, limit)?;
        let truncated = engine_truncated || rows.len() > visible_limit;
        rows.truncate(visible_limit);
        // _rid is retained for stable internal ordering, but hidden in previews.
        if columns.first().is_some_and(|name| name == "_rid") {
            columns.remove(0);
            for row in &mut rows {
                if !row.is_empty() { row.remove(0); }
            }
        }
        Ok((columns, rows, truncated))
    }

    /// Executes a validated pipeline into a separate data sheet.
    ///
    /// The source is never modified. The output is replaced only after DuckDB
    /// successfully evaluates the parameterized CREATE TABLE AS statement.
    pub fn execute_transform(
        &mut self,
        output_table: &str,
        pipeline: &TransformPipeline,
    ) -> Result<TableInfo> {
        if !is_valid_table_name(output_table) {
            return Err(CellxError::InvalidName(output_table.to_string()));
        }
        if output_table.eq_ignore_ascii_case(&pipeline.source_table)
            || pipeline.required_join_tables().iter().any(|table| output_table.eq_ignore_ascii_case(table))
        {
            return Err(CellxError::Bridge(
                "변환 결과를 원본 또는 조인 입력 데이터 시트에 덮어쓸 수 없습니다".into(),
            ));
        }

        let plan = self.plan_transform(pipeline)?;
        let parameters = plan
            .parameters
            .iter()
            .map(|value| match value {
                TransformValue::Text(value) => duckdb::types::Value::Text(value.clone()),
                TransformValue::Number(value) => duckdb::types::Value::Double(*value),
                TransformValue::Boolean(value) => duckdb::types::Value::Boolean(*value),
            })
            .collect::<Vec<_>>();
        let sql = format!(
            "CREATE OR REPLACE TABLE {} AS {}",
            quote_ident(output_table),
            plan.sql
        );
        self.conn.execute(&sql, params_from_iter(parameters.iter()))?;
        self.touch(output_table);
        self.table_info(output_table)
    }

    /// Runs a query that returns one value.
    pub fn scalar(&self, sql: &str, params: &[duckdb::types::Value]) -> Result<Scalar> {
        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = stmt.query(params_from_iter(params.iter()))?;
        match rows.next()? {
            Some(row) => Ok(from_duckdb(row.get::<_, duckdb::types::Value>(0)?)),
            None => Ok(Scalar::Empty),
        }
    }

    /// Runs a query and returns column names plus up to `max_rows` rows.
    pub fn rows(
        &self,
        sql: &str,
        params: &[duckdb::types::Value],
        max_rows: usize,
    ) -> Result<(Vec<String>, Vec<Vec<Scalar>>, bool)> {
        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = stmt.query(params_from_iter(params.iter()))?;
        let names = rows
            .as_ref()
            .map(|s| s.column_names())
            .unwrap_or_default();
        let width = names.len();
        let mut out = Vec::new();
        let mut truncated = false;
        while let Some(row) = rows.next()? {
            if out.len() == max_rows {
                truncated = true;
                break;
            }
            let mut values = Vec::with_capacity(width);
            for i in 0..width {
                values.push(from_duckdb(row.get::<_, duckdb::types::Value>(i)?));
            }
            out.push(values);
        }
        Ok((names, out, truncated))
    }

    /// Rejects anything that is not a single read-only SELECT over CellX tables.
    ///
    /// DuckDB's own parser does the work: `json_serialize_sql` only accepts SELECT
    /// statements, and its output lets us refuse table functions such as
    /// `read_csv` or `glob`, which could read arbitrary files.
    pub fn validate_user_select(&self, sql: &str) -> Result<()> {
        let trimmed = sql.trim().trim_end_matches(';');
        if trimmed.contains(';') {
            return Err(CellxError::UnsafeSql("여러 문장은 실행할 수 없습니다".into()));
        }
        let json: String = self.conn.query_row(
            "SELECT json_serialize_sql(?::VARCHAR)",
            [trimmed],
            |r| r.get(0),
        )?;
        let parsed: serde_json::Value = serde_json::from_str(&json)
            .map_err(|e| CellxError::UnsafeSql(e.to_string()))?;
        if parsed.get("error").and_then(|v| v.as_bool()) == Some(true) {
            let message = parsed
                .get("error_message")
                .and_then(|v| v.as_str())
                .unwrap_or("SELECT 문만 실행할 수 있습니다");
            return Err(CellxError::UnsafeSql(message.to_string()));
        }
        if json_contains_table_function(&parsed) {
            return Err(CellxError::UnsafeSql(
                "파일을 읽는 테이블 함수(read_csv 등)는 사용할 수 없습니다".into(),
            ));
        }
        Ok(())
    }

    /// One page of a data sheet for the grid, with optional sort and filters.
    pub fn page(
        &mut self,
        table: &str,
        offset: u64,
        limit: u64,
        sort: Option<&Sort>,
        filters: &[ColumnFilter],
    ) -> Result<Page> {
        let info = self.table_info(table)?;
        let limit = limit.min(MAX_PAGE_SIZE);
        let mut where_sql = Vec::new();
        let mut params = Vec::new();
        for f in filters {
            if f.criterion.trim().is_empty() {
                continue;
            }
            let col = self.column(table, &f.column)?;
            let (sql, mut p) = crate::criteria::criterion_sql(
                &col,
                &Scalar::Text(f.criterion.clone()),
            );
            where_sql.push(sql);
            params.append(&mut p);
        }
        let where_clause = if where_sql.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_sql.join(" AND "))
        };
        let order = match sort {
            Some(s) => {
                let col = self.column(table, &s.column)?;
                format!(
                    "ORDER BY {} {} NULLS LAST, _rid",
                    quote_ident(&col.name),
                    if s.descending { "DESC" } else { "ASC" }
                )
            }
            None => "ORDER BY _rid".to_string(),
        };
        let cols: Vec<String> = info.columns.iter().map(|c| quote_ident(&c.name)).collect();
        let t = quote_ident(table);
        let total = match self.scalar(
            &format!("SELECT count(*) FROM {t} {where_clause}"),
            &params,
        )? {
            Scalar::Number(n) => n as u64,
            _ => 0,
        };
        let sql = format!(
            "SELECT _rid, {cols} FROM {t} {where_clause} {order} LIMIT {limit} OFFSET {offset}",
            cols = cols.join(", "),
        );
        let (_, rows, _) = self.rows(&sql, &params, limit as usize)?;
        let mut row_ids = Vec::with_capacity(rows.len());
        let mut data = Vec::with_capacity(rows.len());
        for mut r in rows {
            let rid = match r.remove(0) {
                Scalar::Number(n) => n as i64,
                _ => 0,
            };
            row_ids.push(rid);
            data.push(r);
        }
        Ok(Page {
            columns: info.columns.iter().map(|c| c.name.clone()).collect(),
            row_ids,
            rows: data,
            total,
        })
    }

    /// Edits one cell of a data sheet in place, keyed by `_rid`.
    pub fn update_cell(&mut self, table: &str, rid: i64, column: &str, text: &str) -> Result<()> {
        let col = self.column(table, column)?;
        // Refuse text that does not fit the column type instead of silently storing NULL.
        if !text.trim().is_empty() {
            let fits = self.scalar(
                &format!("SELECT TRY_CAST(? AS {}) IS NOT NULL", col.sql_type),
                &[duckdb::types::Value::Text(text.to_string())],
            )?;
            if fits != Scalar::Bool(true) {
                return Err(CellxError::Bridge(format!(
                    "'{text}'은(는) {} 열({})에 넣을 수 없습니다",
                    col.name, col.sql_type
                )));
            }
        }
        let sql = format!(
            "UPDATE {t} SET {c} = TRY_CAST(? AS {ty}) WHERE _rid = ?",
            t = quote_ident(table),
            c = quote_ident(&col.name),
            ty = col.sql_type
        );
        self.conn.execute(
            &sql,
            [
                duckdb::types::Value::Text(text.to_string()),
                duckdb::types::Value::BigInt(rid),
            ],
        )?;
        self.touch(table);
        Ok(())
    }

    /// Writes a data sheet to CSV or Parquet. CSV output is UTF-8 with BOM for desktop spreadsheet compatibility.
    pub fn export(&self, table: &str, path: &Path) -> Result<()> {
        self.export_with_locale(table, path, "ko-KR")
    }

    /// Dates are written as ISO (2025-05-04) whatever `_locale` is: a file
    /// exported in one language must import the same way in another.
    pub fn export_with_locale(&self, table: &str, path: &Path, _locale: &str) -> Result<()> {
        self.export_with_encoding(table, path, None)
    }

    /// CSV in UTF-8 with BOM (`None`, `utf-8`), or in a legacy encoding such
    /// as `cp949` for programs that cannot read UTF-8. A character the legacy
    /// encoding cannot hold is an error, not a silent `?`.
    pub fn export_with_encoding(&self, table: &str, path: &Path, encoding: Option<&str>) -> Result<()> {
        let monitor = self.monitor.clone();
        monitor.begin();
        let result = monitor.map_err(self.export_inner(table, path, encoding));
        monitor.end();
        result
    }

    fn export_inner(&self, table: &str, path: &Path, encoding: Option<&str>) -> Result<()> {
        let legacy = match encoding.map(|e| e.trim().to_ascii_lowercase()) {
            None => None,
            Some(e) if e.is_empty() || e == "auto" || e == "utf-8" || e == "utf8" => None,
            Some(e) => match TextEncoding::from_name(&e) {
                Some(TextEncoding::Legacy(enc)) => Some(enc),
                _ => return Err(CellxError::CsvEncoding(e)),
            },
        };
        let is_parquet = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("parquet"));
        if let (Some(enc), false) = (legacy, is_parquet) {
            static EXPORT_ID: AtomicU64 = AtomicU64::new(1);
            let id = EXPORT_ID.fetch_add(1, Ordering::Relaxed);
            let temp = std::env::temp_dir().join(format!("cellx-export-{}-{id}.csv", std::process::id()));
            // The destination is only opened once the UTF-8 copy is complete,
            // so a failure or cancel before that leaves an existing file alone.
            let mut writing = false;
            let result = (|| -> Result<()> {
                self.export_utf8(table, &temp, false)?;
                self.monitor.check()?;
                writing = true;
                transcode_from_utf8(&temp, path, enc, &self.monitor)
            })();
            let _ = fs::remove_file(&temp);
            if result.is_err() && writing {
                let _ = fs::remove_file(path);
            }
            return result;
        }
        self.export_utf8(table, path, true)
    }

    fn export_utf8(&self, table: &str, path: &Path, bom: bool) -> Result<()> {
        let path_text = path
            .to_str()
            .ok_or_else(|| CellxError::InvalidName(path.display().to_string()))?;
        let is_parquet = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("parquet"));
        self.monitor.set_phase(JobPhase::Querying);
        if is_parquet {
            let escaped = path_text.replace('\'', "''");
            if let Err(error) = self.conn.execute_batch(&format!(
                "COPY (SELECT * EXCLUDE (_rid) FROM {t} ORDER BY _rid) TO '{escaped}' (FORMAT parquet)",
                t = quote_ident(table)
            )) {
                let _ = fs::remove_file(path);
                return Err(error.into());
            }
            return Ok(());
        }

        // The BOM and the header go in DuckDB's PREFIX (it cannot be combined
        // with HEADER), so the file is written once, straight to its place.
        let names = self.export_column_names(table)?;
        let header = names.iter().map(|n| csv_field(n)).collect::<Vec<_>>().join(",");
        // DuckDB writes PREFIX and SUFFIX around the rows; with no rows the
        // file is just the header line, written here.
        let empty = self.conn.query_row(
            &format!("SELECT NOT EXISTS (SELECT 1 FROM {})", quote_ident(table)),
            [],
            |r| r.get::<_, bool>(0),
        )?;
        if empty {
            fs::write(path, format!("{}{header}\n", if bom { "\u{feff}" } else { "" }))?;
            return Ok(());
        }
        let bom = if bom { "\u{feff}" } else { "" };
        let prefix = format!("{bom}{header}\n").replace('\'', "''");
        let path_sql = path_text.replace('\'', "''");
        let sql = format!(
            "COPY (SELECT * EXCLUDE (_rid) FROM {t} ORDER BY _rid) TO '{path_sql}' (FORMAT csv, HEADER false, DELIMITER ',', DATEFORMAT '%Y-%m-%d', TIMESTAMPFORMAT '%Y-%m-%d %H:%M:%S', PREFIX '{prefix}', SUFFIX '\n')",
            t = quote_ident(table)
        );
        if let Err(error) = self.conn.execute_batch(&sql) {
            let _ = fs::remove_file(path);
            return Err(error.into());
        }
        Ok(())
    }

    fn export_column_names(&self, table: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(&format!("SELECT * EXCLUDE (_rid) FROM {} LIMIT 0", quote_ident(table)))?;
        stmt.execute([])?;
        Ok(stmt.column_names())
    }

    /// Runs SQL written by CellX itself (never user input).
    pub(crate) fn execute_internal(&self, sql: &str) -> Result<()> {
        self.conn.execute_batch(sql)?;
        Ok(())
    }

    pub fn checkpoint(&self) -> Result<()> {
        self.conn.execute_batch("CHECKPOINT")?;
        Ok(())
    }
}

/// A CSV field, quoted when it holds a comma, a quote or a line break.
fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

/// The text encoding of a CSV file.
#[derive(Clone, Copy, Debug)]
pub enum TextEncoding {
    Auto,
    Utf8,
    Utf16,
    Legacy(&'static encoding_rs::Encoding),
}

impl TextEncoding {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().replace('_', "-").as_str() {
            "auto" => Self::Auto,
            "utf-8" | "utf8" => Self::Utf8,
            "utf-16" | "utf16" => Self::Utf16,
            "cp949" | "euc-kr" | "uhc" | "ks-c-5601-1987" => Self::Legacy(encoding_rs::EUC_KR),
            "shift-jis" | "sjis" | "cp932" | "windows-31j" => Self::Legacy(encoding_rs::SHIFT_JIS),
            "gbk" | "gb2312" | "cp936" | "gb18030" => Self::Legacy(encoding_rs::GB18030),
            "big5" | "cp950" => Self::Legacy(encoding_rs::BIG5),
            "windows-1252" | "cp1252" | "latin1" | "iso-8859-1" => Self::Legacy(encoding_rs::WINDOWS_1252),
            _ => return None,
        })
    }

    /// The name the UI shows and passes back.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Utf8 => "utf-8",
            Self::Utf16 => "utf-16",
            Self::Legacy(e) if *e == encoding_rs::EUC_KR => "cp949",
            Self::Legacy(e) if *e == encoding_rs::SHIFT_JIS => "shift_jis",
            Self::Legacy(e) if *e == encoding_rs::GB18030 || *e == encoding_rs::GBK => "gbk",
            Self::Legacy(e) if *e == encoding_rs::BIG5 => "big5",
            Self::Legacy(_) => "windows-1252",
        }
    }
}

/// What the import dialog shows of a file before it is imported.
#[derive(Clone, Debug, Serialize)]
pub struct FilePreview {
    /// The encoding the lines were read with (see `TextEncoding::name`).
    pub encoding: String,
    pub lines: Vec<String>,
}

/// Guesses the encoding of the start of a file: a BOM, else UTF-8 when it is
/// valid, else the legacy encoding whose script best fits the text, the UI
/// language's own first on a tie.
pub fn detect_encoding(head: &[u8], locale: &str) -> Result<TextEncoding> {
    if head.starts_with(&[0xff, 0xfe, 0x00, 0x00]) || head.starts_with(&[0x00, 0x00, 0xfe, 0xff]) {
        return Err(CellxError::CsvEncoding("UTF-32".into()));
    }
    if head.starts_with(&[0xff, 0xfe]) || head.starts_with(&[0xfe, 0xff]) {
        return Ok(TextEncoding::Utf16);
    }
    if head.starts_with(&[0xef, 0xbb, 0xbf]) || is_utf8_prefix(head) {
        return Ok(TextEncoding::Utf8);
    }
    let mut candidates = vec![legacy_encoding(locale)];
    for e in [encoding_rs::EUC_KR, encoding_rs::SHIFT_JIS, encoding_rs::GBK, encoding_rs::BIG5, encoding_rs::WINDOWS_1252] {
        if !candidates.contains(&e) {
            candidates.push(e);
        }
    }
    // The first candidate (the UI language's, then Korean, Japanese and the
    // Chinese ones) that reads the text mostly as its own script wins: these
    // encodings overlap (Korean bytes are valid GBK), so a prior is needed.
    // Failing that, the best fit. The import dialog's preview shows the
    // result, and the encoding can be chosen there.
    let mut best: Option<(&'static encoding_rs::Encoding, f64)> = None;
    for enc in candidates {
        let mut decoder = enc.new_decoder_without_bom_handling();
        let mut text = String::with_capacity(head.len() * 3);
        let (result, _) = decoder.decode_to_string_without_replacement(head, &mut text, false);
        if matches!(result, encoding_rs::DecoderResult::Malformed(..)) {
            continue;
        }
        // Almost any bytes are valid Windows-1252, so it only wins when no
        // multibyte encoding reads the text as its own script.
        let weight = if enc == encoding_rs::WINDOWS_1252 { 0.5 } else { 1.0 };
        let score = script_fit(enc, &text) * weight;
        if score >= 0.8 {
            return Ok(TextEncoding::Legacy(enc));
        }
        if best.is_none_or(|(_, b)| score > b) {
            best = Some((enc, score));
        }
    }
    Ok(TextEncoding::Legacy(best.map_or(legacy_encoding(locale), |(e, _)| e)))
}

/// The share of the non-ASCII characters of `text` that belong to the
/// scripts the encoding is used for.
fn script_fit(enc: &'static encoding_rs::Encoding, text: &str) -> f64 {
    let fits = |c: char| -> bool {
        let u = c as u32;
        let cjk = (0x4E00..=0x9FFF).contains(&u) || (0x3000..=0x303F).contains(&u) || (0xFF00..=0xFFEF).contains(&u);
        if enc == encoding_rs::EUC_KR {
            // Korean text keeps to the 2,350 everyday syllables of KS X 1001
            // (both bytes 0xA1..); text in another encoding read as CP949
            // lands mostly in its rarely used extension.
            let common = || {
                let mut b = [0_u8; 4];
                let bytes = encoding_rs::EUC_KR.encode(c.encode_utf8(&mut b)).0;
                bytes.len() == 2 && bytes[0] >= 0xA1 && bytes[1] >= 0xA1
            };
            ((0xAC00..=0xD7A3).contains(&u) || (0x3130..=0x318F).contains(&u)) && common()
        } else if enc == encoding_rs::SHIFT_JIS {
            (0x3040..=0x30FF).contains(&u) || cjk
        } else if enc == encoding_rs::GBK || enc == encoding_rs::BIG5 || enc == encoding_rs::GB18030 {
            cjk
        } else {
            (0x00A0..=0x00FF).contains(&u) || (0x2010..=0x203A).contains(&u)
        }
    };
    let (mut all, mut good) = (0usize, 0usize);
    for c in text.chars().filter(|c| !c.is_ascii()) {
        all += 1;
        if fits(c) {
            good += 1;
        }
    }
    if all == 0 {
        0.0
    } else {
        good as f64 / all as f64
    }
}

/// 64 KB of a file from its first byte that is not UTF-8.
fn non_utf8_sample(path: &Path) -> Result<Vec<u8>> {
    let mut file = std::io::BufReader::with_capacity(1 << 20, File::open(path)?);
    let mut buf = vec![0_u8; 1 << 20];
    let mut carry: Vec<u8> = Vec::new();
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Ok(carry);
        }
        let mut chunk = std::mem::take(&mut carry);
        chunk.extend_from_slice(&buf[..n]);
        match std::str::from_utf8(&chunk) {
            Ok(_) => {}
            Err(e) if e.error_len().is_none() => carry = chunk[e.valid_up_to()..].to_vec(),
            Err(e) => {
                // Back up to the start of the line, so the sample starts cleanly.
                let at = chunk[..e.valid_up_to()].iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
                let mut sample = chunk[at..].to_vec();
                sample.truncate(64 * 1024);
                if sample.len() < 64 * 1024 {
                    let mut more = vec![0_u8; 64 * 1024 - sample.len()];
                    let m = file.read(&mut more)?;
                    sample.extend_from_slice(&more[..m]);
                }
                return Ok(sample);
            }
        }
    }
}

fn transcode_from_utf8(
    from: &Path,
    to: &Path,
    encoding: &'static encoding_rs::Encoding,
    monitor: &JobMonitor,
) -> Result<()> {
    let total = fs::metadata(from).map(|m| m.len()).unwrap_or(0);
    let mut done = 0_u64;
    monitor.set_bytes(0, total);
    monitor.set_phase(JobPhase::Converting);
    let mut input = std::io::BufReader::with_capacity(1 << 20, File::open(from)?);
    let mut output = std::io::BufWriter::with_capacity(1 << 20, File::create(to)?);
    let mut encoder = encoding.new_encoder();
    let mut inbuf = vec![0_u8; 1 << 20];
    let mut carry: Vec<u8> = Vec::new();
    let mut outbuf = vec![0_u8; 4 << 20];
    loop {
        monitor.check()?;
        let n = input.read(&mut inbuf)?;
        done += n as u64;
        monitor.set_bytes(done, total);
        let last = n == 0;
        let mut chunk = std::mem::take(&mut carry);
        chunk.extend_from_slice(&inbuf[..n]);
        // Keep a character cut off at the end of the chunk for the next one.
        let valid = match std::str::from_utf8(&chunk) {
            Ok(_) => chunk.len(),
            Err(e) if e.error_len().is_none() && !last => e.valid_up_to(),
            Err(_) => return Err(CellxError::CsvEncoding("UTF-8".into())),
        };
        carry = chunk[valid..].to_vec();
        let mut src = std::str::from_utf8(&chunk[..valid]).unwrap_or_default();
        loop {
            let (result, read, written) = encoder.encode_from_utf8_without_replacement(src, &mut outbuf, last);
            output.write_all(&outbuf[..written])?;
            src = &src[read..];
            match result {
                encoding_rs::EncoderResult::InputEmpty => break,
                encoding_rs::EncoderResult::OutputFull => {}
                encoding_rs::EncoderResult::Unmappable(c) => {
                    return Err(CellxError::TextEncoding(format!(
                        "'{c}' (U+{:04X}) 문자는 {}로 저장할 수 없습니다. UTF-8로 내보내세요",
                        c as u32,
                        TextEncoding::Legacy(encoding).name()
                    )));
                }
            }
        }
        if last {
            break;
        }
    }
    output.flush()?;
    Ok(())
}

/// The encoding Excel uses for CSV files on Windows in each UI language.
fn legacy_encoding(locale: &str) -> &'static encoding_rs::Encoding {
    let tag = locale.to_ascii_lowercase();
    if tag.starts_with("ko") {
        encoding_rs::EUC_KR // CP949 (Unified Hangul Code), a superset of EUC-KR
    } else if tag.starts_with("ja") {
        encoding_rs::SHIFT_JIS
    } else if tag == "zh-tw" || tag.starts_with("zh-hant") || tag == "zh-hk" {
        encoding_rs::BIG5
    } else if tag.starts_with("zh") {
        encoding_rs::GBK
    } else {
        encoding_rs::WINDOWS_1252
    }
}

/// Whether `bytes` is UTF-8, allowing a character cut off at the end.
fn is_utf8_prefix(bytes: &[u8]) -> bool {
    match std::str::from_utf8(bytes) {
        Ok(_) => true,
        Err(e) => e.error_len().is_none() && bytes.len() - e.valid_up_to() < 4,
    }
}

fn transcode_to_utf8(
    from: &Path,
    to: &Path,
    encoding: &'static encoding_rs::Encoding,
    monitor: &JobMonitor,
) -> Result<()> {
    let total = fs::metadata(from).map(|m| m.len()).unwrap_or(0);
    let mut done = 0_u64;
    monitor.set_bytes(0, total);
    monitor.set_phase(JobPhase::Converting);
    let mut input = std::io::BufReader::with_capacity(1 << 20, File::open(from)?);
    let mut output = std::io::BufWriter::with_capacity(1 << 20, File::create(to)?);
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let mut inbuf = vec![0_u8; 1 << 20];
    let mut outbuf = String::with_capacity(3 << 20);
    loop {
        monitor.check()?;
        let n = input.read(&mut inbuf)?;
        done += n as u64;
        monitor.set_bytes(done, total);
        let last = n == 0;
        let mut src = &inbuf[..n];
        loop {
            outbuf.clear();
            let (result, read) = decoder.decode_to_string_without_replacement(src, &mut outbuf, last);
            output.write_all(outbuf.as_bytes())?;
            src = &src[read..];
            match result {
                encoding_rs::DecoderResult::InputEmpty => break,
                encoding_rs::DecoderResult::OutputFull => {
                    outbuf.reserve(outbuf.capacity());
                }
                encoding_rs::DecoderResult::Malformed(_, _) => {
                    return Err(CellxError::TextEncoding(format!(
                        "{}로 읽을 수 없는 바이트가 있습니다. 가져오기 창에서 다른 인코딩을 고르세요",
                        TextEncoding::Legacy(encoding).name()
                    )));
                }
            }
        }
        if last {
            break;
        }
    }
    output.flush()?;
    Ok(())
}

fn json_contains_table_function(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Object(map) => {
            if map.get("type").and_then(|t| t.as_str()) == Some("TABLE_FUNCTION") {
                return true;
            }
            map.values().any(json_contains_table_function)
        }
        serde_json::Value::Array(items) => items.iter().any(json_contains_table_function),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cancels the store's job from another thread once it reaches `phase`,
    /// returning the largest progress fraction seen before that.
    fn cancel_when(monitor: Arc<JobMonitor>, phase: JobPhase) -> std::thread::JoinHandle<f64> {
        std::thread::spawn(move || {
            let mut best = 0.0_f64;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
            while std::time::Instant::now() < deadline {
                let snap = monitor.snapshot();
                if let Some(f) = snap.fraction {
                    best = best.max(f);
                }
                if snap.phase == phase {
                    monitor.cancel();
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            best
        })
    }

    #[test]
    fn a_running_query_can_be_cancelled_and_the_store_still_works() {
        let mut store = DataStore::open_in_memory().unwrap();
        let canceller = cancel_when(store.monitor(), JobPhase::Querying);
        let result = store.create_table_from_sql(
            "huge",
            "SELECT a.range * b.range AS x FROM range(200000) a, range(200000) b",
        );
        canceller.join().unwrap();
        assert!(matches!(result, Err(CellxError::Cancelled)), "{result:?}");
        assert!(!store.has_table("huge"));
        assert!(!store.monitor().snapshot().cancelled || store.monitor().snapshot().phase == JobPhase::Idle);
        // The next job starts clean.
        let info = store.create_table_from_sql("small", "SELECT 1 AS x").unwrap();
        assert_eq!(info.rows, 1);
    }

    #[test]
    fn converting_a_legacy_csv_reports_progress_and_can_be_cancelled() {
        let dir = std::env::temp_dir().join(format!("cellx-cancel-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.csv");
        let mut text = String::from("지역,금액\n");
        while text.len() < 24 << 20 {
            text.push_str("서울특별시,12345\n부산광역시,67890\n");
        }
        let (bytes, _, _) = encoding_rs::EUC_KR.encode(&text);
        fs::write(&path, &bytes).unwrap();
        drop(text);

        let mut store = DataStore::open_in_memory().unwrap();
        let monitor = store.monitor();
        // Cancel half-way through the conversion.
        let watcher = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
            let mut seen = 0.0_f64;
            while std::time::Instant::now() < deadline {
                let snap = monitor.snapshot();
                if snap.phase == JobPhase::Converting {
                    if let Some(f) = snap.fraction {
                        seen = seen.max(f);
                        if f >= 0.3 {
                            monitor.cancel();
                            break;
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_micros(200));
            }
            seen
        });
        let result = store.import_file_with_encoding(&path, "big", "ko-KR", Some("cp949"));
        let seen = watcher.join().unwrap();
        assert!(matches!(result, Err(CellxError::Cancelled)), "{result:?}");
        assert!((0.3..1.0).contains(&seen), "progress {seen}");
        assert!(!store.has_table("big"));

        let info = store.import_file_with_encoding(&path, "big", "ko-KR", Some("cp949")).unwrap();
        assert!(info.rows > 1000);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cancelled_export_leaves_an_existing_file_alone() {
        let store = store_with_sales();
        let path = std::env::temp_dir().join(format!("cellx-keep-{}.csv", std::process::id()));
        fs::write(&path, "keep me").unwrap();
        // A cancel requested before the export starts is cleared by it, so
        // simulate the request landing mid-job: the job checks it at its
        // first step and stops before opening the destination.
        let monitor = store.monitor();
        monitor.begin();
        monitor.cancel();
        let result = monitor.map_err(store.export_inner("sales", &path, Some("cp949")));
        assert!(matches!(result, Err(CellxError::Cancelled)), "{result:?}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep me");
        let _ = fs::remove_file(path);
    }

    /// `cargo test -p cellx-core --release -- --ignored --nocapture csv_import_progress`
    #[test]
    #[ignore]
    fn csv_import_progress_advances() {
        let path = std::env::temp_dir().join(format!("cellx-progress-{}.csv", std::process::id()));
        {
            let mut out = std::io::BufWriter::new(File::create(&path).unwrap());
            writeln!(out, "id,region,amount,day").unwrap();
            for i in 0..3_000_000u64 {
                writeln!(out, "{i},region{},{}.25,2025-01-{:02}", i % 17, i % 1000, 1 + i % 28).unwrap();
            }
        }
        let mut store = DataStore::open_in_memory().unwrap();
        let monitor = store.monitor();
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = done.clone();
        let watcher = std::thread::spawn(move || {
            let mut seen = Vec::new();
            while !flag.load(Ordering::Acquire) {
                let snap = monitor.snapshot();
                if let Some(f) = snap.fraction {
                    if seen.last() != Some(&(f * 100.0).round()) {
                        seen.push((f * 100.0).round());
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            seen
        });
        let start = std::time::Instant::now();
        let info = store.import_file(&path, "big").unwrap();
        done.store(true, Ordering::Release);
        let seen = watcher.join().unwrap();
        println!("rows {} in {:?}; progress samples {:?}", info.rows, start.elapsed(), seen);
        let _ = fs::remove_file(path);
    }

    fn store_with_sales() -> DataStore {
        let mut s = DataStore::open_in_memory().unwrap();
        s.create_table_from_sql(
            "sales",
            "SELECT * FROM (VALUES
                ('서울', 100.0::DOUBLE, DATE '2026-01-05', '정상'),
                ('부산', 250.0, DATE '2026-02-01', '취소'),
                ('서울', 300.0, DATE '2026-03-10', '정상'),
                ('대구', NULL,  DATE '2026-03-11', '정상')
             ) AS t(region, amount, order_date, status)",
        )
        .unwrap();
        s
    }

    #[test]
    fn import_assigns_row_ids_and_types() {
        let mut s = store_with_sales();
        let info = s.table_info("sales").unwrap();
        assert_eq!(info.rows, 4);
        assert_eq!(info.columns[1].kind, ColumnKind::Number);
        assert_eq!(info.columns[2].kind, ColumnKind::Date);
    }

    #[test]
    fn profile_reports_null_empty_distinct_and_range_statistics() {
        let mut store = store_with_sales();
        let profile = store.profile("sales").unwrap();
        assert_eq!(profile.row_count, 4);
        let amount = profile.columns.iter().find(|c| c.name == "amount").unwrap();
        assert_eq!(amount.null_count, 1);
        assert_eq!(amount.distinct_count, 3);
        assert_eq!(amount.min, Some(Scalar::Number(100.0)));
        assert_eq!(amount.max, Some(Scalar::Number(300.0)));
        let region = profile.columns.iter().find(|c| c.name == "region").unwrap();
        assert_eq!(region.distinct_count, 3);
        assert_eq!(region.empty_count, 0);
    }

    #[test]
    fn inspect_csv_schema_does_not_import_or_mutate_a_table() {
        let path = std::env::temp_dir().join(format!("cellx-inspect-schema-{}.csv", std::process::id()));
        fs::write(&path, "region,amount\nSeoul,12.5\nBusan,7.25\n").unwrap();
        let store = DataStore::open_in_memory().unwrap();
        let schema = store.inspect_file_schema(&path, "ko-KR").unwrap();
        assert_eq!(schema.iter().map(|column| column.name.as_str()).collect::<Vec<_>>(), vec!["region", "amount"]);
        assert_eq!(schema[0].kind, ColumnKind::Text);
        assert_eq!(schema[1].kind, ColumnKind::Number);
        assert!(!store.has_table("schema_preview"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn inspect_cp949_csv_schema_decodes_korean_headers() {
        let path = std::env::temp_dir().join(format!("cellx-inspect-cp949-{}.csv", std::process::id()));
        let (bytes, _, _) = encoding_rs::EUC_KR.encode("지역,금액\n서울,12\n부산,7\n");
        fs::write(&path, &bytes).unwrap();
        let store = DataStore::open_in_memory().unwrap();
        let schema = store.inspect_file_schema(&path, "ko-KR").unwrap();
        assert_eq!(schema.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["지역", "금액"]);
        assert_eq!(schema[1].kind, ColumnKind::Number);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn inspect_utf16_csv_schema_uses_the_import_encoding() {
        let path = std::env::temp_dir().join(format!("cellx-inspect-utf16-{}.csv", std::process::id()));
        let mut bytes = vec![0xff, 0xfe];
        for unit in "name,amount\nalpha,12\n".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        fs::write(&path, bytes).unwrap();
        let store = DataStore::open_in_memory().unwrap();
        let schema = store.inspect_file_schema(&path, "ko-KR").unwrap();
        assert_eq!(schema.len(), 2);
        assert_eq!(schema[0].name, "name");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn page_sorts_and_filters() {
        let mut s = store_with_sales();
        let page = s
            .page(
                "sales",
                0,
                10,
                Some(&Sort { column: "amount".into(), descending: true }),
                &[ColumnFilter { column: "region".into(), criterion: "서울".into() }],
            )
            .unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.rows[0][1], Scalar::Number(300.0));
    }

    #[test]
    fn transform_pipeline_creates_a_separate_result_table() {
        use crate::transform::{
            CompareOperator, FilterExpr, TransformPipeline, TransformStep, TransformValue, SortKey,
        };

        let mut store = store_with_sales();
        let pipeline = TransformPipeline {
            source_table: "sales".into(),
            steps: vec![
                TransformStep::Filter {
                    predicate: FilterExpr::Compare {
                        column: "status".into(),
                        operator: CompareOperator::NotEqual,
                        value: TransformValue::Text("취소".into()),
                    },
                },
                TransformStep::SelectColumns {
                    columns: vec!["region".into(), "amount".into()],
                },
                TransformStep::Sort {
                    keys: vec![SortKey {
                        column: "amount".into(),
                        descending: true,
                    }],
                },
            ],
        };

        let result = store.execute_transform("sales_clean", &pipeline).unwrap();
        assert_eq!(result.rows, 3);
        assert_eq!(
            result.columns.iter().map(|column| column.name.as_str()).collect::<Vec<_>>(),
            vec!["region", "amount"]
        );
        assert_eq!(
            store.scalar("SELECT amount FROM sales_clean LIMIT 1", &[]).unwrap(),
            Scalar::Number(300.0)
        );
        assert_eq!(store.table_info("sales").unwrap().rows, 4);
    }

    #[test]
    fn transform_pipeline_cannot_overwrite_its_source() {
        use crate::transform::TransformPipeline;

        let mut store = store_with_sales();
        let pipeline = TransformPipeline {
            source_table: "sales".into(),
            steps: vec![],
        };
        assert!(store.execute_transform("SALES", &pipeline).is_err());
        assert_eq!(store.table_info("sales").unwrap().rows, 4);
    }

    #[test]
    fn join_and_group_by_materialize_expected_aggregates() {
        use crate::transform::{AggregateFunction, AggregateSpec, JoinKind, TransformPipeline, TransformStep};

        let mut store = DataStore::open_in_memory().unwrap();
        store.create_table_from_sql(
            "orders",
            "SELECT * FROM (VALUES (1, 10.0::DOUBLE), (1, 20.0), (2, 15.0), (3, 7.0)) AS t(customer_id, amount)",
        ).unwrap();
        store.create_table_from_sql(
            "customers",
            "SELECT * FROM (VALUES (1, '서울'), (2, '부산')) AS t(id, region)",
        ).unwrap();
        let pipeline = TransformPipeline {
            source_table: "orders".into(),
            steps: vec![
                TransformStep::Join {
                    table: "customers".into(),
                    left_column: "customer_id".into(),
                    right_column: "id".into(),
                    kind: JoinKind::Left,
                    suffix: "customer".into(),
                },
                TransformStep::GroupBy {
                    columns: vec!["region".into()],
                    aggregates: vec![AggregateSpec {
                        function: AggregateFunction::Sum,
                        column: Some("amount".into()),
                        as_name: "total".into(),
                    }],
                },
            ],
        };
        assert!(store.execute_transform("customers", &pipeline).is_err());
        assert_eq!(store.table_info("customers").unwrap().rows, 2);
        let result = store.execute_transform("regional_totals", &pipeline).unwrap();
        assert_eq!(result.rows, 3);
        assert_eq!(
            store.scalar("SELECT total FROM regional_totals WHERE region = '서울'", &[]).unwrap(),
            Scalar::Number(30.0)
        );
        assert_eq!(store.table_info("orders").unwrap().rows, 4);
    }

    #[test]
    fn transform_preview_is_bounded_and_does_not_create_a_table() {
        use crate::transform::{TransformPipeline, TransformStep};

        let mut store = store_with_sales();
        let pipeline = TransformPipeline {
            source_table: "sales".into(),
            steps: vec![TransformStep::Limit { rows: 1000 }],
        };
        let (columns, rows, truncated) = store.preview_transform(&pipeline, 2).unwrap();
        assert_eq!(columns.len(), 4);
        assert_eq!(rows.len(), 2);
        assert!(truncated);
        assert!(!store.has_table("preview"));
        assert_eq!(store.table_info("sales").unwrap().rows, 4);
    }

    #[test]
    fn user_sql_must_be_a_plain_select() {
        let s = store_with_sales();
        assert!(s.validate_user_select("SELECT region, sum(amount) FROM sales GROUP BY 1").is_ok());
        assert!(s.validate_user_select("DROP TABLE sales").is_err());
        assert!(s.validate_user_select("SELECT 1; DROP TABLE sales").is_err());
        assert!(s.validate_user_select("SELECT * FROM read_csv('/etc/passwd')").is_err());
        assert!(s.validate_user_select("COPY sales TO 'x.csv'").is_err());
    }

    #[test]
    fn update_cell_by_row_id() {
        let mut s = store_with_sales();
        let before = s.version("sales");
        s.update_cell("sales", 1, "amount", "111").unwrap();
        assert!(s.version("sales") > before);
        let v = s.scalar("SELECT amount FROM sales WHERE _rid = 1", &[]).unwrap();
        assert_eq!(v, Scalar::Number(111.0));
    }

    #[test]
    fn recreated_table_never_reuses_a_cache_generation() {
        let mut s = store_with_sales();
        let before = s.version("sales");
        s.drop_table("SALES").unwrap();
        s.create_table_from_sql("sales", "SELECT 1 AS amount").unwrap();
        let after = s.version("sales");
        assert!(after > before, "drop/recreate must advance the generation");
    }
    #[test]
    fn page_limit_is_bounded() {
        let mut s = store_with_sales();
        let page = s.page("sales", 0, u64::MAX, None, &[]).unwrap();
        assert_eq!(page.rows.len(), 4);
        let empty = s.page("sales", 0, 0, None, &[]).unwrap();
        assert!(empty.rows.is_empty());
    }

    #[test]
    fn csv_dates_import_and_export_the_same_way_in_every_language() {
        let path = std::env::temp_dir().join(format!("cellx-iso-dates-{}.csv", std::process::id()));
        fs::write(&path, "id,d\n1,2025-05-04\n2,2025-12-31\n").unwrap();
        for locale in ["ko-KR", "en-US", "ja-JP", "zh-CN", "zh-TW"] {
            let mut s = DataStore::open_in_memory().unwrap();
            let info = s.import_file_with_locale(&path, "t", locale).unwrap();
            assert_eq!(info.columns[1].kind, ColumnKind::Date, "{locale}");
            let out = std::env::temp_dir().join(format!("cellx-iso-out-{}-{locale}.csv", std::process::id()));
            s.export_with_locale("t", &out, locale).unwrap();
            let text = fs::read_to_string(&out).unwrap();
            assert!(text.contains("2025-05-04"), "{locale}: {text}");
            let _ = fs::remove_file(out);
        }
        let _ = fs::remove_file(path);
    }

    #[test]
    fn csv_export_writes_utf8_bom_and_can_be_reimported() {
        let source = store_with_sales();
        let path = std::env::temp_dir().join(format!("cellx-locale-export-{}.csv", std::process::id()));
        let _ = fs::remove_file(&path);
        source.export_with_locale("sales", &path, "en-US").unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.starts_with(&[0xef, 0xbb, 0xbf]));

        let mut target = DataStore::open_in_memory().unwrap();
        let info = target.import_file_with_locale(&path, "roundtrip", "en-US").unwrap();
        assert_eq!(info.rows, 4);
        let count = target.scalar("SELECT count(*) FROM roundtrip", &[]).unwrap();
        assert_eq!(count, Scalar::Number(4.0));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn csv_import_supports_utf16_bom() {
        let path = std::env::temp_dir().join(format!("cellx-utf16-{}.csv", std::process::id()));
        let mut bytes = vec![0xff, 0xfe];
        for unit in "name,amount\nalpha,12\n".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        fs::write(&path, bytes).unwrap();
        let mut store = DataStore::open_in_memory().unwrap();
        let info = store.import_file_with_locale(&path, "utf16_data", "ko-KR").unwrap();
        assert_eq!(info.rows, 1);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn csv_import_rejects_utf32_with_a_specific_error_code() {
        let path = std::env::temp_dir().join(format!("cellx-utf32-{}.csv", std::process::id()));
        fs::write(&path, [0xff, 0xfe, 0x00, 0x00, 0x61, 0x00, 0x00, 0x00]).unwrap();
        let mut store = DataStore::open_in_memory().unwrap();
        let error = store.import_file_with_locale(&path, "invalid_csv", "ko-KR").unwrap_err();
        assert_eq!(error.code(), "CSV_ENCODING");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn tsv_import_uses_tab_delimiter_and_utf8_bom() {
        let path = std::env::temp_dir().join(format!("cellx-tsv-{}.tsv", std::process::id()));
        fs::write(&path, b"\xef\xbb\xbfregion\tamount\nSeoul\t12.5\nBusan\t7.25\n").unwrap();
        let mut store = DataStore::open_in_memory().unwrap();
        let info = store.import_file_with_locale(&path, "tsv_data", "ko-KR").unwrap();
        assert_eq!(info.rows, 2);
        assert_eq!(info.columns.len(), 2);
        let _ = fs::remove_file(path);
    }


    fn cp949(text: &str) -> Vec<u8> {
        encoding_rs::EUC_KR.encode(text).0.into_owned()
    }

    #[test]
    fn csv_import_reads_korean_excel_cp949_files() {
        let path = std::env::temp_dir().join(format!("cellx-cp949-{}.csv", std::process::id()));
        // 똠 and 펲 are in CP949 but not in plain EUC-KR.
        fs::write(&path, cp949("지역,금액,메모\n서울,12,똠방각하\n부산,7,펲\n")).unwrap();
        let mut store = DataStore::open_in_memory().unwrap();
        let info = store.import_file_with_locale(&path, "cp949_data", "ko-KR").unwrap();
        assert_eq!(info.rows, 2);
        assert_eq!(info.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["지역", "금액", "메모"]);
        let v = store.scalar("SELECT 메모 FROM cp949_data WHERE _rid = 1", &[]).unwrap();
        assert_eq!(v, Scalar::Text("똠방각하".into()));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn csv_import_falls_back_when_korean_text_starts_late() {
        // The first 64 KB are ASCII (valid UTF-8); the Korean text comes after.
        let path = std::env::temp_dir().join(format!("cellx-cp949-late-{}.csv", std::process::id()));
        let mut text = String::from("id,name\n");
        for i in 0..10_000 {
            text.push_str(&format!("{i},abc\n"));
        }
        text.push_str("10000,한글\n");
        fs::write(&path, cp949(&text)).unwrap();
        let mut store = DataStore::open_in_memory().unwrap();
        let info = store.import_file_with_locale(&path, "late", "ko-KR").unwrap();
        assert_eq!(info.rows, 10_001);
        let v = store.scalar("SELECT name FROM late WHERE id = 10000", &[]).unwrap();
        assert_eq!(v, Scalar::Text("한글".into()));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn csv_import_uses_the_encoding_of_the_ui_language() {
        let path = std::env::temp_dir().join(format!("cellx-sjis-{}.csv", std::process::id()));
        fs::write(&path, encoding_rs::SHIFT_JIS.encode("地域,金額\n東京,12\n").0).unwrap();
        let mut store = DataStore::open_in_memory().unwrap();
        let info = store.import_file_with_locale(&path, "sjis", "ja-JP").unwrap();
        assert_eq!(info.columns[0].name, "地域");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn csv_export_quotes_header_names_and_handles_empty_tables() {
        let mut store = DataStore::open_in_memory().unwrap();
        store.create_table_from_sql("odd", "SELECT 1 AS \"a,b\", 'x' AS \"it's\" WHERE false").unwrap();
        let path = std::env::temp_dir().join(format!("cellx-odd-{}.csv", std::process::id()));
        store.export("odd", &path).unwrap();
        let text = String::from_utf8(fs::read(&path).unwrap()).unwrap();
        assert_eq!(text, "\u{feff}\"a,b\",it's\n");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn encoding_is_detected_whatever_the_ui_language() {
        let korean = cp949("지역,금액\n서울특별시,12\n부산광역시,7\n");
        let japanese = encoding_rs::SHIFT_JIS.encode("地域,金額\nとうきょう,12\nおおさか,7\n").0.into_owned();
        assert_eq!(detect_encoding(&korean, "en-US").unwrap().name(), "cp949");
        assert_eq!(detect_encoding(&korean, "ja-JP").unwrap().name(), "cp949");
        assert_eq!(detect_encoding(&japanese, "ko-KR").unwrap().name(), "shift_jis");
        let chinese = encoding_rs::GBK.encode("地区,金额,备注\n北京市,12,这是我们的数据\n上海市,7,销售额统计\n").0.into_owned();
        assert_eq!(detect_encoding(&chinese, "ko-KR").unwrap().name(), "gbk");
        assert_eq!(detect_encoding(&chinese, "zh-CN").unwrap().name(), "gbk");
        let latin = encoding_rs::WINDOWS_1252.encode("name,city\nJosé,São Paulo\nRenée,Zürich\n").0.into_owned();
        assert_eq!(detect_encoding(&latin, "ko-KR").unwrap().name(), "windows-1252");
        assert_eq!(detect_encoding("a,b\n1,2\n".as_bytes(), "ko-KR").unwrap().name(), "utf-8");
        assert_eq!(detect_encoding(&[0xff, 0xfe, b'a', 0], "ko-KR").unwrap().name(), "utf-16");
    }

    #[test]
    fn import_preview_and_explicit_encoding() {
        let path = std::env::temp_dir().join(format!("cellx-explicit-{}.csv", std::process::id()));
        fs::write(&path, cp949("지역,금액\n서울,12\n")).unwrap();
        let p = DataStore::preview_file(&path, "en-US", None, 10).unwrap();
        assert_eq!(p.encoding, "cp949");
        assert_eq!(p.lines, ["지역,금액", "서울,12"]);
        // Read as UTF-8 on purpose: an encoding error, not garbage.
        let mut store = DataStore::open_in_memory().unwrap();
        let e = store.import_file_with_encoding(&path, "x", "ko-KR", Some("utf-8")).unwrap_err();
        assert_eq!(e.code(), "CSV_ENCODING");
        let info = store.import_file_with_encoding(&path, "x", "en-US", Some("cp949")).unwrap();
        assert_eq!(info.columns[0].name, "지역");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn export_in_cp949_round_trips_and_refuses_unmappable_text() {
        let mut store = DataStore::open_in_memory().unwrap();
        store.create_table_from_sql("k", "SELECT '서울' AS 지역, 12 AS 금액").unwrap();
        let path = std::env::temp_dir().join(format!("cellx-cp949-out-{}.csv", std::process::id()));
        store.export_with_encoding("k", &path, Some("cp949")).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(!bytes.starts_with(&[0xef, 0xbb, 0xbf]));
        assert_eq!(encoding_rs::EUC_KR.decode(&bytes).0, "지역,금액\n서울,12\n");
        let info = store.import_file_with_locale(&path, "back", "ko-KR").unwrap();
        assert_eq!(info.columns[0].name, "지역");

        store.create_table_from_sql("emoji", "SELECT '😀' AS a").unwrap();
        let e = store.export_with_encoding("emoji", &path, Some("cp949")).unwrap_err();
        assert_eq!(e.code(), "TEXT_ENCODING");
        assert!(!path.exists(), "a failed export leaves no file");
    }

    #[test]
    #[ignore]
    fn measure_big_csv() {
        let Ok(src) = std::env::var("CELLX_BIG_CSV") else { return };
        let src = Path::new(&src);
        let t = std::time::Instant::now();
        let mut s = DataStore::open_in_memory().unwrap();
        s.import_file_with_locale(src, "o", "ko-KR").unwrap();
        eprintln!("MEASURE utf8 import {:?}", t.elapsed());
        let out = src.with_file_name("o_out.csv");
        let t = std::time::Instant::now();
        s.export("o", &out).unwrap();
        eprintln!("MEASURE export (one pass) {:?}", t.elapsed());
        let tmp = src.with_file_name("o_tmp.csv");
        let t = std::time::Instant::now();
        s.conn.execute_batch(&format!("COPY (SELECT * EXCLUDE (_rid) FROM o ORDER BY _rid) TO '{}' (FORMAT csv, HEADER true)", tmp.display())).unwrap();
        let mut d = File::create(&out).unwrap();
        d.write_all(&[0xef, 0xbb, 0xbf]).unwrap();
        std::io::copy(&mut File::open(&tmp).unwrap(), &mut d).unwrap();
        d.sync_all().unwrap();
        eprintln!("MEASURE export (temp + copy, before) {:?}", t.elapsed());
        let _ = fs::remove_file(&tmp);
        let _ = fs::remove_file(&out);
        let cp = src.with_file_name("o_cp949.csv");
        let text = fs::read_to_string(src).unwrap();
        fs::write(&cp, encoding_rs::EUC_KR.encode(&text).0).unwrap();
        drop(text);
        let t = std::time::Instant::now();
        let mut s2 = DataStore::open_in_memory().unwrap();
        s2.import_file_with_locale(&cp, "o", "ko-KR").unwrap();
        eprintln!("MEASURE cp949 import {:?}", t.elapsed());
        let _ = fs::remove_file(&cp);
    }
}
