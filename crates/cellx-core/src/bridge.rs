//! The formula → SQL bridge.
//!
//! A formula such as `=ROUND(SUMIFS(sales[amount], sales[region], B1), 0)` refers to a
//! data sheet (`sales`) that may hold tens of millions of rows. The calculation engine
//! never sees those rows. Instead, every *bridge call* inside the formula
//! (`SUMIFS(sales[...], ...)`) is executed in DuckDB, and the call is replaced by
//! its result before the formula reaches the calculation engine:
//! `=ROUND(1234.5, 0)`.
//!
//! Supported bridge calls (when their ranges are data-sheet columns):
//! SUM, COUNT, COUNTA, AVERAGE, MIN, MAX, SUMIF, COUNTIF, AVERAGEIF,
//! SUMIFS, COUNTIFS, AVERAGEIFS, MINIFS, MAXIFS, XLOOKUP, and `DQUERY(sql)`,
//! which spills a whole query result as a dynamic array.

use duckdb::types::Value;
use serde::{Deserialize, Serialize};

use crate::criteria::{criterion_sql, numeric_expr};
use crate::data::{quote_ident, ColumnInfo, DataStore};
use crate::error::{CellxError, Result};
use crate::value::Scalar;

/// Maximum number of result rows `DQUERY` spills into the sheet.
pub const DQUERY_MAX_ROWS: usize = 10_000;

const AGGREGATES: &[&str] = &[
    "SUM", "COUNT", "COUNTA", "AVERAGE", "MIN", "MAX", "SUMIF", "COUNTIF", "AVERAGEIF", "SUMIFS",
    "COUNTIFS", "AVERAGEIFS", "MINIFS", "MAXIFS", "XLOOKUP",
];

/// A reference to a single cell of the calculation engine, e.g. `Sheet2!$B$4`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CellRef {
    pub sheet: Option<String>,
    pub row: i32,
    pub column: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Term {
    Str(String),
    Num(f64),
    Bool(bool),
    Cell(CellRef),
    /// `table[column]`
    Column { table: String, column: String },
}

/// One argument: terms joined with `&`.
#[derive(Debug, Clone, PartialEq)]
pub struct Arg(pub Vec<Term>);

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeCall {
    pub function: String,
    pub args: Vec<Arg>,
    /// Byte span of the call inside the formula text.
    pub span: (usize, usize),
}

/// Result of a bridge call, ready to be put back into the formula.
#[derive(Debug, Clone, PartialEq)]
pub enum BridgeValue {
    Scalar(Scalar),
    /// A 2-D array (header row first for DQUERY).
    Array(Vec<Vec<Scalar>>),
}

impl BridgeValue {
    pub fn to_formula_literal(&self) -> String {
        match self {
            BridgeValue::Scalar(s) => s.to_formula_literal(),
            BridgeValue::Array(rows) => {
                if rows.is_empty() {
                    return "#CALC!".into();
                }
                let body: Vec<String> = rows
                    .iter()
                    .map(|r| {
                        r.iter()
                            .map(|v| match v {
                                // Array constants accept only constants, not parenthesised negatives.
                                Scalar::Number(n) if *n < 0.0 => crate::value::number_text(*n),
                                Scalar::Empty => "\"\"".to_string(),
                                other => other.to_formula_literal(),
                            })
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .collect();
                format!("{{{}}}", body.join(";"))
            }
        }
    }
}

// ---------------------------------------------------------------- lexing helpers

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.'
}

/// Finds the end (exclusive) of the parenthesised group starting at `open`.
fn matching_paren(text: &str, open: usize) -> Option<usize> {
    let bytes: Vec<(usize, char)> = text[open..].char_indices().collect();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut in_bracket = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        let (off, c) = bytes[i];
        if in_string {
            if c == '"' {
                if i + 1 < bytes.len() && bytes[i + 1].1 == '"' {
                    i += 1;
                } else {
                    in_string = false;
                }
            }
        } else if c == '"' {
            in_string = true;
        } else if c == '[' {
            in_bracket += 1;
        } else if c == ']' {
            in_bracket -= 1;
        } else if in_bracket == 0 && c == '(' {
            depth += 1;
        } else if in_bracket == 0 && c == ')' {
            depth -= 1;
            if depth == 0 {
                return Some(open + off + 1);
            }
        }
        i += 1;
    }
    None
}

