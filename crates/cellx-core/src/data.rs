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
        let mut store = DataStore {
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

        let mut head = Vec::with_capacity(64 * 1024);
        File::open(path)?.take(64 * 1024).read_to_end(&mut head)?;
        let utf32 = head.starts_with(&[0xff, 0xfe, 0x00, 0x00]) || head.starts_with(&[0x00, 0x00, 0xfe, 0xff]);
        if utf32 {
            return Err(CellxError::CsvEncoding("UTF-32".into()));
        }
        if head.starts_with(&[0xff, 0xfe]) || head.starts_with(&[0xfe, 0xff]) {
            return self.import_with_reader(path, table, csv_reader("utf-16"));
        }
        let legacy = legacy_encoding(locale);
        // A file that is not UTF-8 from the start is converted right away;
        // one whose first 64 KB are UTF-8 is tried as UTF-8 first.
        if !head.starts_with(&[0xef, 0xbb, 0xbf]) && !is_utf8_prefix(&head) {
            return self.import_transcoded(path, table, legacy, csv_reader("utf-8"));
        }
        match self.import_with_reader(path, table, csv_reader("utf-8")) {
            Err(CellxError::CsvEncoding(_)) => self.import_transcoded(path, table, legacy, csv_reader("utf-8")),
            other => other,
        }
    }

    fn import_with_reader(&mut self, path: &Path, table: &str, reader: String) -> Result<TableInfo> {
        let path_text = path
            .to_str()
            .ok_or_else(|| CellxError::InvalidName(path.display().to_string()))?;
        let sql = format!(
            "CREATE OR REPLACE TABLE {t} AS SELECT row_number() OVER () AS _rid, * FROM {reader}",
            t = quote_ident(table)
        );
        if let Err(error) = self.conn.execute(&sql, [path_text]) {
            let diagnostic = error.to_string().to_ascii_lowercase();
            if diagnostic.contains("utf-8") || diagnostic.contains("utf8") || diagnostic.contains("unicode") {
                return Err(CellxError::CsvEncoding("non-UTF-8 input".into()));
            }
            return Err(error.into());
        }
        self.touch(table);
        self.table_info(table)
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
            transcode_to_utf8(path, &temp, encoding)?;
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
        self.conn.execute_batch(&sql)?;
        self.touch(table);
        self.table_info(table)
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
        let path_text = path
            .to_str()
            .ok_or_else(|| CellxError::InvalidName(path.display().to_string()))?;
        let is_parquet = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("parquet"));
        if is_parquet {
            let escaped = path_text.replace('\'', "''");
            self.conn.execute_batch(&format!(
                "COPY (SELECT * EXCLUDE (_rid) FROM {t} ORDER BY _rid) TO '{escaped}' (FORMAT parquet)",
                t = quote_ident(table)
            ))?;
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
            fs::write(path, format!("\u{feff}{header}\n"))?;
            return Ok(());
        }
        let prefix = format!("\u{feff}{header}\n").replace('\'', "''");
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

fn transcode_to_utf8(from: &Path, to: &Path, encoding: &'static encoding_rs::Encoding) -> Result<()> {
    let mut input = std::io::BufReader::with_capacity(1 << 20, File::open(from)?);
    let mut output = std::io::BufWriter::with_capacity(1 << 20, File::create(to)?);
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let mut inbuf = vec![0_u8; 1 << 20];
    let mut outbuf = String::with_capacity(3 << 20);
    loop {
        let n = input.read(&mut inbuf)?;
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
                    return Err(CellxError::CsvEncoding(format!("UTF-8도 {}도 아닌 파일", encoding.name())));
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
