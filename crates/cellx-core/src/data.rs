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
use crate::transform::{TransformPipeline, TransformPlan, TransformValue};

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

    /// `_locale` is kept for the API: dates are detected from the file itself
    /// (ISO, and the other common forms DuckDB recognises), not forced to the
    /// UI language, so the same file imports the same way in every language.
    pub fn import_file_with_locale(&mut self, path: &Path, table: &str, _locale: &str) -> Result<TableInfo> {
        if !is_valid_table_name(table) {
            return Err(CellxError::InvalidName(table.to_string()));
        }
        let path_text = path
            .to_str()
            .ok_or_else(|| CellxError::InvalidName(path.display().to_string()))?;

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let mut encoding = "utf-8";
        if ext != "parquet" {
            let mut file = File::open(path)?;
            let mut bom = [0_u8; 4];
            let count = file.read(&mut bom)?;
            let utf16 = count >= 2 && ((bom[0] == 0xff && bom[1] == 0xfe) || (bom[0] == 0xfe && bom[1] == 0xff));
            let utf32 = count >= 4 && ((bom == [0xff, 0xfe, 0x00, 0x00]) || (bom == [0x00, 0x00, 0xfe, 0xff]));
            if utf32 {
                return Err(CellxError::CsvEncoding("UTF-32".into()));
            }
            if utf16 {
                encoding = "utf-16";
            }
        }

        let reader = if ext == "parquet" {
            "read_parquet(?)".to_string()
        } else {
            let delimiter = if ext == "tsv" { ", delim = '\\t'" } else { "" };
            format!(
                "read_csv(?, auto_detect = true, sample_size = 20480, ignore_errors = false, encoding = '{}', decimal_separator = '.'{})",
                encoding, delimiter
            )
        };
        let sql = format!(
            "CREATE OR REPLACE TABLE {t} AS SELECT row_number() OVER () AS _rid, * FROM {reader}",
            t = quote_ident(table)
        );
        match self.conn.execute(&sql, [path_text]) {
            Ok(_) => {}
            Err(error) => {
                let diagnostic = error.to_string().to_ascii_lowercase();
                if diagnostic.contains("utf-8") || diagnostic.contains("utf8") || diagnostic.contains("unicode") {
                    return Err(CellxError::CsvEncoding("non-UTF-8 input".into()));
                }
                return Err(error.into());
            }
        }
        self.touch(table);
        self.table_info(table)
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

    /// Builds a validated transformation plan against the current source schema.
    ///
    /// Planning does not execute SQL or modify the source table. The caller can
    /// inspect the generated SQL and bound parameters before scheduling execution.
    pub fn plan_transform(&mut self, pipeline: &TransformPipeline) -> Result<TransformPlan> {
        let info = self.table_info(&pipeline.source_table)?;
        let columns = info.columns.iter().map(|column| column.name.clone()).collect::<Vec<_>>();
        pipeline.compile(&columns)
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
        let limit = max_rows.clamp(1, 500).saturating_add(1);
        let sql = format!(
            "SELECT * FROM ({}) AS {} LIMIT {}",
            plan.sql,
            quote_ident("_cellx_preview"),
            limit
        );
        let (mut columns, mut rows, engine_truncated) = self.rows(&sql, &parameters, limit)?;
        let truncated = engine_truncated || rows.len() > max_rows;
        rows.truncate(max_rows);
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
        if output_table.eq_ignore_ascii_case(&pipeline.source_table) {
            return Err(CellxError::Bridge(
                "변환 결과를 원본 데이터 시트에 덮어쓸 수 없습니다".into(),
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

        static EXPORT_ID: AtomicU64 = AtomicU64::new(1);
        let id = EXPORT_ID.fetch_add(1, Ordering::Relaxed);
        let temp_path = path.with_file_name(format!(".cellx-export-{}-{id}.tmp", std::process::id()));
        let temp_text = temp_path
            .to_str()
            .ok_or_else(|| CellxError::InvalidName(temp_path.display().to_string()))?
            .replace('\'', "''");
        let date_format = "%Y-%m-%d";
        let timestamp_format = "%Y-%m-%d %H:%M:%S";
        let sql = format!(
            "COPY (SELECT * EXCLUDE (_rid) FROM {t} ORDER BY _rid) TO '{temp_text}' (FORMAT csv, HEADER true, DELIMITER ',', DATEFORMAT '{date_format}', TIMESTAMPFORMAT '{timestamp_format}')",
            t = quote_ident(table)
        );
        if let Err(error) = self.conn.execute_batch(&sql) {
            let _ = fs::remove_file(&temp_path);
            return Err(error.into());
        }

        let write_result = (|| -> Result<()> {
            let mut source = File::open(&temp_path)?;
            let mut destination = File::create(path)?;
            destination.write_all(&[0xef, 0xbb, 0xbf])?;
            std::io::copy(&mut source, &mut destination)?;
            destination.sync_all()?;
            Ok(())
        })();
        let _ = fs::remove_file(&temp_path);
        if write_result.is_err() {
            let _ = fs::remove_file(path);
        }
        write_result
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

}