/// Finds bridge calls in a formula. `is_table` says whether a name is a data sheet.
pub fn find_calls(formula: &str, is_table: &dyn Fn(&str) -> bool) -> Vec<Result<BridgeCall>> {
    let mut calls = Vec::new();
    let chars: Vec<(usize, char)> = formula.char_indices().collect();
    let mut i = 0;
    let mut in_string = false;
    while i < chars.len() {
        let (off, c) = chars[i];
        if in_string {
            if c == '"' {
                if i + 1 < chars.len() && chars[i + 1].1 == '"' {
                    i += 1;
                } else {
                    in_string = false;
                }
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_string = true;
            i += 1;
            continue;
        }
        let starts_ident = (c.is_alphabetic() || c == '_')
            && (i == 0 || !is_ident_char(chars[i - 1].1));
        if !starts_ident {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() && is_ident_char(chars[j].1) {
            j += 1;
        }
        let name_end = if j < chars.len() { chars[j].0 } else { formula.len() };
        let name = formula[off..name_end].to_ascii_uppercase();
        let name = name.strip_prefix("_XLFN.").unwrap_or(&name).to_string();
        let is_call = j < chars.len() && chars[j].1 == '(';
        if !is_call {
            i = j.max(i + 1);
            continue;
        }
        let is_bridge_name = name == "DQUERY" || AGGREGATES.contains(&name.as_str());
        if !is_bridge_name {
            i = j + 1;
            continue;
        }
        let open = chars[j].0;
        let Some(close) = matching_paren(formula, open) else {
            i = j + 1;
            continue;
        };
        let inner = &formula[open + 1..close - 1];
        let refers_to_table = name == "DQUERY" || mentions_table(inner, is_table);
        if refers_to_table {
            let parsed = parse_args(inner).map(|args| BridgeCall {
                function: name.clone(),
                args,
                span: (off, close),
            });
            calls.push(parsed.map_err(|e| match e {
                CellxError::Bridge(m) => CellxError::Bridge(format!("{name}: {m}")),
                other => other,
            }));
            // Skip past the whole call; nested bridge calls inside are not supported.
            while i < chars.len() && chars[i].0 < close {
                i += 1;
            }
        } else {
            i = j + 1;
        }
    }
    calls
}

fn mentions_table(inner: &str, is_table: &dyn Fn(&str) -> bool) -> bool {
    let mut in_string = false;
    let mut start: Option<usize> = None;
    for (off, c) in inner.char_indices() {
        if c == '"' {
            in_string = !in_string;
            start = None;
            continue;
        }
        if in_string {
            continue;
        }
        if c.is_alphanumeric() || c == '_' {
            if start.is_none() {
                start = Some(off);
            }
        } else {
            if c == '[' {
                if let Some(s) = start {
                    if is_table(&inner[s..off]) {
                        return true;
                    }
                }
            }
            start = None;
        }
    }
    false
}

// ---------------------------------------------------------------- argument parser

struct Parser<'a> {
    s: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<char> {
        self.s[self.pos..].chars().next()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.bump();
        }
    }
    fn err<T>(&self, msg: &str) -> Result<T> {
        Err(CellxError::Bridge(format!("{msg} (위치 {})", self.pos)))
    }

    fn string(&mut self) -> Result<String> {
        self.bump(); // opening quote
        let mut out = String::new();
        loop {
            match self.bump() {
                Some('"') => {
                    if self.peek() == Some('"') {
                        self.bump();
                        out.push('"');
                    } else {
                        return Ok(out);
                    }
                }
                Some(c) => out.push(c),
                None => return self.err("문자열이 닫히지 않았습니다"),
            }
        }
    }

    fn number(&mut self) -> Result<f64> {
        let start = self.pos;
        if matches!(self.peek(), Some('-') | Some('+')) {
            self.bump();
        }
        while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == '.' || c == 'E' || c == 'e')
        {
            let c = self.bump();
            if matches!(c, Some('E') | Some('e')) && matches!(self.peek(), Some('-') | Some('+')) {
                self.bump();
            }
        }
        let text = &self.s[start..self.pos];
        if self.peek() == Some('%') {
            self.bump();
            return text
                .parse::<f64>()
                .map(|v| v / 100.0)
                .or_else(|_| self.err("숫자 형식이 잘못되었습니다"));
        }
        text.parse::<f64>().or_else(|_| self.err("숫자 형식이 잘못되었습니다"))
    }

    fn word(&mut self) -> String {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_alphanumeric() || c == '_' || c == '$' || c == '.') {
            self.bump();
        }
        self.s[start..self.pos].to_string()
    }

    fn quoted_sheet(&mut self) -> Result<String> {
        self.bump();
        let mut out = String::new();
        loop {
            match self.bump() {
                Some('\'') => {
                    if self.peek() == Some('\'') {
                        self.bump();
                        out.push('\'');
                    } else {
                        return Ok(out);
                    }
                }
                Some(c) => out.push(c),
                None => return self.err("시트 이름이 닫히지 않았습니다"),
            }
        }
    }

    fn term(&mut self) -> Result<Term> {
        self.skip_ws();
        match self.peek() {
            Some('"') => Ok(Term::Str(self.string()?)),
            Some(c) if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' => {
                Ok(Term::Num(self.number()?))
            }
            Some('\'') => {
                let sheet = self.quoted_sheet()?;
                if self.bump() != Some('!') {
                    return self.err("시트 참조 뒤에 !가 필요합니다");
                }
                let r = self.word();
                parse_a1(&r)
                    .map(|(row, column)| Term::Cell(CellRef { sheet: Some(sheet), row, column }))
                    .ok_or_else(|| CellxError::Bridge(format!("셀 주소가 잘못되었습니다: {r}")))
            }
            Some(c) if c.is_alphabetic() || c == '_' || c == '$' => {
                let w = self.word();
                match self.peek() {
                    Some('[') => {
                        self.bump();
                        let start = self.pos;
                        while !matches!(self.peek(), Some(']') | None) {
                            self.bump();
                        }
                        let column = self.s[start..self.pos].trim().to_string();
                        if self.bump() != Some(']') {
                            return self.err("]가 필요합니다");
                        }
                        Ok(Term::Column { table: w, column })
                    }
                    Some('!') => {
                        self.bump();
                        let r = self.word();
                        parse_a1(&r)
                            .map(|(row, column)| Term::Cell(CellRef { sheet: Some(w.clone()), row, column }))
                            .ok_or_else(|| CellxError::Bridge(format!("셀 주소가 잘못되었습니다: {r}")))
                    }
                    _ => {
                        let upper = w.to_ascii_uppercase();
                        if upper == "TRUE" || upper == "FALSE" {
                            return Ok(Term::Bool(upper == "TRUE"));
                        }
                        parse_a1(&w)
                            .map(|(row, column)| Term::Cell(CellRef { sheet: None, row, column }))
                            .ok_or_else(|| {
                                CellxError::Bridge(format!(
                                    "지원하지 않는 인수입니다: {w} (문자열, 숫자, 셀 주소, 표[열]만 가능)"
                                ))
                            })
                    }
                }
            }
            Some(c) => self.err(&format!("지원하지 않는 문자입니다: {c}")),
            None => self.err("인수가 비어 있습니다"),
        }
    }

    fn arg(&mut self) -> Result<Arg> {
        let mut terms = vec![self.term()?];
        loop {
            self.skip_ws();
            if self.peek() == Some('&') {
                self.bump();
                terms.push(self.term()?);
            } else {
                return Ok(Arg(terms));
            }
        }
    }
}

