//! Cell values shared by the calculation engine bridge and the data engine.

use serde::{Deserialize, Serialize};

/// Days between 1899-12-30 (Excel serial day 0) and 1970-01-01 (Unix epoch).
pub const EXCEL_EPOCH_OFFSET_DAYS: f64 = 25569.0;

/// A scalar value as the spreadsheet sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "v")]
pub enum Scalar {
    Empty,
    Number(f64),
    Text(String),
    Bool(bool),
    /// An Excel error literal such as `#N/A` or `#DIV/0!`.
    Error(String),
}

impl Scalar {
    /// Renders the value as a literal that can be embedded in an Excel formula.
    pub fn to_formula_literal(&self) -> String {
        match self {
            Scalar::Empty => "0".to_string(),
            Scalar::Number(n) => number_literal(*n),
            Scalar::Text(s) => format!("\"{}\"", s.replace('"', "\"\"")),
            Scalar::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
            Scalar::Error(e) => e.clone(),
        }
    }

    /// Text used when the value is a criterion or a lookup key.
    pub fn as_criterion_text(&self) -> String {
        match self {
            Scalar::Empty => String::new(),
            Scalar::Number(n) => number_text(*n),
            Scalar::Text(s) => s.clone(),
            Scalar::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
            Scalar::Error(e) => e.clone(),
        }
    }

    /// Display text for grids and CLI output.
    pub fn display(&self) -> String {
        match self {
            Scalar::Empty => String::new(),
            Scalar::Number(n) => number_text(*n),
            Scalar::Text(s) => s.clone(),
            Scalar::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
            Scalar::Error(e) => e.clone(),
        }
    }
}

/// Shortest text that round-trips the number, without exponent noise for integers.
pub fn number_text(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn number_literal(n: f64) -> String {
    if !n.is_finite() {
        return "#NUM!".to_string();
    }
    let text = number_text(n);
    // Parenthesise negatives so the literal is safe inside any expression (e.g. `2^x`).
    if n < 0.0 {
        format!("({text})")
    } else {
        text
    }
}

/// Converts a DuckDB value into a spreadsheet scalar. Dates and timestamps become
/// Excel serial numbers so that date formulas keep working on the result.
pub fn from_duckdb(value: duckdb::types::Value) -> Scalar {
    use duckdb::types::{TimeUnit, Value as V};
    fn unit_to_seconds(unit: TimeUnit, v: i64) -> f64 {
        match unit {
            TimeUnit::Second => v as f64,
            TimeUnit::Millisecond => v as f64 / 1e3,
            TimeUnit::Microsecond => v as f64 / 1e6,
            TimeUnit::Nanosecond => v as f64 / 1e9,
        }
    }
    match value {
        V::Null => Scalar::Empty,
        V::Boolean(b) => Scalar::Bool(b),
        V::TinyInt(i) => Scalar::Number(i as f64),
        V::SmallInt(i) => Scalar::Number(i as f64),
        V::Int(i) => Scalar::Number(i as f64),
        V::BigInt(i) => Scalar::Number(i as f64),
        V::HugeInt(i) => Scalar::Number(i as f64),
        V::UHugeInt(i) => Scalar::Number(i as f64),
        V::UTinyInt(i) => Scalar::Number(i as f64),
        V::USmallInt(i) => Scalar::Number(i as f64),
        V::UInt(i) => Scalar::Number(i as f64),
        V::UBigInt(i) => Scalar::Number(i as f64),
        V::Float(f) => Scalar::Number(f as f64),
        V::Double(f) => Scalar::Number(f),
        V::Decimal(d) => d
            .to_string()
            .parse::<f64>()
            .map(Scalar::Number)
            .unwrap_or_else(|_| Scalar::Text(d.to_string())),
        V::Timestamp(unit, v) => {
            Scalar::Number(unit_to_seconds(unit, v) / 86400.0 + EXCEL_EPOCH_OFFSET_DAYS)
        }
        V::Date32(days) => Scalar::Number(days as f64 + EXCEL_EPOCH_OFFSET_DAYS),
        V::Time64(unit, v) => Scalar::Number(unit_to_seconds(unit, v) / 86400.0),
        V::Text(s) => Scalar::Text(s),
        V::Enum(s) => Scalar::Text(s),
        other => Scalar::Text(format!("{other:?}")),
    }
}

/// Converts an IronCalc cell value into a scalar.
pub fn from_ironcalc(value: ironcalc_base::cell::CellValue) -> Scalar {
    use ironcalc_base::cell::CellValue;
    match value {
        CellValue::None => Scalar::Empty,
        CellValue::Number(n) => Scalar::Number(n),
        CellValue::String(s) => {
            if s.starts_with('#') && is_error_literal(&s) {
                Scalar::Error(s)
            } else {
                Scalar::Text(s)
            }
        }
        CellValue::Boolean(b) => Scalar::Bool(b),
    }
}

pub fn is_error_literal(s: &str) -> bool {
    matches!(
        s,
        "#N/A" | "#DIV/0!" | "#VALUE!" | "#REF!" | "#NAME?" | "#NUM!" | "#NULL!" | "#CALC!"
            | "#SPILL!" | "#CIRC!" | "#ERROR!"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_are_formula_safe() {
        assert_eq!(Scalar::Number(-3.5).to_formula_literal(), "(-3.5)");
        assert_eq!(Scalar::Number(42.0).to_formula_literal(), "42");
        assert_eq!(Scalar::Text("a\"b".into()).to_formula_literal(), "\"a\"\"b\"");
        assert_eq!(Scalar::Error("#N/A".into()).to_formula_literal(), "#N/A");
    }

    #[test]
    fn dates_become_excel_serials() {
        // 2026-01-01 is 20454 days after the Unix epoch and Excel serial 46023.
        assert_eq!(
            from_duckdb(duckdb::types::Value::Date32(20454)),
            Scalar::Number(46023.0)
        );
    }
}
