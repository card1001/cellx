//! Importing a worksheet of an Excel or OpenDocument file (xlsx, xlsm,
//! xlsb, xls, ods) into a data sheet.
//!
//! The cells are read with calamine. Each column gets one type from the
//! values it holds (whole numbers, numbers, dates, date-times, times,
//! TRUE/FALSE, or text when they are mixed), the rows are written to a
//! temporary CSV with those types fixed, and DuckDB reads that CSV with no
//! type guessing — so a column of codes such as `00123` stays text, and a
//! date column stays a date.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use calamine::{open_workbook_auto, Data, Range, Reader, SheetType};

use super::{is_valid_table_name, DataStore, TableInfo};
use crate::error::{CellxError, Result};
use crate::job::JobPhase;

/// File extensions read as spreadsheets rather than as CSV.
pub const SPREADSHEET_EXTENSIONS: &[&str] = &["xlsx", "xlsm", "xlsb", "xls", "ods"];

pub fn is_spreadsheet(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| SPREADSHEET_EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

fn xlsx_error(e: impl std::fmt::Display) -> CellxError {
    CellxError::Xlsx(e.to_string())
}

/// The worksheets of a spreadsheet file, in workbook order (chart sheets and
/// macro sheets are left out).
pub fn spreadsheet_sheets(path: &Path) -> Result<Vec<String>> {
    let book = open_workbook_auto(path).map_err(xlsx_error)?;
    Ok(book
        .sheets_metadata()
        .iter()
        .filter(|s| s.typ == SheetType::WorkSheet)
        .map(|s| s.name.clone())
        .collect())
}

/// The type a column is stored as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Only empty cells so far.
    Unknown,
    Bool,
    Int,
    Float,
    Date,
    Timestamp,
    Time,
    Text,
}

impl Kind {
    fn sql(self) -> &'static str {
        match self {
            Kind::Bool => "BOOLEAN",
            Kind::Int => "BIGINT",
            Kind::Float => "DOUBLE",
            Kind::Date => "DATE",
            Kind::Timestamp => "TIMESTAMP",
            Kind::Time => "TIME",
            Kind::Unknown | Kind::Text => "VARCHAR",
        }
    }

    fn merge(self, other: Kind) -> Kind {
        use Kind::*;
        match (self, other) {
            (a, b) if a == b => a,
            (Unknown, b) => b,
            (a, Unknown) => a,
            (Int, Float) | (Float, Int) => Float,
            (Date, Timestamp) | (Timestamp, Date) => Timestamp,
            _ => Text,
        }
    }
}

/// Integers up to 2^53 are exact in a double, so whole numbers below that
/// are stored as BIGINT.
const MAX_EXACT: f64 = 9_007_199_254_740_992.0;

fn kind_of(cell: &Data) -> Kind {
    match cell {
        Data::Empty | Data::Error(_) => Kind::Unknown,
        Data::String(s) if s.is_empty() => Kind::Unknown,
        Data::Int(_) => Kind::Int,
        Data::Float(v) => {
            if v.fract() == 0.0 && v.abs() < MAX_EXACT {
                Kind::Int
            } else {
                Kind::Float
            }
        }
        Data::Bool(_) => Kind::Bool,
        Data::DateTime(dt) if dt.is_datetime() => {
            let v = dt.as_f64();
            if (0.0..1.0).contains(&v) {
                Kind::Time
            } else if v.fract() == 0.0 {
                Kind::Date
            } else {
                Kind::Timestamp
            }
        }
        // Durations ([h]:mm), ISO strings from ods and text.
        _ => Kind::Text,
    }
}