fn parse_args(inner: &str) -> Result<Vec<Arg>> {
    let mut p = Parser { s: inner, pos: 0 };
    let mut args = Vec::new();
    p.skip_ws();
    if p.peek().is_none() {
        return Ok(args);
    }
    loop {
        args.push(p.arg()?);
        p.skip_ws();
        match p.bump() {
            Some(',') => continue,
            None => return Ok(args),
            Some(c) => return p.err(&format!("예상하지 못한 문자입니다: {c}")),
        }
    }
}

/// Parses `B4`, `$B$4` into (row, column), both 1-based.
pub fn parse_a1(s: &str) -> Option<(i32, i32)> {
    // Excel permits an optional absolute marker before the column and/or row:
    // A1, $A1, A$1 and $A$1. Reject misplaced or repeated markers.
    let s = s.strip_prefix('$').unwrap_or(s);
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = s.split_at(split);
    let letters = letters.strip_suffix('$').unwrap_or(letters);
    if letters.is_empty()
        || letters.len() > 3
        || !letters.chars().all(|c| c.is_ascii_alphabetic())
        || digits.is_empty()
        || !digits.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let row: i32 = digits.parse().ok()?;
    let mut column = 0i32;
    for c in letters.to_ascii_uppercase().chars() {
        column = column * 26 + (c as i32 - 'A' as i32 + 1);
    }
    (row >= 1 && row <= 1_048_576 && column >= 1 && column <= 16_384)
        .then_some((row, column))
}

