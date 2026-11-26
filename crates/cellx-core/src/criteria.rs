//! Translation of Excel criteria (`">=100"`, `"서울"`, `"*카드*"`, `"<>"`) into SQL.
//!
//! The rules follow Excel's COUNTIFS/SUMIFS behaviour:
//! * a leading operator (`=`, `<>`, `<`, `<=`, `>`, `>=`) is optional,
//! * text comparison is case-insensitive,
//! * `*` and `?` are wildcards in equality tests, `~` escapes them,
//! * a number criterion matches numbers, and dates through their serial number,
//! * `"<>x"` also matches blank cells, `""` matches blank cells.

use duckdb::types::Value;

use crate::data::{quote_ident, ColumnInfo, ColumnKind};
use crate::value::{Scalar, EXCEL_EPOCH_OFFSET_DAYS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Op {
    fn sql(self) -> &'static str {
        match self {
            Op::Eq => "=",
            Op::Ne => "<>",
            Op::Lt => "<",
            Op::Le => "<=",
            Op::Gt => ">",
            Op::Ge => ">=",
        }
    }
}

fn split_operator(s: &str) -> (Op, &str) {
    for (prefix, op) in [
        (">=", Op::Ge),
        ("<=", Op::Le),
        ("<>", Op::Ne),
        (">", Op::Gt),
        ("<", Op::Lt),
        ("=", Op::Eq),
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            return (op, rest);
        }
    }
    (Op::Eq, s)
}

/// Numeric view of a column: numbers as-is, dates as Excel serials, text ignored.
pub fn numeric_expr(col: &ColumnInfo) -> String {
    let c = quote_ident(&col.name);
    match col.kind {
        ColumnKind::Number => format!("CAST({c} AS DOUBLE)"),
        ColumnKind::Date => {
            format!("CAST(date_diff('day', DATE '1899-12-30', {c}) AS DOUBLE)")
        }
        ColumnKind::Timestamp => format!("(epoch({c}) / 86400.0 + {EXCEL_EPOCH_OFFSET_DAYS})"),
        ColumnKind::Bool | ColumnKind::Text | ColumnKind::Other => "CAST(NULL AS DOUBLE)".into(),
    }
}

fn text_expr(col: &ColumnInfo) -> String {
    format!("lower(CAST({} AS VARCHAR))", quote_ident(&col.name))
}

fn blank_expr(col: &ColumnInfo) -> String {
    let c = quote_ident(&col.name);
    format!("({c} IS NULL OR CAST({c} AS VARCHAR) = '')")
}

/// Converts Excel wildcards to a LIKE pattern; returns None when there are none.
fn wildcard_to_like(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut has_wildcard = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '~' => match chars.peek() {
                Some('*') | Some('?') | Some('~') => {
                    let next = chars.next().unwrap_or('~');
                    out.push(next);
                }
                _ => out.push('~'),
            },
            '*' => {
                has_wildcard = true;
                out.push('%');
            }
            '?' => {
                has_wildcard = true;
                out.push('_');
            }
            '%' | '_' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            other => out.push(other),
        }
    }
    has_wildcard.then_some(out)
}