/// A number as Excel shows it in a General cell: at most 15 significant digits.
fn number_text(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return if v.is_finite() { "0".into() } else { v.to_string() };
    }
    let rounded: f64 = format!("{v:.14e}").parse().unwrap_or(v);
    let exp = rounded.abs().log10().floor() as i32;
    if (-5..15).contains(&exp) {
        return format!("{rounded}");
    }
    let text = format!("{rounded:e}");
    let (mantissa, exp) = text.split_once('e').unwrap_or((&text, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    format!("{mantissa}E{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
}

fn date_parts(dt: &calamine::ExcelDateTime) -> (u16, u8, u8, u8, u8, u8, u16) {
    dt.to_ymd_hms_milli()
}

fn date_text(dt: &calamine::ExcelDateTime) -> String {
    let (y, m, d, ..) = date_parts(dt);
    format!("{y:04}-{m:02}-{d:02}")
}

fn time_text(h: u8, mi: u8, s: u8, ms: u16) -> String {
    if ms == 0 {
        format!("{h:02}:{mi:02}:{s:02}")
    } else {
        format!("{h:02}:{mi:02}:{s:02}.{ms:03}")
    }
}

fn timestamp_text(dt: &calamine::ExcelDateTime) -> String {
    let (y, m, d, h, mi, s, ms) = date_parts(dt);
    format!("{y:04}-{m:02}-{d:02} {}", time_text(h, mi, s, ms))
}

/// `[h]:mm:ss` for a duration in days.
fn duration_text(days: f64) -> String {
    let total = (days * 86_400.0).round() as i64;
    let sign = if total < 0 { "-" } else { "" };
    let t = total.abs();
    format!("{sign}{}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60)
}

/// A cell as text, for a text column or a header.
fn cell_text(cell: &Data) -> String {
    match cell {
        Data::Empty | Data::Error(_) => String::new(),
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(v) => number_text(*v),
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Data::DateTime(dt) if dt.is_duration() => duration_text(dt.as_f64()),
        Data::DateTime(dt) => match kind_of(cell) {
            Kind::Date => date_text(dt),
            Kind::Time => {
                let (.., h, mi, s, ms) = date_parts(dt);
                time_text(h, mi, s, ms)
            }
            _ => timestamp_text(dt),
        },
    }
}

/// A cell as a CSV field of a column of type `kind`. Empty is NULL; text is
/// always quoted, so an empty string stays an empty string.
fn field(cell: &Data, kind: Kind) -> String {
    if kind_of(cell) == Kind::Unknown {
        return String::new();
    }
    match kind {
        Kind::Text | Kind::Unknown => format!("\"{}\"", cell_text(cell).replace('"', "\"\"")),
        Kind::Bool => match cell {
            Data::Bool(true) => "true".into(),
            _ => "false".into(),
        },
        Kind::Int => match cell {
            Data::Int(i) => i.to_string(),
            Data::Float(v) => format!("{}", *v as i64),
            _ => String::new(),
        },
        Kind::Float => match cell {
            Data::Int(i) => i.to_string(),
            // Full precision: `{}` round-trips a double.
            Data::Float(v) => format!("{v}"),
            _ => String::new(),
        },
        Kind::Date => match cell {
            Data::DateTime(dt) => date_text(dt),
            _ => String::new(),
        },
        Kind::Timestamp => match cell {
            Data::DateTime(dt) => timestamp_text(dt),
            _ => String::new(),
        },
        Kind::Time => match cell {
            Data::DateTime(dt) => {
                let (.., h, mi, s, ms) = date_parts(dt);
                time_text(h, mi, s, ms)
            }
            _ => String::new(),
        },
    }
}

/// Excel's name for column `index` (0 = A).
fn column_letter(mut index: u32) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (index % 26) as u8);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Column names from the header row: blank names become the sheet's column
/// letter, repeats get `_2`, `_3`, … (compared without case, as DuckDB does).
fn column_names(header: Option<&[Data]>, width: usize, first_col: u32) -> Vec<String> {
    let mut names: Vec<String> = Vec::with_capacity(width);
    let mut seen = std::collections::HashSet::new();
    seen.insert("_rid".to_string());
    for i in 0..width {
        let raw = header
            .and_then(|row| row.get(i))
            .map(cell_text)
            .unwrap_or_default();
        let base = {
            let t = raw.trim().replace(['\r', '\n'], " ");
            if t.is_empty() { column_letter(first_col + i as u32) } else { t }
        };
        let mut name = base.clone();
        let mut n = 2;
        while !seen.insert(name.to_lowercase()) {
            name = format!("{base}_{n}");
            n += 1;
        }
        names.push(name);
    }
    names
}

impl DataStore {
    /// Imports one worksheet (`None`: the first) of an xlsx, xlsm, xlsb, xls
    /// or ods file. With `header`, the first row holds the column names.
    pub fn import_spreadsheet(
        &mut self,
        path: &Path,
        table: &str,
        sheet: Option<&str>,
        header: bool,
    ) -> Result<TableInfo> {
        self.monitored(|store| store.import_spreadsheet_inner(path, table, sheet, header))
    }

    fn import_spreadsheet_inner(
        &mut self,
        path: &Path,
        table: &str,
        sheet: Option<&str>,
        header: bool,
    ) -> Result<TableInfo> {
        if !is_valid_table_name(table) {
            return Err(CellxError::InvalidName(table.to_string()));
        }
        // Reading the file has no progress of its own.
        self.monitor.set_rows(0, 0);
        self.monitor.set_phase(JobPhase::Converting);
        let mut book = open_workbook_auto(path).map_err(xlsx_error)?;
        let name = match sheet.map(str::trim).filter(|s| !s.is_empty()) {
            Some(name) => book
                .sheet_names()
                .into_iter()
                .find(|s| s.eq_ignore_ascii_case(name))
                .ok_or_else(|| CellxError::Xlsx(format!("시트가 없습니다: {name}")))?,
            None => book
                .sheets_metadata()
                .iter()
                .find(|s| s.typ == SheetType::WorkSheet)
                .map(|s| s.name.clone())
                .ok_or_else(|| CellxError::Xlsx("워크시트가 없습니다".into()))?,
        };
        let range = book.worksheet_range(&name).map_err(xlsx_error)?;
        drop(book);
        self.monitor.check()?;

        static ID: AtomicU64 = AtomicU64::new(1);
        let id = ID.fetch_add(1, Ordering::Relaxed);
        let temp = std::env::temp_dir().join(format!("cellx-sheet-{}-{id}.csv", std::process::id()));
        let result = (|| -> Result<TableInfo> {
            let columns = self.write_range_csv(&range, header, &temp)?;
            let spec = columns
                .iter()
                .map(|(name, kind)| format!("'{}': '{}'", name.replace('\'', "''"), kind.sql()))
                .collect::<Vec<_>>()
                .join(", ");
            let reader = format!(
                "read_csv(?, header = false, auto_detect = false, delim = ',', quote = '\"', escape = '\"', \
                 allow_quoted_nulls = false, columns = {{{spec}}})"
            );
            self.import_with_reader(&temp, table, reader)
        })();
        let _ = fs::remove_file(&temp);
        result
    }

    /// Writes the cells to `temp` and returns each column's name and type.
    fn write_range_csv(&self, range: &Range<Data>, header: bool, temp: &Path) -> Result<Vec<(String, Kind)>> {
        let (height, width) = range.get_size();
        if height == 0 || width == 0 || (header && height == 1 && width == 0) {
            return Err(CellxError::Xlsx("시트가 비어 있습니다".into()));
        }
        let first_col = range.start().map(|(_, c)| c).unwrap_or(0);
        let mut rows = range.rows();
        let header_row = if header { rows.next() } else { None };
        let names = column_names(header_row, width, first_col);
        let data_rows = height - usize::from(header);

        // One pass to type the columns, one to write them.
        let mut kinds = vec![Kind::Unknown; width];
        for (i, row) in range.rows().skip(usize::from(header)).enumerate() {
            for (kind, cell) in kinds.iter_mut().zip(row) {
                *kind = kind.merge(kind_of(cell));
            }
            if i % 50_000 == 0 {
                self.monitor.check()?;
                self.monitor.set_rows(i as u64 / 2, data_rows as u64);
            }
        }

        let mut out = BufWriter::with_capacity(1 << 20, File::create(temp)?);
        let mut line = String::new();
        for (i, row) in rows.enumerate() {
            line.clear();
            for (c, kind) in kinds.iter().enumerate() {
                if c > 0 {
                    line.push(',');
                }
                if let Some(cell) = row.get(c) {
                    line.push_str(&field(cell, *kind));
                }
            }
            line.push('\n');
            out.write_all(line.as_bytes())?;
            if i % 50_000 == 0 {
                self.monitor.check()?;
                self.monitor.set_rows((data_rows + i) as u64 / 2, data_rows as u64);
            }
        }
        out.flush()?;
        self.monitor.set_rows(data_rows as u64, data_rows as u64);
        Ok(names.into_iter().zip(kinds).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_types_merge_like_excel_data() {
        assert_eq!(Kind::Int.merge(Kind::Float), Kind::Float);
        assert_eq!(Kind::Date.merge(Kind::Timestamp), Kind::Timestamp);
        assert_eq!(Kind::Int.merge(Kind::Text), Kind::Text);
        assert_eq!(Kind::Unknown.merge(Kind::Bool), Kind::Bool);
        assert_eq!(Kind::Date.merge(Kind::Int), Kind::Text);
    }

    #[test]
    fn numbers_read_as_excel_shows_them() {
        assert_eq!(number_text(42.0), "42");
        assert_eq!(number_text(0.1 + 0.2), "0.3");
        assert_eq!(number_text(1234.5), "1234.5");
        assert_eq!(number_text(1e20), "1E+20");
    }

    #[test]
    fn header_names_are_unique_and_never_blank() {
        let header = [
            Data::String("금액".into()),
            Data::Empty,
            Data::String("금액".into()),
            Data::String("_RID".into()),
        ];
        assert_eq!(column_names(Some(&header), 4, 2), vec!["금액", "D", "금액_2", "_RID_2"]);
        assert_eq!(column_letter(0), "A");
        assert_eq!(column_letter(25), "Z");
        assert_eq!(column_letter(26), "AA");
        assert_eq!(column_letter(16_383), "XFD");
    }

    fn sample_xlsx() -> std::path::PathBuf {
        let mut wb = crate::Workbook::new().unwrap();
        let rows: &[&[&str]] = &[
            &["코드", "금액", "날짜", "시각", "여부", "섞임", "", "메모"],
            &["'00123", "100", "2025-05-04", "2025-05-04 13:30", "TRUE", "1", "x", "첫 줄\n둘째 줄"],
            &["'00456", "250.5", "2024-12-31", "2024-12-31 08:00", "FALSE", "가", "y", "\"따옴표\", 쉼표"],
            &["'00789", "", "", "", "", "3", "", ""],
        ];
        for (r, row) in rows.iter().enumerate() {
            for (c, text) in row.iter().enumerate() {
                if !text.is_empty() {
                    wb.set_input(0, r as i32 + 3, c as i32 + 2, text).unwrap();
                }
            }
        }
        wb.add_sheet().unwrap();
        wb.set_input(1, 1, 1, "7").unwrap();
        let dir = crate::project::new_work_dir().unwrap();
        let path = dir.join("book.xlsx");
        wb.save_xlsx(&path).unwrap();
        path
    }

    #[test]
    fn a_worksheet_becomes_a_typed_data_sheet() {
        let path = sample_xlsx();
        let sheets = spreadsheet_sheets(&path).unwrap();
        assert_eq!(sheets.len(), 2);
        assert!(is_spreadsheet(&path));

        let mut store = DataStore::open_in_memory().unwrap();
        let info = store.import_spreadsheet(&path, "sales", None, true).unwrap();
        assert_eq!(info.rows, 3);
        let types: Vec<(&str, &str)> = info
            .columns
            .iter()
            .map(|c| (c.name.as_str(), c.sql_type.as_str()))
            .collect();
        assert_eq!(
            types,
            vec![
                ("코드", "VARCHAR"),
                ("금액", "DOUBLE"),
                ("날짜", "DATE"),
                ("시각", "TIMESTAMP"),
                ("여부", "BOOLEAN"),
                ("섞임", "VARCHAR"),
                ("H", "VARCHAR"),
                ("메모", "VARCHAR"),
            ]
        );
        let q = |sql: &str| store.scalar(sql, &[]).unwrap();
        use crate::value::Scalar;
        assert_eq!(q("SELECT 코드 FROM sales WHERE _rid = 1"), Scalar::Text("00123".into()));
        assert_eq!(q("SELECT 금액 FROM sales WHERE _rid = 2"), Scalar::Number(250.5));
        assert_eq!(q("SELECT 금액 IS NULL FROM sales WHERE _rid = 3"), Scalar::Bool(true));
        assert_eq!(q("SELECT CAST(날짜 AS VARCHAR) FROM sales WHERE _rid = 1"), Scalar::Text("2025-05-04".into()));
        assert_eq!(q("SELECT CAST(시각 AS VARCHAR) FROM sales WHERE _rid = 1"), Scalar::Text("2025-05-04 13:30:00".into()));
        assert_eq!(q("SELECT 섞임 FROM sales WHERE _rid = 1"), Scalar::Text("1".into()));
        assert_eq!(q("SELECT 메모 FROM sales WHERE _rid = 2"), Scalar::Text("\"따옴표\", 쉼표".into()));
        assert_eq!(q("SELECT 메모 FROM sales WHERE _rid = 1"), Scalar::Text("첫 줄\n둘째 줄".into()));

        // Another sheet, and no header row.
        let other = store.import_spreadsheet(&path, "second", Some(&sheets[1]), false).unwrap();
        assert_eq!(other.rows, 1);
        assert_eq!(other.columns.iter().find(|c| c.name != "_rid").unwrap().name, "A");
        assert!(store.import_spreadsheet(&path, "missing", Some("없는시트"), true).is_err());
        let _ = fs::remove_file(path);
    }

    /// `CELLX_XLSX=/path/big.xlsx cargo test -p cellx-core --release --lib -- --ignored --nocapture measure_xlsx`
    #[test]
    #[ignore]
    fn measure_xlsx_import() {
        let Ok(path) = std::env::var("CELLX_XLSX") else { return };
        let mut store = DataStore::open_in_memory().unwrap();
        let start = std::time::Instant::now();
        let info = store.import_spreadsheet(Path::new(&path), "big", None, true).unwrap();
        println!(
            "rows {} in {:?}: {:?}",
            info.rows,
            start.elapsed(),
            info.columns.iter().map(|c| format!("{} {}", c.name, c.sql_type)).collect::<Vec<_>>()
        );
    }
}