// ---------------------------------------------------------------- evaluation

/// Resolves cell references to their current values in the calculation engine.
pub trait CellResolver {
    fn cell_value(&self, cell: &CellRef) -> Scalar;
}

#[derive(Debug, Clone, PartialEq)]
enum Resolved {
    Value(Scalar),
    Column(ColumnInfo, String), // column + canonical table
}

fn resolve_arg(arg: &Arg, store: &mut DataStore, cells: &dyn CellResolver) -> Result<Resolved> {
    if arg.0.len() == 1 {
        if let Term::Column { table, column } = &arg.0[0] {
            let canonical = store
                .canonical_table(table)
                .ok_or_else(|| CellxError::UnknownTable(table.clone()))?;
            let col = store.column(&canonical, column)?;
            return Ok(Resolved::Column(col, canonical));
        }
    }
    let mut values = Vec::new();
    for t in &arg.0 {
        values.push(match t {
            Term::Str(s) => Scalar::Text(s.clone()),
            Term::Num(n) => Scalar::Number(*n),
            Term::Bool(b) => Scalar::Bool(*b),
            Term::Cell(c) => cells.cell_value(c),
            Term::Column { table, column } => {
                return Err(CellxError::Bridge(format!(
                    "{table}[{column}]는 & 연결에 쓸 수 없습니다"
                )))
            }
        });
    }
    if values.len() == 1 {
        return Ok(Resolved::Value(values.remove(0)));
    }
    if let Some(e) = values.iter().find(|v| matches!(v, Scalar::Error(_))) {
        return Ok(Resolved::Value(e.clone()));
    }
    Ok(Resolved::Value(Scalar::Text(
        values.iter().map(|v| v.as_criterion_text()).collect::<String>(),
    )))
}

/// The SQL a bridge call will run, plus its parameters. Also used as the cache key.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledCall {
    pub sql: String,
    pub params: Vec<Value>,
    pub tables: Vec<String>,
    pub kind: CallKind,
    /// Value returned when the query yields NULL / no row.
    pub if_empty: Scalar,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CallKind {
    Scalar,
    Table,
}

impl CompiledCall {
    pub fn cache_key(&self) -> String {
        format!("{}\u{1f}{:?}", self.sql, self.params)
    }
}