/// Removes `~` escapes for an exact (non-wildcard) text match.
fn unescape_tilde(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '~' && matches!(chars.peek(), Some('*') | Some('?') | Some('~')) {
            if let Some(n) = chars.next() {
                out.push(n);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parses `YYYY-MM-DD` (optionally with `/` or `.`) into an Excel serial number.
pub fn parse_date_serial(s: &str) -> Option<f64> {
    let s = s.trim();
    let parts: Vec<&str> = s.split(['-', '/', '.']).collect();
    if parts.len() != 3 || parts[0].len() != 4 {
        return None;
    }
    let y: i64 = parts[0].parse().ok()?;
    let m: i64 = parts[1].parse().ok()?;
    let d: i64 = parts[2].parse().ok()?;
    if !(1..=12).contains(&m) || y < 1 || !(1..=31).contains(&d) {
        return None;
    }
    // Validate the actual Gregorian calendar date (including leap years), not
    // merely the independent month/day ranges.
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days_in_month = match m {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if d > days_in_month {
        return None;
    }
    // Days from civil (Howard Hinnant's algorithm), relative to 1970-01-01.
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days as f64 + EXCEL_EPOCH_OFFSET_DAYS)
}

fn parse_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let cleaned: String = t.chars().filter(|c| *c != ',').collect();
    if let Some(p) = cleaned.strip_suffix('%') {
        return p.parse::<f64>().ok().map(|v| v / 100.0);
    }
    cleaned.parse::<f64>().ok()
}

fn numeric_compare(col: &ColumnInfo, op: Op, n: f64) -> (String, Vec<Value>) {
    let expr = numeric_expr(col);
    let sql = if op == Op::Ne {
        format!("({expr} IS NULL OR {expr} <> ?)")
    } else {
        format!("{expr} {} ?", op.sql())
    };
    (sql, vec![Value::Double(n)])
}

/// Builds a SQL predicate (with `?` parameters) for one Excel criterion on one column.
pub fn criterion_sql(col: &ColumnInfo, criterion: &Scalar) -> (String, Vec<Value>) {
    match criterion {
        Scalar::Number(n) => numeric_compare(col, Op::Eq, *n),
        Scalar::Bool(b) => {
            if col.kind == ColumnKind::Bool {
                (format!("{} = ?", quote_ident(&col.name)), vec![Value::Boolean(*b)])
            } else {
                ("FALSE".into(), vec![])
            }
        }
        Scalar::Empty => (blank_expr(col), vec![]),
        Scalar::Error(_) => ("FALSE".into(), vec![]),
        Scalar::Text(raw) => {
            let (op, rest) = split_operator(raw);
            if rest.is_empty() {
                return match op {
                    Op::Ne => (format!("NOT {}", blank_expr(col)), vec![]),
                    // "=" and "" both mean "blank".
                    _ => (blank_expr(col), vec![]),
                };
            }
            if let Some(n) = parse_number(rest) {
                if col.kind != ColumnKind::Text {
                    return numeric_compare(col, op, n);
                }
            }
            if matches!(col.kind, ColumnKind::Date | ColumnKind::Timestamp) {
                if let Some(serial) = parse_date_serial(rest) {
                    return numeric_compare(col, op, serial);
                }
            }
            let text = text_expr(col);
            // Text with no cased letters (Korean, digits, symbols) needs no lower(): comparing
            // the raw column keeps the comparison case-insensitive and is much faster.
            let caseless = col.kind == ColumnKind::Text && rest.to_lowercase() == rest.to_uppercase();
            let raw = quote_ident(&col.name);
            match op {
                Op::Eq | Op::Ne => {
                    let (pred, param) = match (wildcard_to_like(rest), caseless) {
                        (Some(pattern), true) => (format!("{raw} LIKE ? ESCAPE '\\'"), Value::Text(pattern)),
                        (Some(pattern), false) => (
                            format!("{text} LIKE lower(?) ESCAPE '\\'"),
                            Value::Text(pattern),
                        ),
                        (None, true) => (format!("{raw} = ?"), Value::Text(unescape_tilde(rest))),
                        (None, false) => (format!("{text} = lower(?)"), Value::Text(unescape_tilde(rest))),
                    };
                    if op == Op::Eq {
                        (pred, vec![param])
                    } else {
                        let c = quote_ident(&col.name);
                        (format!("({c} IS NULL OR NOT ({pred}))"), vec![param])
                    }
                }
                _ => (
                    format!("{text} {} lower(?)", op.sql()),
                    vec![Value::Text(rest.to_string())],
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, kind: ColumnKind) -> ColumnInfo {
        ColumnInfo { name: name.into(), sql_type: String::new(), kind }
    }

    #[test]
    fn operators_and_numbers() {
        let (sql, p) = criterion_sql(&col("amount", ColumnKind::Number), &Scalar::Text(">=100".into()));
        assert_eq!(sql, "CAST(\"amount\" AS DOUBLE) >= ?");
        assert_eq!(p, vec![Value::Double(100.0)]);
    }

    #[test]
    fn wildcards_become_like() {
        let (sql, p) = criterion_sql(&col("name", ColumnKind::Text), &Scalar::Text("*카드*".into()));
        assert!(sql.contains("LIKE"));
        assert_eq!(p, vec![Value::Text("%카드%".into())]);
        let (_, p) = criterion_sql(&col("name", ColumnKind::Text), &Scalar::Text("a~*b".into()));
        assert_eq!(p, vec![Value::Text("a*b".into())]);
    }

    #[test]
    fn not_equal_includes_blanks() {
        let (sql, _) = criterion_sql(&col("status", ColumnKind::Text), &Scalar::Text("<>취소".into()));
        assert!(sql.starts_with("(\"status\" IS NULL OR NOT"));
    }

    #[test]
    fn date_text_is_a_serial() {
        assert_eq!(parse_date_serial("2026-01-01"), Some(46023.0));
        assert_eq!(parse_date_serial("1900-03-01"), Some(61.0));
        let (_, p) = criterion_sql(&col("d", ColumnKind::Date), &Scalar::Text(">=2026-01-01".into()));
        assert_eq!(p, vec![Value::Double(46023.0)]);
    }

    #[test]
    fn rejects_impossible_gregorian_dates() {
        for date in ["2026-02-29", "2026-02-31", "2026-04-31", "2026-13-01", "0000-01-01"] {
            assert_eq!(parse_date_serial(date), None, "{date}");
        }
        assert!(parse_date_serial("2024-02-29").is_some());
        assert!(parse_date_serial("2000-02-29").is_some());
        assert_eq!(parse_date_serial("1900-02-29"), None);
    }
}