fn expect_column(r: Resolved, function: &str, index: usize) -> Result<(ColumnInfo, String)> {
    match r {
        Resolved::Column(c, t) => Ok((c, t)),
        Resolved::Value(_) => Err(CellxError::Bridge(format!(
            "{function}의 {}번째 인수는 표[열] 형식이어야 합니다",
            index + 1
        ))),
    }
}

fn expect_value(r: Resolved, function: &str, index: usize) -> Result<Scalar> {
    match r {
        Resolved::Value(v) => Ok(v),
        Resolved::Column(..) => Err(CellxError::Bridge(format!(
            "{function}의 {}번째 인수는 값이어야 합니다",
            index + 1
        ))),
    }
}

/// Compiles a bridge call to SQL, resolving cell references through `cells`.
pub fn compile(call: &BridgeCall, store: &mut DataStore, cells: &dyn CellResolver) -> Result<CompiledCall> {
    let f = call.function.as_str();
    if f == "DQUERY" {
        if call.args.len() != 1 {
            return Err(CellxError::Bridge("DQUERY는 SQL 문자열 하나를 받습니다".into()));
        }
        let sql = match resolve_arg(&call.args[0], store, cells)? {
            Resolved::Value(Scalar::Text(s)) => s,
            _ => return Err(CellxError::Bridge("DQUERY의 인수는 SQL 문자열이어야 합니다".into())),
        };
        store.validate_user_select(&sql)?;
        let tables = store.table_names()?;
        return Ok(CompiledCall {
            sql: format!("SELECT * FROM ({}) AS _q", sql.trim().trim_end_matches(';')),
            params: vec![],
            tables,
            kind: CallKind::Table,
            if_empty: Scalar::Error("#CALC!".into()),
        });
    }

    let mut resolved = Vec::with_capacity(call.args.len());
    for a in &call.args {
        resolved.push(resolve_arg(a, store, cells)?);
    }
    let arity = |min: usize, max: usize| -> Result<()> {
        if resolved.len() < min || resolved.len() > max {
            Err(CellxError::Bridge(format!("{f}의 인수 개수가 맞지 않습니다")))
        } else {
            Ok(())
        }
    };

    let mut table: Option<String> = None;
    let mut use_table = |t: &str| -> Result<()> {
        match &table {
            Some(existing) if existing != t => Err(CellxError::Bridge(format!(
                "{f}의 범위는 모두 같은 데이터 시트에 있어야 합니다 ({existing}, {t})"
            ))),
            _ => {
                table = Some(t.to_string());
                Ok(())
            }
        }
    };

    let mut where_parts: Vec<String> = Vec::new();
    let mut params: Vec<Value> = Vec::new();
    let mut add_criterion = |col: &ColumnInfo, crit: &Scalar| {
        let (sql, mut p) = criterion_sql(col, crit);
        where_parts.push(sql);
        params.append(&mut p);
    };

    let (select, if_empty) = match f {
        "SUM" | "COUNT" | "COUNTA" | "AVERAGE" | "MIN" | "MAX" => {
            arity(1, 1)?;
            let (col, t) = expect_column(resolved.remove(0), f, 0)?;
            use_table(&t)?;
            let num = numeric_expr(&col);
            let c = quote_ident(&col.name);
            match f {
                "SUM" => (format!("SUM({num})"), Scalar::Number(0.0)),
                "COUNT" => (format!("COUNT({num})"), Scalar::Number(0.0)),
                "COUNTA" => (
                    format!("COUNT(CASE WHEN {c} IS NOT NULL AND CAST({c} AS VARCHAR) <> '' THEN 1 END)"),
                    Scalar::Number(0.0),
                ),
                "AVERAGE" => (format!("AVG({num})"), Scalar::Error("#DIV/0!".into())),
                "MIN" => (format!("MIN({num})"), Scalar::Number(0.0)),
                _ => (format!("MAX({num})"), Scalar::Number(0.0)),
            }
        }
        "COUNTIF" | "SUMIF" | "AVERAGEIF" => {
            arity(2, if f == "COUNTIF" { 2 } else { 3 })?;
            let (crit_col, t) = expect_column(resolved.remove(0), f, 0)?;
            use_table(&t)?;
            let crit = expect_value(resolved.remove(0), f, 1)?;
            add_criterion(&crit_col, &crit);
            let target = if resolved.is_empty() {
                crit_col
            } else {
                let (c, t2) = expect_column(resolved.remove(0), f, 2)?;
                use_table(&t2)?;
                c
            };
            match f {
                "COUNTIF" => ("COUNT(*)".to_string(), Scalar::Number(0.0)),
                "SUMIF" => (format!("SUM({})", numeric_expr(&target)), Scalar::Number(0.0)),
                _ => (format!("AVG({})", numeric_expr(&target)), Scalar::Error("#DIV/0!".into())),
            }
        }
        "COUNTIFS" | "SUMIFS" | "AVERAGEIFS" | "MINIFS" | "MAXIFS" => {
            let has_target = f != "COUNTIFS";
            let min = if has_target { 3 } else { 2 };
            if resolved.len() < min || (resolved.len() - usize::from(has_target)) % 2 != 0 {
                return Err(CellxError::Bridge(format!("{f}의 인수 개수가 맞지 않습니다")));
            }
            let target = if has_target {
                let (c, t) = expect_column(resolved.remove(0), f, 0)?;
                use_table(&t)?;
                Some(c)
            } else {
                None
            };
            let offset = usize::from(has_target);
            let mut i = 0;
            while !resolved.is_empty() {
                let (col, t) = expect_column(resolved.remove(0), f, offset + i)?;
                use_table(&t)?;
                let crit = expect_value(resolved.remove(0), f, offset + i + 1)?;
                if let Scalar::Error(e) = &crit {
                    return Ok(constant(Scalar::Error(e.clone())));
                }
                add_criterion(&col, &crit);
                i += 2;
            }
            match (f, target) {
                ("COUNTIFS", _) => ("COUNT(*)".to_string(), Scalar::Number(0.0)),
                ("SUMIFS", Some(c)) => (format!("SUM({})", numeric_expr(&c)), Scalar::Number(0.0)),
                ("AVERAGEIFS", Some(c)) => {
                    (format!("AVG({})", numeric_expr(&c)), Scalar::Error("#DIV/0!".into()))
                }
                ("MINIFS", Some(c)) => (format!("MIN({})", numeric_expr(&c)), Scalar::Number(0.0)),
                (_, Some(c)) => (format!("MAX({})", numeric_expr(&c)), Scalar::Number(0.0)),
                _ => unreachable!(),
            }
        }
        "XLOOKUP" => {
            arity(3, 4)?;
            let key = expect_value(resolved.remove(0), f, 0)?;
            let (key_col, t) = expect_column(resolved.remove(0), f, 1)?;
            use_table(&t)?;
            let (ret_col, t2) = expect_column(resolved.remove(0), f, 2)?;
            use_table(&t2)?;
            let not_found = if resolved.is_empty() {
                Scalar::Error("#N/A".into())
            } else {
                expect_value(resolved.remove(0), f, 3)?
            };
            // XLOOKUP matches exactly: no wildcards, no operators.
            let exact = match &key {
                Scalar::Text(s) => Scalar::Text(format!("={}", s.replace('~', "~~").replace('*', "~*").replace('?', "~?"))),
                other => other.clone(),
            };
            add_criterion(&key_col, &exact);
            let t = table.clone().unwrap_or_default();
            let sql = format!(
                "SELECT {c} FROM {tbl} WHERE {w} ORDER BY _rid LIMIT 1",
                c = quote_ident(&ret_col.name),
                tbl = quote_ident(&t),
                w = where_parts.join(" AND ")
            );
            return Ok(CompiledCall {
                sql,
                params,
                tables: vec![t],
                kind: CallKind::Scalar,
                if_empty: not_found,
            });
        }
        other => return Err(CellxError::Bridge(format!("{other}는 데이터 시트에서 지원하지 않습니다"))),
    };

    let t = table.ok_or_else(|| CellxError::Bridge(format!("{f}에 데이터 시트 범위가 없습니다")))?;
    let where_clause = if where_parts.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", where_parts.join(" AND "))
    };
    Ok(CompiledCall {
        sql: format!("SELECT {select} FROM {}{where_clause}", quote_ident(&t)),
        params,
        tables: vec![t],
        kind: CallKind::Scalar,
        if_empty,
    })
}

fn constant(v: Scalar) -> CompiledCall {
    CompiledCall {
        sql: String::new(),
        params: vec![],
        tables: vec![],
        kind: CallKind::Scalar,
        if_empty: v,
    }
}

/// Runs a compiled call.
pub fn execute(compiled: &CompiledCall, store: &DataStore) -> Result<BridgeValue> {
    if compiled.sql.is_empty() {
        return Ok(BridgeValue::Scalar(compiled.if_empty.clone()));
    }
    match compiled.kind {
        CallKind::Scalar => {
            let v = store.scalar(&compiled.sql, &compiled.params)?;
            Ok(BridgeValue::Scalar(if v == Scalar::Empty {
                compiled.if_empty.clone()
            } else {
                v
            }))
        }
        CallKind::Table => {
            let (names, rows, truncated) = store.rows(&compiled.sql, &compiled.params, DQUERY_MAX_ROWS)?;
            if truncated {
                return Err(CellxError::Bridge(format!(
                    "DQUERY 결과가 {DQUERY_MAX_ROWS}행을 넘습니다. 집계하거나 LIMIT를 지정하세요"
                )));
            }
            let mut out = vec![names.into_iter().map(Scalar::Text).collect::<Vec<_>>()];
            out.extend(rows);
            Ok(BridgeValue::Array(out))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoCells;
    impl CellResolver for NoCells {
        fn cell_value(&self, c: &CellRef) -> Scalar {
            if c.row == 1 && c.column == 2 {
                Scalar::Text("서울".into())
            } else if c.row == 2 && c.column == 2 {
                Scalar::Number(46023.0) // 2026-01-01
            } else {
                Scalar::Empty
            }
        }
    }

    fn store() -> DataStore {
        let mut s = DataStore::open_in_memory().unwrap();
        s.create_table_from_sql(
            "sales",
            "SELECT * FROM (VALUES
                (1, '서울', 100.0::DOUBLE, DATE '2025-12-31', '정상'),
                (2, '부산', 250.0, DATE '2026-02-01', '취소'),
                (3, '서울', 300.0, DATE '2026-03-10', '정상'),
                (4, '대구', NULL,  DATE '2026-03-11', '정상'),
                (5, '서울특별시', 50.0, DATE '2026-04-01', NULL)
             ) AS t(order_id, region, amount, order_date, status)",
        )
        .unwrap();
        s
    }

    fn run(formula: &str) -> BridgeValue {
        let mut s = store();
        let calls = find_calls(formula, &|n| n.eq_ignore_ascii_case("sales"));
        assert_eq!(calls.len(), 1, "{formula}");
        let call = calls.into_iter().next().unwrap().unwrap();
        let compiled = compile(&call, &mut s, &NoCells).unwrap();
        execute(&compiled, &s).unwrap()
    }

    fn num(v: BridgeValue) -> f64 {
        match v {
            BridgeValue::Scalar(Scalar::Number(n)) => n,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn finds_calls_inside_regular_formulas() {
        let f = "=ROUND(SUMIFS(sales[amount], sales[region], \"서울\"), 0) + SUM(A1:A3)";
        let calls = find_calls(f, &|n| n == "sales");
        assert_eq!(calls.len(), 1);
        let c = calls[0].as_ref().unwrap();
        assert_eq!(&f[c.span.0..c.span.1], "SUMIFS(sales[amount], sales[region], \"서울\")");
        // SUM(A1:A3) is a normal range and stays with the calculation engine.
        assert!(find_calls("=SUM(A1:A3)", &|n| n == "sales").is_empty());
    }

    #[test]
    fn aggregates() {
        assert_eq!(num(run("=SUM(sales[amount])")), 700.0);
        assert_eq!(num(run("=COUNT(sales[amount])")), 4.0);
        assert_eq!(num(run("=COUNTA(sales[status])")), 4.0);
        assert_eq!(num(run("=AVERAGE(sales[amount])")), 175.0);
        assert_eq!(num(run("=MAX(sales[amount])")), 300.0);
    }

    #[test]
    fn criteria_functions() {
        assert_eq!(num(run("=SUMIFS(sales[amount], sales[region], B1)")), 400.0);
        assert_eq!(num(run("=SUMIFS(sales[amount], sales[region], \"서울*\")")), 450.0);
        assert_eq!(num(run("=COUNTIFS(sales[status], \"<>취소\")")), 4.0);
        assert_eq!(num(run("=SUMIFS(sales[amount], sales[order_date], \">=\"&B2)")), 600.0);
        assert_eq!(num(run("=COUNTIF(sales[amount], \">=250\")")), 2.0);
        assert_eq!(num(run("=SUMIF(sales[region], \"부산\", sales[amount])")), 250.0);
        assert_eq!(num(run("=MINIFS(sales[amount], sales[region], \"서울\")")), 100.0);
        assert_eq!(
            run("=AVERAGEIFS(sales[amount], sales[region], \"제주\")"),
            BridgeValue::Scalar(Scalar::Error("#DIV/0!".into()))
        );
    }

    #[test]
    fn xlookup_exact_match() {
        assert_eq!(
            run("=XLOOKUP(3, sales[order_id], sales[region])"),
            BridgeValue::Scalar(Scalar::Text("서울".into()))
        );
        assert_eq!(
            run("=XLOOKUP(99, sales[order_id], sales[region])"),
            BridgeValue::Scalar(Scalar::Error("#N/A".into()))
        );
        assert_eq!(
            run("=XLOOKUP(\"서울*\", sales[region], sales[order_id], \"없음\")"),
            BridgeValue::Scalar(Scalar::Text("없음".into()))
        );
    }

    #[test]
    fn dquery_spills_with_header() {
        let v = run("=DQUERY(\"SELECT region, SUM(amount) AS total FROM sales GROUP BY 1 ORDER BY 1\")");
        match &v {
            BridgeValue::Array(rows) => {
                assert_eq!(rows[0], vec![Scalar::Text("region".into()), Scalar::Text("total".into())]);
                assert_eq!(rows.len(), 5);
            }
            other => panic!("{other:?}"),
        }
        assert!(v.to_formula_literal().starts_with("{\"region\",\"total\";"));
    }

    #[test]
    fn mixed_tables_are_rejected() {
        let mut s = store();
        s.create_table_from_sql("other", "SELECT 1 AS x").unwrap();
        let call = find_calls("=SUMIFS(sales[amount], other[x], 1)", &|_| true)
            .remove(0)
            .unwrap();
        assert!(compile(&call, &mut s, &NoCells).is_err());
    }

    #[test]
    fn parses_only_valid_a1_absolute_reference_forms() {
        for address in ["A1", "$A1", "A$1", "$A$1", "XFD1048576"] {
            assert!(parse_a1(address).is_some(), "{address}");
        }
        for address in ["", "A0", "XFE1", "A1048577", "A$$1", "$$A1", "A$-1", "A1x", "1A"] {
            assert_eq!(parse_a1(address), None, "{address}");
        }
    }
}
