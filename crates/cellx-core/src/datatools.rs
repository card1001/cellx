//! Excel's data tools on regular sheets: sort, AutoFilter, text to columns, remove
//! duplicates, data validation, flash fill, replace, defined names, hyperlinks and
//! goal seek.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use ironcalc_base::types::{Link, Style};
use serde::{Deserialize, Serialize};

use crate::error::{CellxError, Result};
use crate::extras::parse_range;
use crate::objects::range_a1;
use crate::refs;
use crate::value::Scalar;
use crate::workbook::{CellRange, RecalcStats, Workbook};

// ---------------------------------------------------------------- types

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct SortKey {
    /// Absolute column on the sheet.
    pub col: i32,
    pub descending: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterOp {
    Equal,
    NotEqual,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    Contains,
    NotContains,
    BeginsWith,
    EndsWith,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FilterCondition {
    pub op: FilterOp,
    pub value: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ColumnFilter {
    pub col: i32,
    /// Shown texts to keep (`""` stands for blanks). `None`: every value.
    pub values: Option<Vec<String>>,
    pub conditions: Vec<FilterCondition>,
    /// Two conditions: both (`true`) or either (`false`).
    #[serde(default = "yes")]
    pub and: bool,
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AutoFilter {
    pub sheet: u32,
    /// Header row and data, e.g. A1:F200.
    pub range: CellRange,
    pub columns: Vec<ColumnFilter>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct FilterValue {
    pub text: String,
    pub checked: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationKind {
    Any,
    WholeNumber,
    Decimal,
    List,
    Date,
    Time,
    TextLength,
    Custom,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationOp {
    Between,
    NotBetween,
    Equal,
    NotEqual,
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorStyle {
    Stop,
    Warning,
    Information,
}

/// A data validation rule, stored like xlsx `dataValidation`: formulas without `=`,
/// list literals quoted (`"서울,부산"`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Validation {
    pub sheet: u32,
    /// Space-separated ranges (xlsx `sqref`).
    pub range: String,
    pub kind: ValidationKind,
    pub op: ValidationOp,
    pub formula1: String,
    pub formula2: String,
    pub allow_blank: bool,
    pub in_cell_dropdown: bool,
    pub show_input: bool,
    pub input_title: String,
    pub input_message: String,
    pub show_error: bool,
    pub error_style: ErrorStyle,
    pub error_title: String,
    pub error_message: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ValidationFailure {
    pub style: ErrorStyle,
    pub title: String,
    pub message: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SplitOptions {
    /// Delimiter characters (tab, `;`, `,`, space, or any other).
    pub delimiters: String,
    /// Treat consecutive delimiters as one.
    pub consecutive: bool,
    /// Text qualifier, usually `"`.
    pub qualifier: Option<char>,
    /// Fixed width: break positions in characters (overrides delimiters).
    pub widths: Option<Vec<usize>>,
    pub dest_row: i32,
    pub dest_col: i32,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct DuplicateResult {
    pub removed: usize,
    pub remaining: usize,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct NameView {
    pub name: String,
    pub scope: Option<u32>,
    pub formula: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct LinkView {
    pub target: String,
    pub internal: bool,
    pub tooltip: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct GoalSeekResult {
    pub found: bool,
    pub value: f64,
    pub result: f64,
    pub iterations: usize,
}

// ---------------------------------------------------------------- helpers

/// Excel's sort order: numbers, text (case-insensitive), FALSE, TRUE, errors; blanks last.
fn sort_rank(v: &Scalar) -> u8 {
    match v {
        Scalar::Number(_) => 0,
        Scalar::Text(_) => 1,
        Scalar::Bool(_) => 2,
        Scalar::Error(_) => 3,
        Scalar::Empty => 4,
    }
}

fn compare_values(a: &Scalar, b: &Scalar, descending: bool) -> Ordering {
    // Blanks stay at the bottom in both directions.
    match (a, b) {
        (Scalar::Empty, Scalar::Empty) => return Ordering::Equal,
        (Scalar::Empty, _) => return Ordering::Greater,
        (_, Scalar::Empty) => return Ordering::Less,
        _ => {}
    }
    let o = match (a, b) {
        (Scalar::Number(x), Scalar::Number(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Scalar::Text(x), Scalar::Text(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
        (Scalar::Bool(x), Scalar::Bool(y)) => x.cmp(y),
        (Scalar::Error(x), Scalar::Error(y)) => x.cmp(y),
        _ => sort_rank(a).cmp(&sort_rank(b)),
    };
    if descending {
        o.reverse()
    } else {
        o
    }
}

fn number_of(text: &str) -> Option<f64> {
    let t = text.trim().replace(',', "");
    if let Some(p) = t.strip_suffix('%') {
        return p.trim().parse::<f64>().ok().map(|v| v / 100.0);
    }
    t.parse::<f64>().ok()
}

/// Excel serial of a typed date (`2026-11-22`, `2026.11.22`, `2026/11/22`).
pub fn date_serial(text: &str) -> Option<f64> {
    let t = text.trim().trim_end_matches('.');
    let parts: Vec<&str> = t.split(['-', '.', '/']).map(str::trim).collect();
    if parts.len() != 3 {
        return None;
    }
    let (y, m, d): (i64, i64, i64) = (parts[0].parse().ok()?, parts[1].parse().ok()?, parts[2].parse().ok()?);
    if !(1900..=9999).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Days from civil (Howard Hinnant), then shift to Excel's 1899-12-30 epoch.
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some((days + 25569) as f64)
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    // Excel wildcards: * any run, ? one character, ~ escapes.
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    fn go(p: &[char], t: &[char]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some('*') => (0..=t.len()).any(|i| go(&p[1..], &t[i..])),
            Some('?') => !t.is_empty() && go(&p[1..], &t[1..]),
            Some('~') if p.len() > 1 => t.first() == Some(&p[1]) && go(&p[2..], &t[1..]),
            Some(c) => t.first() == Some(c) && go(&p[1..], &t[1..]),
        }
    }
    go(&p, &t)
}

fn condition_holds(c: &FilterCondition, shown: &str, value: &Scalar) -> bool {
    let num = match value {
        Scalar::Number(n) => Some(*n),
        _ => None,
    };
    let want = number_of(&c.value).or_else(|| date_serial(&c.value));
    let text = shown.to_lowercase();
    let v = c.value.to_lowercase();
    let cmp = |f: fn(f64, f64) -> bool| match (num, want) {
        (Some(a), Some(b)) => f(a, b),
        _ => false,
    };
    match c.op {
        FilterOp::Equal => {
            if v.contains(['*', '?']) {
                wildcard_match(&v, &text)
            } else {
                match (num, want) {
                    (Some(a), Some(b)) => (a - b).abs() < 1e-9,
                    _ => text == v,
                }
            }
        }
        FilterOp::NotEqual => !condition_holds(&FilterCondition { op: FilterOp::Equal, value: c.value.clone() }, shown, value),
        FilterOp::GreaterThan => cmp(|a, b| a > b),
        FilterOp::GreaterThanOrEqual => cmp(|a, b| a >= b),
        FilterOp::LessThan => cmp(|a, b| a < b),
        FilterOp::LessThanOrEqual => cmp(|a, b| a <= b),
        FilterOp::Contains => text.contains(&v),
        FilterOp::NotContains => !text.contains(&v),
        FilterOp::BeginsWith => text.starts_with(&v),
        FilterOp::EndsWith => text.ends_with(&v),
    }
}

fn column_passes(f: &ColumnFilter, shown: &str, value: &Scalar) -> bool {
    if let Some(vals) = &f.values {
        let key = shown.trim();
        if !vals.iter().any(|v| v.trim().eq_ignore_ascii_case(key) || v.trim() == key) {
            return false;
        }
    }
    if f.conditions.is_empty() {
        return true;
    }
    if f.and {
        f.conditions.iter().all(|c| condition_holds(c, shown, value))
    } else {
        f.conditions.iter().any(|c| condition_holds(c, shown, value))
    }
}

fn op_holds(op: ValidationOp, v: f64, a: Option<f64>, b: Option<f64>) -> bool {
    let (Some(a), b) = (a, b) else { return true };
    match op {
        ValidationOp::Between => b.is_none_or(|b| v >= a.min(b) && v <= a.max(b)),
        ValidationOp::NotBetween => b.is_none_or(|b| v < a.min(b) || v > a.max(b)),
        ValidationOp::Equal => (v - a).abs() < 1e-9,
        ValidationOp::NotEqual => (v - a).abs() >= 1e-9,
        ValidationOp::GreaterThan => v > a,
        ValidationOp::LessThan => v < a,
        ValidationOp::GreaterThanOrEqual => v >= a,
        ValidationOp::LessThanOrEqual => v <= a,
    }
}

/// Parts of `a` not covered by `b`.
pub(crate) fn subtract_range(a: CellRange, b: CellRange) -> Vec<CellRange> {
    let (a, b) = (a.normalized(), b.normalized());
    if b.r1 < a.r0 || b.r0 > a.r1 || b.c1 < a.c0 || b.c0 > a.c1 {
        return vec![a];
    }
    let mut out = Vec::new();
    if b.r0 > a.r0 {
        out.push(CellRange { r0: a.r0, c0: a.c0, r1: b.r0 - 1, c1: a.c1 });
    }
    if b.r1 < a.r1 {
        out.push(CellRange { r0: b.r1 + 1, c0: a.c0, r1: a.r1, c1: a.c1 });
    }
    let (r0, r1) = (a.r0.max(b.r0), a.r1.min(b.r1));
    if b.c0 > a.c0 {
        out.push(CellRange { r0, c0: a.c0, r1, c1: b.c0 - 1 });
    }
    if b.c1 < a.c1 {
        out.push(CellRange { r0, c0: b.c1 + 1, r1, c1: a.c1 });
    }
    out
}

fn sqref_parts(sqref: &str) -> Vec<CellRange> {
    sqref
        .split_whitespace()
        .filter_map(parse_range)
        .map(|(_, r0, c0, r1, c1)| CellRange { r0, c0, r1, c1 })
        .collect()
}

/// Splits one line for text to columns.
pub fn split_text(text: &str, o: &SplitOptions) -> Vec<String> {
    if let Some(widths) = &o.widths {
        let chars: Vec<char> = text.chars().collect();
        let mut out = Vec::new();
        let mut start = 0;
        let mut breaks: Vec<usize> = widths.clone();
        breaks.sort_unstable();
        for b in breaks.into_iter().chain(std::iter::once(chars.len())) {
            let end = b.min(chars.len());
            if end > start || out.is_empty() {
                out.push(chars[start.min(end)..end].iter().collect::<String>().trim().to_string());
            }
            start = end;
        }
        return out;
    }
    let delims: Vec<char> = o.delimiters.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut last_was_delim = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if Some(ch) == o.qualifier {
            if quoted && chars.peek() == Some(&ch) {
                cur.push(ch);
                chars.next();
            } else {
                quoted = !quoted;
            }
            last_was_delim = false;
            continue;
        }
        if !quoted && delims.contains(&ch) {
            if !(o.consecutive && last_was_delim) {
                out.push(std::mem::take(&mut cur));
            }
            last_was_delim = true;
            continue;
        }
        last_was_delim = false;
        cur.push(ch);
    }
    out.push(cur);
    out
}

// ---------------------------------------------------------------- flash fill

#[derive(Clone, Copy, Debug, PartialEq)]
enum Case {
    Same,
    Upper,
    Lower,
    Proper,
}

#[derive(Clone, Debug, PartialEq)]
enum Piece {
    Const(String),
    /// Token of a source column: whole text (`delim` None) or the n-th piece split by `delim`
    /// (negative counts from the end).
    Token { src: usize, delim: Option<char>, index: i32, case: Case },
    /// The first or last n characters.
    Chars { src: usize, from_end: bool, n: usize, case: Case },
}

const FLASH_DELIMS: [char; 10] = [' ', '-', '_', '@', '.', ',', '/', '(', ')', ':'];

fn apply_case(s: &str, c: Case) -> String {
    match c {
        Case::Same => s.to_string(),
        Case::Upper => s.to_uppercase(),
        Case::Lower => s.to_lowercase(),
        Case::Proper => {
            let mut out = String::new();
            let mut start = true;
            for ch in s.chars() {
                if ch.is_alphabetic() {
                    if start {
                        out.extend(ch.to_uppercase());
                    } else {
                        out.extend(ch.to_lowercase());
                    }
                    start = false;
                } else {
                    out.push(ch);
                    start = true;
                }
            }
            out
        }
    }
}

fn eval_piece(p: &Piece, sources: &[String]) -> Option<String> {
    match p {
        Piece::Const(s) => Some(s.clone()),
        Piece::Token { src, delim, index, case } => {
            let s = sources.get(*src)?;
            let tok = match delim {
                None => s.clone(),
                Some(d) => {
                    let toks: Vec<&str> = s.split(*d).filter(|t| !t.is_empty()).collect();
                    let i = if *index < 0 { toks.len() as i32 + index } else { *index };
                    if toks.len() < 2 || i < 0 {
                        return None;
                    }
                    toks.get(i as usize)?.to_string()
                }
            };
            Some(apply_case(&tok, *case))
        }
        Piece::Chars { src, from_end, n, case } => {
            let s: Vec<char> = sources.get(*src)?.chars().collect();
            if s.len() < *n {
                return None;
            }
            let part: String = if *from_end { s[s.len() - n..].iter().collect() } else { s[..*n].iter().collect() };
            Some(apply_case(&part, *case))
        }
    }
}

fn eval_program(prog: &[Piece], sources: &[String]) -> Option<String> {
    let mut out = String::new();
    for p in prog {
        out.push_str(&eval_piece(p, sources)?);
    }
    Some(out)
}

/// Learns a program from examples (source texts → target text).
fn learn(examples: &[(Vec<String>, String)]) -> Option<Vec<Piece>> {
    let (sources, target) = examples.first()?;
    // Candidate pieces with the text they produce on the first example.
    let mut cands: Vec<(Piece, String)> = Vec::new();
    for (i, s) in sources.iter().enumerate() {
        if s.is_empty() {
            continue;
        }
        for case in [Case::Same, Case::Upper, Case::Lower, Case::Proper] {
            cands.push((Piece::Token { src: i, delim: None, index: 0, case }, apply_case(s, case)));
            for d in FLASH_DELIMS {
                let toks: Vec<&str> = s.split(d).filter(|t| !t.is_empty()).collect();
                if toks.len() < 2 {
                    continue;
                }
                for (k, t) in toks.iter().enumerate() {
                    let text = apply_case(t, case);
                    cands.push((Piece::Token { src: i, delim: Some(d), index: k as i32, case }, text.clone()));
                    cands.push((Piece::Token { src: i, delim: Some(d), index: k as i32 - toks.len() as i32, case }, text));
                }
            }
            let chars: Vec<char> = s.chars().collect();
            for n in 1..chars.len().min(8) {
                cands.push((Piece::Chars { src: i, from_end: false, n, case }, apply_case(&chars[..n].iter().collect::<String>(), case)));
                cands.push((Piece::Chars { src: i, from_end: true, n, case }, apply_case(&chars[chars.len() - n..].iter().collect::<String>(), case)));
            }
        }
    }
    cands.retain(|(_, t)| !t.is_empty());
    cands.sort_by_key(|(p, t)| (std::cmp::Reverse(t.chars().count()), matches!(p, Piece::Chars { .. }) as u8));

    let target: Vec<char> = target.chars().collect();
    let mut budget = 50_000usize;
    let mut found = None;
    fn dfs(
        pos: usize,
        target: &[char],
        cands: &[(Piece, String)],
        prog: &mut Vec<Piece>,
        examples: &[(Vec<String>, String)],
        budget: &mut usize,
        found: &mut Option<Vec<Piece>>,
    ) {
        if found.is_some() || *budget == 0 {
            return;
        }
        *budget -= 1;
        if pos == target.len() {
            if examples.iter().skip(1).all(|(s, t)| eval_program(prog, s).as_deref() == Some(t.as_str())) {
                *found = Some(prog.clone());
            }
            return;
        }
        let rest: String = target[pos..].iter().collect();
        for (p, text) in cands {
            if rest.starts_with(text.as_str()) {
                prog.push(p.clone());
                dfs(pos + text.chars().count(), target, cands, prog, examples, budget, found);
                prog.pop();
                if found.is_some() {
                    return;
                }
            }
        }
        // A literal character (separators like ", " or "-").
        let ch = target[pos];
        let merged = matches!(prog.last(), Some(Piece::Const(_)));
        if merged {
            if let Some(Piece::Const(s)) = prog.last_mut() {
                s.push(ch);
            }
        } else {
            prog.push(Piece::Const(ch.to_string()));
        }
        dfs(pos + 1, target, cands, prog, examples, budget, found);
        if merged {
            if let Some(Piece::Const(s)) = prog.last_mut() {
                s.pop();
            }
        } else {
            prog.pop();
        }
    }
    dfs(0, &target, &cands, &mut Vec::new(), examples, &mut budget, &mut found);
    // A program made only of constants just repeats the example: not a pattern.
    found.filter(|p| p.iter().any(|x| !matches!(x, Piece::Const(_))))
}

// ---------------------------------------------------------------- workbook

impl Workbook {
    fn shown(&self, sheet: u32, row: i32, col: i32) -> String {
        self.model().get_formatted_cell_value(sheet, row, col).unwrap_or_default()
    }

    /// Inputs and style indexes of every cell in `r`, row by row.
    fn row_block(&self, sheet: u32, r: CellRange) -> Result<(Vec<Vec<String>>, Vec<Vec<i32>>)> {
        let model = self.model();
        let mut inputs = Vec::with_capacity((r.r1 - r.r0 + 1) as usize);
        let mut styles = Vec::with_capacity(inputs.capacity());
        for row in r.r0..=r.r1 {
            let mut ins = Vec::with_capacity((r.c1 - r.c0 + 1) as usize);
            let mut sts = Vec::with_capacity(ins.capacity());
            for col in r.c0..=r.c1 {
                ins.push(self.input(sheet, row, col)?);
                sts.push(model.get_cell_style_index(sheet, row, col).map_err(CellxError::Calc)?);
            }
            inputs.push(ins);
            styles.push(sts);
        }
        Ok((inputs, styles))
    }

    /// Writes whole rows back in a new order; `rows[i]` holds the original row now at
    /// position i. Formulas move their relative references with the row, as Excel's sort does.
    /// Formatting is rewritten only when some cell actually ends up with a different style.
    /// `before` is what the block held when it was read: cells that end up with the same
    /// entry (a column of row-relative formulas, repeated values) are not rewritten.
    fn write_rows(
        &mut self,
        sheet: u32,
        r: CellRange,
        before: &[Vec<String>],
        rows: &[(i32, Vec<String>, Vec<i32>)],
    ) -> Result<()> {
        let width = (r.c1 - r.c0 + 1) as usize;
        let height = (r.r1 - r.r0 + 1) as usize;
        for i in 0..height {
            let row = r.r0 + i as i32;
            for j in 0..width {
                let col = r.c0 + j as i32;
                let text = match rows.get(i) {
                    Some((orig, ins, _)) if ins[j].starts_with('=') => refs::offset_refs(&ins[j], row - orig, 0),
                    Some((_, ins, _)) => ins[j].clone(),
                    None => String::new(),
                };
                let old = before.get(i).map_or("", |b| b[j].as_str());
                if text == old {
                    continue;
                }
                if text.is_empty() {
                    let one = CellRange { r0: row, c0: col, r1: row, c1: col };
                    self.um.range_clear_contents(&one.area(sheet))?;
                    self.bridges.remove(&(sheet, row, col));
                } else {
                    self.write_input(sheet, row, col, &text, true)?;
                }
            }
        }
        // Rows past the kept ones (remove duplicates) get the default style.
        let wanted = |i: usize, j: usize| rows.get(i).map_or(0, |x| x.2[j]);
        let model = self.model();
        let mut differs = false;
        'scan: for i in 0..height {
            for j in 0..width {
                let now = model
                    .get_cell_style_index(sheet, r.r0 + i as i32, r.c0 + j as i32)
                    .map_err(CellxError::Calc)?;
                if now != wanted(i, j) {
                    differs = true;
                    break 'scan;
                }
            }
        }
        if differs {
            let mut cache: HashMap<i32, Style> = HashMap::new();
            let mut styles: Vec<Vec<Style>> = Vec::with_capacity(height);
            for i in 0..height {
                let mut line = Vec::with_capacity(width);
                for j in 0..width {
                    let idx = wanted(i, j);
                    let style = match cache.get(&idx) {
                        Some(s) => s.clone(),
                        None => {
                            let s = model.workbook.styles.get_style(idx).map_err(CellxError::Calc)?;
                            cache.insert(idx, s.clone());
                            s
                        }
                    };
                    line.push(style);
                }
                styles.push(line);
            }
            self.select(sheet, r)?;
            self.um.on_paste_styles(&styles)?;
        }
        Ok(())
    }

    /// Puts the rows of `r` in a new order: `sources[i]` is the row that ends
    /// up at position i (`None`, or past the end: an emptied row). Whole cells
    /// move, formulas with their relative references, as Excel does.
    fn reorder_rows(&mut self, sheet: u32, r: CellRange, sources: &[Option<i32>]) -> Result<()> {
        if self.move_cells_raw(sheet, r, sources)? {
            return Ok(());
        }
        // Array formulas and data engine formulas go the text way.
        let (inputs, styles) = self.row_block(sheet, r)?;
        let rows: Vec<(i32, Vec<String>, Vec<i32>)> = sources
            .iter()
            .flatten()
            .map(|&src| {
                let i = (src - r.r0) as usize;
                (src, inputs[i].clone(), styles[i].clone())
            })
            .collect();
        self.write_rows(sheet, r, &inputs, &rows)
    }

    /// `reorder_rows` with the cells themselves, as one undoable change.
    /// Returns false, changing nothing, when the block holds an array formula,
    /// a spill or a data engine formula.
    fn move_cells_raw(&mut self, sheet: u32, r: CellRange, sources: &[Option<i32>]) -> Result<bool> {
        use ironcalc_base::types::Cell;
        if self.bridges().keys().any(|(s, row, col)| *s == sheet && r.contains(*row, *col)) {
            return Ok(false);
        }
        let model = self.model();
        let worksheet = model.workbook.worksheet(sheet).map_err(CellxError::Calc)?;
        let width = (r.c1 - r.c0 + 1) as usize;
        // What every cell holds now, and the style it shows (its own, or its row's or column's).
        let mut grid: Vec<(Option<Cell>, i32)> = Vec::with_capacity(width * (r.r1 - r.r0 + 1) as usize);
        for row in r.r0..=r.r1 {
            for col in r.c0..=r.c1 {
                let cell = worksheet.cell(row, col).map(std::borrow::Cow::into_owned);
                if matches!(cell, Some(Cell::ArrayFormula { .. } | Cell::SpillCell { .. })) {
                    return Ok(false);
                }
                let style = model.get_cell_style_index(sheet, row, col).map_err(CellxError::Calc)?;
                grid.push((cell, style));
            }
        }
        let mut changes = Vec::new();
        for i in 0..(r.r1 - r.r0 + 1) as usize {
            for j in 0..width {
                let (here, here_style) = &grid[i * width + j];
                // The text way left emptied rows with the default style, and an
                // empty source cell's style in place.
                let (cell, style) = match sources.get(i).copied().flatten() {
                    Some(src) => {
                        let (c, s) = &grid[(src - r.r0) as usize * width + j];
                        (c.clone(), *s)
                    }
                    None => (None, 0),
                };
                let new = match cell {
                    Some(c) => Some(c),
                    None if here.is_none() && *here_style == style => continue,
                    None => Some(Cell::EmptyCell { s: style }),
                };
                if new.as_ref() != here.as_ref() {
                    changes.push((r.r0 + i as i32, r.c0 + j as i32, new));
                }
            }
        }
        if !changes.is_empty() {
            self.um.set_cells(sheet, changes).map_err(CellxError::Calc)?;
        }
        Ok(true)
    }

    // ------------------------------------------------------------ sort

    /// 정렬: sorts the rows of `range` by `keys` (first key first).
    pub fn sort_range(&mut self, sheet: u32, range: CellRange, keys: &[SortKey], has_header: bool) -> Result<RecalcStats> {
        self.guard(sheet, range, crate::sheetopts::Guard::Sort)?;
        self.guard(sheet, range, crate::sheetopts::Guard::Content)?;
        let mut r = range.normalized();
        if has_header {
            r.r0 += 1;
        }
        let (last_row, _) = self.dimensions(sheet)?;
        r.r1 = r.r1.min(last_row.max(r.r0));
        if r.r1 <= r.r0 || keys.is_empty() {
            return Ok(RecalcStats::default());
        }
        let start = self.begin();
        let mut rows: Vec<(i32, Vec<Scalar>)> = (r.r0..=r.r1)
            .map(|row| (row, keys.iter().map(|k| self.value(sheet, row, k.col).unwrap_or(Scalar::Empty)).collect()))
            .collect();
        // Stable: rows that compare equal keep their order, as in Excel.
        rows.sort_by(|a, b| {
            for (i, k) in keys.iter().enumerate() {
                let o = compare_values(&a.1[i], &b.1[i], k.descending);
                if o != Ordering::Equal {
                    return o;
                }
            }
            Ordering::Equal
        });
        let order: Vec<Option<i32>> = rows.iter().map(|(row, _)| Some(*row)).collect();
        self.reorder_rows(sheet, r, &order)?;
        let out = self.commit(start)?;
        if self.filters.iter().any(|f| f.sheet == sheet) {
            self.apply_filter(sheet)?;
        }
        Ok(out)
    }

    // ------------------------------------------------------------ AutoFilter

    pub fn auto_filter(&self, sheet: u32) -> Option<AutoFilter> {
        self.filters.iter().find(|f| f.sheet == sheet).cloned()
    }

    /// Ctrl+Shift+L: turns filter buttons on for `range` (the current region when it is
    /// one cell), or off with every row shown again.
    pub fn toggle_auto_filter(&mut self, sheet: u32, range: CellRange) -> Result<Option<AutoFilter>> {
        self.guard(sheet, range, crate::sheetopts::Guard::AutoFilter)?;
        let start = self.begin();
        if let Some(i) = self.filters.iter().position(|f| f.sheet == sheet) {
            let f = self.filters.remove(i);
            if f.range.r1 > f.range.r0 {
                self.um.set_rows_hidden(sheet, f.range.r0 + 1, f.range.r1, false)?;
            }
            self.commit(start)?;
            return Ok(None);
        }
        let mut r = range.normalized();
        if r.r0 == r.r1 && r.c0 == r.c1 {
            r = self.current_region(sheet, r.r0, r.c0)?;
        }
        let f = AutoFilter { sheet, range: r, columns: Vec::new() };
        self.filters.push(f.clone());
        self.commit(start)?;
        Ok(Some(f))
    }

    /// The filter range grown to rows typed below it since it was made (Excel's "다시 적용").
    fn grow_filter(&mut self, sheet: u32) -> Result<()> {
        let Some(i) = self.filters.iter().position(|f| f.sheet == sheet) else { return Ok(()) };
        let r = self.filters[i].range;
        let mut r1 = r.r1;
        loop {
            let next = r1 + 1;
            if next > refs::LAST_ROW || (r.c0..=r.c1).all(|c| self.input(sheet, next, c).unwrap_or_default().is_empty()) {
                break;
            }
            r1 = next;
        }
        self.filters[i].range.r1 = r1;
        Ok(())
    }

    pub fn set_filter_column(&mut self, sheet: u32, filter: ColumnFilter) -> Result<RecalcStats> {
        self.guard(sheet, CellRange { r0: 1, c0: 1, r1: 1, c1: 1 }, crate::sheetopts::Guard::AutoFilter)?;
        let start = self.begin();
        let f = self
            .filters
            .iter_mut()
            .find(|f| f.sheet == sheet)
            .ok_or_else(|| CellxError::Calc("필터가 없습니다".into()))?;
        f.columns.retain(|c| c.col != filter.col);
        if filter.values.is_some() || !filter.conditions.is_empty() {
            f.columns.push(filter);
        }
        self.grow_filter(sheet)?;
        self.apply_filter_rows(sheet)?;
        self.commit(start)
    }

    /// 지우기: every row shown, the buttons stay.
    pub fn clear_filters(&mut self, sheet: u32) -> Result<RecalcStats> {
        let start = self.begin();
        if let Some(f) = self.filters.iter_mut().find(|f| f.sheet == sheet) {
            f.columns.clear();
        }
        self.apply_filter_rows(sheet)?;
        self.commit(start)
    }

    /// 다시 적용.
    pub fn apply_filter(&mut self, sheet: u32) -> Result<RecalcStats> {
        let start = self.begin();
        self.grow_filter(sheet)?;
        self.apply_filter_rows(sheet)?;
        self.commit(start)
    }

    fn apply_filter_rows(&mut self, sheet: u32) -> Result<()> {
        let Some(f) = self.auto_filter(sheet) else { return Ok(()) };
        let r = f.range;
        let mut run: Option<(i32, i32, bool)> = None;
        let flush = |wb: &mut Workbook, run: (i32, i32, bool)| -> Result<()> {
            wb.um.set_rows_hidden(sheet, run.0, run.1, run.2)?;
            Ok(())
        };
        for row in (r.r0 + 1)..=r.r1 {
            let visible = f.columns.iter().all(|cf| {
                let shown = self.shown(sheet, row, cf.col);
                let value = self.value(sheet, row, cf.col).unwrap_or(Scalar::Empty);
                column_passes(cf, &shown, &value)
            });
            let hidden = !visible;
            let was = self.model().workbook.worksheet(sheet)?.is_row_hidden(row)?;
            if was == hidden {
                if let Some(x) = run.take() {
                    flush(self, x)?;
                }
                continue;
            }
            run = match run {
                Some((a, b, h)) if h == hidden && b + 1 == row => Some((a, row, h)),
                Some(x) => {
                    flush(self, x)?;
                    Some((row, row, hidden))
                }
                None => Some((row, row, hidden)),
            };
        }
        if let Some(x) = run {
            flush(self, x)?;
        }
        Ok(())
    }

    /// The checklist of a filter button: values left by the other columns' filters.
    pub fn filter_values(&mut self, sheet: u32, col: i32) -> Result<Vec<FilterValue>> {
        self.grow_filter(sheet)?;
        let f = self.auto_filter(sheet).ok_or_else(|| CellxError::Calc("필터가 없습니다".into()))?;
        let own = f.columns.iter().find(|c| c.col == col).cloned();
        let mut seen = HashSet::new();
        let mut items: Vec<(Scalar, String)> = Vec::new();
        for row in (f.range.r0 + 1)..=f.range.r1 {
            let others_ok = f.columns.iter().filter(|c| c.col != col).all(|cf| {
                let shown = self.shown(sheet, row, cf.col);
                let value = self.value(sheet, row, cf.col).unwrap_or(Scalar::Empty);
                column_passes(cf, &shown, &value)
            });
            if !others_ok {
                continue;
            }
            let shown = self.shown(sheet, row, col).trim().to_string();
            if seen.insert(shown.to_lowercase()) {
                items.push((self.value(sheet, row, col).unwrap_or(Scalar::Empty), shown));
            }
        }
        items.sort_by(|a, b| compare_values(&a.0, &b.0, false));
        Ok(items
            .into_iter()
            .map(|(_, text)| {
                let checked = match &own {
                    None => true,
                    Some(cf) => cf.values.as_ref().is_none_or(|v| v.iter().any(|x| x.trim().eq_ignore_ascii_case(&text))),
                };
                FilterValue { text, checked }
            })
            .collect())
    }

    // ------------------------------------------------------------ text to columns

    pub fn text_to_columns(&mut self, sheet: u32, range: CellRange, opts: &SplitOptions) -> Result<RecalcStats> {
        let r = range.normalized();
        let start = self.begin();
        for (i, row) in (r.r0..=r.r1).enumerate() {
            let text = self.shown(sheet, row, r.c0);
            if text.is_empty() {
                continue;
            }
            let parts = split_text(&text, opts);
            for (j, p) in parts.iter().enumerate() {
                let (tr, tc) = (opts.dest_row + i as i32, opts.dest_col + j as i32);
                if p.is_empty() {
                    self.um.range_clear_contents(&CellRange { r0: tr, c0: tc, r1: tr, c1: tc }.area(sheet))?;
                } else {
                    self.write_input(sheet, tr, tc, p, true)?;
                }
            }
        }
        self.commit(start)
    }

    // ------------------------------------------------------------ remove duplicates

    pub fn remove_duplicates(&mut self, sheet: u32, range: CellRange, cols: &[i32], has_header: bool) -> Result<DuplicateResult> {
        self.guard(sheet, range, crate::sheetopts::Guard::Content)?;
        let mut r = range.normalized();
        if has_header {
            r.r0 += 1;
        }
        let (last_row, _) = self.dimensions(sheet)?;
        r.r1 = r.r1.min(last_row.max(r.r0));
        if r.r1 < r.r0 {
            return Ok(DuplicateResult { removed: 0, remaining: 0 });
        }
        let cols: Vec<i32> = if cols.is_empty() { (r.c0..=r.c1).collect() } else { cols.to_vec() };
        let start = self.begin();
        let mut seen = HashSet::new();
        let mut kept: Vec<Option<i32>> = Vec::new();
        for row in r.r0..=r.r1 {
            let key: Vec<String> = cols
                .iter()
                .map(|c| match self.value(sheet, row, *c).unwrap_or(Scalar::Empty) {
                    Scalar::Number(n) => format!("n{n}"),
                    Scalar::Text(t) => format!("t{}", t.to_lowercase()),
                    other => format!("o{other:?}"),
                })
                .collect();
            if seen.insert(key) {
                kept.push(Some(row));
            }
        }
        let total = (r.r1 - r.r0 + 1) as usize;
        let result = DuplicateResult { removed: total - kept.len(), remaining: kept.len() };
        if result.removed > 0 {
            self.reorder_rows(sheet, r, &kept)?;
        }
        self.commit(start)?;
        Ok(result)
    }

    // ------------------------------------------------------------ data validation

    pub fn validations(&self, sheet: u32) -> Vec<Validation> {
        self.validations.iter().filter(|v| v.sheet == sheet).cloned().collect()
    }

    pub fn validation_at(&self, sheet: u32, row: i32, col: i32) -> Option<Validation> {
        self.validations
            .iter()
            .find(|v| v.sheet == sheet && sqref_parts(&v.range).iter().any(|p| p.contains(row, col)))
            .cloned()
    }

    fn cut_validations(&mut self, sheet: u32, range: CellRange) {
        for v in self.validations.iter_mut().filter(|v| v.sheet == sheet) {
            let parts: Vec<String> =
                sqref_parts(&v.range).into_iter().flat_map(|p| subtract_range(p, range)).map(range_a1).collect();
            v.range = parts.join(" ");
        }
        self.validations.retain(|v| !v.range.trim().is_empty());
    }

    /// 데이터 유효성 검사: the rule applies to `range`, replacing rules there.
    pub fn set_validation(&mut self, sheet: u32, range: CellRange, mut v: Validation) -> Result<RecalcStats> {
        self.guard(sheet, range, crate::sheetopts::Guard::FormatCells)?;
        let start = self.begin();
        self.cut_validations(sheet, range);
        v.sheet = sheet;
        v.range = range_a1(range);
        if v.kind != ValidationKind::Any {
            self.validations.insert(0, v);
        }
        self.commit(start)
    }

    pub fn clear_validation(&mut self, sheet: u32, range: CellRange) -> Result<RecalcStats> {
        let start = self.begin();
        self.cut_validations(sheet, range);
        self.commit(start)
    }

    /// A formula operand of a rule: a number, a date, a cell reference or a defined name.
    fn operand(&self, sheet: u32, f: &str) -> Option<f64> {
        let f = f.trim().trim_start_matches('=');
        if f.is_empty() {
            return None;
        }
        if let Some(n) = number_of(f).or_else(|| date_serial(f.trim_matches('"'))) {
            return Some(n);
        }
        let (sn, r0, c0, _, _) = parse_range(f)?;
        let s = match sn {
            Some(n) => self.sheet_names().iter().position(|x| x.eq_ignore_ascii_case(&n))? as u32,
            None => sheet,
        };
        match self.value(s, r0, c0).ok()? {
            Scalar::Number(n) => Some(n),
            _ => None,
        }
    }

    /// The choices of a list rule (for the in-cell dropdown).
    pub fn list_items(&self, sheet: u32, row: i32, col: i32) -> Option<Vec<String>> {
        let v = self.validation_at(sheet, row, col)?;
        if v.kind != ValidationKind::List {
            return None;
        }
        self.list_source(sheet, &v.formula1)
    }

    fn list_source(&self, sheet: u32, formula: &str) -> Option<Vec<String>> {
        let f = formula.trim().trim_start_matches('=');
        if let Some(lit) = f.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
            return Some(lit.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect());
        }
        // A defined name stands for its range.
        let target = self
            .um
            .get_defined_name_list()
            .into_iter()
            .find(|(n, scope, _)| n.eq_ignore_ascii_case(f) && scope.is_none_or(|s| s == sheet))
            .map(|(_, _, formula)| formula)
            .unwrap_or_else(|| f.to_string());
        let (sn, r0, c0, r1, c1) = parse_range(target.trim_start_matches('='))?;
        let s = match sn {
            Some(n) => self.sheet_names().iter().position(|x| x.eq_ignore_ascii_case(&n))? as u32,
            None => sheet,
        };
        let (last_row, last_col) = self.dimensions(s).ok()?;
        let mut out = Vec::new();
        for r in r0..=r1.min(last_row.max(r0)) {
            for c in c0..=c1.min(last_col.max(c0)) {
                let t = self.shown(s, r, c);
                if !t.is_empty() && !out.contains(&t) {
                    out.push(t);
                }
            }
        }
        Some(out)
    }

    /// Checks a typed entry against the cell's rule. `None`: allowed.
    pub fn validate_input(&mut self, sheet: u32, row: i32, col: i32, input: &str) -> Option<ValidationFailure> {
        let v = self.validation_at(sheet, row, col)?;
        if !v.show_error || v.kind == ValidationKind::Any || input.starts_with('=') {
            return None;
        }
        if v.kind == ValidationKind::Custom && !input.trim().is_empty() && !self.custom_rule_holds(sheet, row, col, input, &v) {
            return Some(ValidationFailure {
                style: v.error_style,
                title: if v.error_title.is_empty() { "CellX".into() } else { v.error_title.clone() },
                message: if v.error_message.is_empty() {
                    "이 값은 이 셀에 정의된 데이터 유효성 검사 제한과 일치하지 않습니다.".into()
                } else {
                    v.error_message.clone()
                },
            });
        }
        let text = input.trim();
        let ok = if text.is_empty() {
            v.allow_blank
        } else {
            let a = self.operand(sheet, &v.formula1);
            let b = self.operand(sheet, &v.formula2);
            match v.kind {
                ValidationKind::WholeNumber => {
                    number_of(text).is_some_and(|n| n.fract() == 0.0 && op_holds(v.op, n, a, b))
                }
                ValidationKind::Decimal => number_of(text).is_some_and(|n| op_holds(v.op, n, a, b)),
                ValidationKind::Date => {
                    date_serial(text).or_else(|| number_of(text)).is_some_and(|n| op_holds(v.op, n, a, b))
                }
                ValidationKind::Time => number_of(text)
                    .or_else(|| {
                        let (h, m) = text.split_once(':')?;
                        Some((h.trim().parse::<f64>().ok()? + m.trim().parse::<f64>().ok()? / 60.0) / 24.0)
                    })
                    .is_some_and(|n| op_holds(v.op, n, a, b)),
                ValidationKind::TextLength => op_holds(v.op, text.chars().count() as f64, a, b),
                ValidationKind::List => self
                    .list_source(sheet, &v.formula1)
                    .is_none_or(|items| items.iter().any(|i| i.eq_ignore_ascii_case(text))),
                ValidationKind::Any | ValidationKind::Custom => true,
            }
        };
        if ok {
            return None;
        }
        Some(ValidationFailure {
            style: v.error_style,
            title: if v.error_title.is_empty() { "CellX".into() } else { v.error_title.clone() },
            message: if v.error_message.is_empty() {
                "이 값은 이 셀에 정의된 데이터 유효성 검사 제한과 일치하지 않습니다.".into()
            } else {
                v.error_message.clone()
            },
        })
    }

    // ------------------------------------------------------------ flash fill

    /// Ctrl+E: learns the pattern of the values typed in `col` from the columns next to
    /// it and fills the rest of the column. Returns how many cells were filled.
    pub fn flash_fill(&mut self, sheet: u32, row: i32, col: i32) -> Result<(usize, RecalcStats)> {
        let region = self.current_region(sheet, row, col)?;
        let has_any = |wb: &Workbook, r: i32| {
            (region.c0..=region.c1).any(|c| c != col && !wb.input(sheet, r, c).unwrap_or_default().is_empty())
        };
        let mut r0 = row;
        while r0 > 1 && has_any(self, r0 - 1) {
            r0 -= 1;
        }
        let mut r1 = row;
        while r1 < refs::LAST_ROW && has_any(self, r1 + 1) {
            r1 += 1;
        }
        let src_cols: Vec<i32> = (region.c0..=region.c1).filter(|c| *c != col).take(12).collect();
        if src_cols.is_empty() {
            return Err(CellxError::Calc("빠른 채우기: 옆 열에 데이터가 없습니다".into()));
        }
        let sources_of = |wb: &Workbook, r: i32| -> Vec<String> { src_cols.iter().map(|c| wb.shown(sheet, r, *c)).collect() };
        let mut examples: Vec<(i32, Vec<String>, String)> = Vec::new();
        for r in r0..=r1 {
            let t = self.shown(sheet, r, col);
            if !t.is_empty() {
                examples.push((r, sources_of(self, r), t));
            }
        }
        if examples.is_empty() {
            return Err(CellxError::Calc("빠른 채우기: 먼저 결과의 예를 한두 개 입력하세요".into()));
        }
        let as_pairs = |ex: &[(i32, Vec<String>, String)]| ex.iter().map(|(_, s, t)| (s.clone(), t.clone())).collect::<Vec<_>>();
        let mut prog = learn(&as_pairs(&examples));
        if prog.is_none() && examples.len() > 1 && examples[0].0 == r0 {
            // The first row is probably a header.
            examples.remove(0);
            r0 += 1;
            prog = learn(&as_pairs(&examples));
        }
        let prog = prog.ok_or_else(|| CellxError::Calc("빠른 채우기: 패턴을 찾지 못했습니다. 예를 하나 더 입력해 보세요".into()))?;
        let first_example = examples.first().map(|e| e.0).unwrap_or(r0);
        let start = self.begin();
        let mut filled = 0;
        for r in first_example..=r1 {
            if !self.input(sheet, r, col)?.is_empty() {
                continue;
            }
            let src = sources_of(self, r);
            if src.iter().all(|s| s.is_empty()) {
                continue;
            }
            if let Some(out) = eval_program(&prog, &src) {
                let text = if out.starts_with('=') || out.chars().next().is_some_and(|c| c.is_ascii_digit()) && number_of(&out).is_none() {
                    format!("'{out}")
                } else {
                    out
                };
                self.write_input(sheet, r, col, &text, true)?;
                filled += 1;
            }
        }
        let stats = self.commit(start)?;
        Ok((filled, stats))
    }

    // ------------------------------------------------------------ replace

    /// 모두 바꾸기 on one sheet (or every sheet). Works on cell inputs, formulas included.
    pub fn replace_all(
        &mut self,
        sheet: Option<u32>,
        find: &str,
        replace: &str,
        match_case: bool,
        whole_cell: bool,
    ) -> Result<(usize, RecalcStats)> {
        if find.is_empty() {
            return Ok((0, RecalcStats::default()));
        }
        let sheets: Vec<u32> = match sheet {
            Some(s) => vec![s],
            None => (0..self.sheet_names().len() as u32).collect(),
        };
        let start = self.begin();
        let mut count = 0;
        for s in sheets {
            let cells: Vec<(i32, i32)> = self.model().workbook.worksheet(s)?.sheet_data.cells().map(|(r, c, _)| (r, c)).collect();
            for (r, c) in cells {
                let input = self.input(s, r, c)?;
                if let Some(out) = replace_text(&input, find, replace, match_case, whole_cell) {
                    self.write_input(s, r, c, &out, true)?;
                    count += 1;
                }
            }
        }
        let stats = self.commit(start)?;
        Ok((count, stats))
    }

    /// 바꾸기 on one cell (the one found).
    pub fn replace_in_cell(
        &mut self,
        sheet: u32,
        row: i32,
        col: i32,
        find: &str,
        replace: &str,
        match_case: bool,
        whole_cell: bool,
    ) -> Result<bool> {
        let input = self.input(sheet, row, col)?;
        match replace_text(&input, find, replace, match_case, whole_cell) {
            Some(out) => {
                self.set_input(sheet, row, col, &out)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    // ------------------------------------------------------------ defined names

    pub fn names(&self) -> Vec<NameView> {
        self.um
            .get_defined_name_list()
            .into_iter()
            .map(|(name, scope, formula)| NameView { name, scope, formula })
            .collect()
    }

    pub fn add_name(&mut self, name: &str, scope: Option<u32>, formula: &str) -> Result<()> {
        let f = formula.trim().trim_start_matches('=');
        self.um.is_valid_defined_name(name, scope, f).map_err(|e| CellxError::Calc(format!("이름을 만들 수 없습니다: {e}")))?;
        let start = self.begin();
        self.um.new_defined_name(name, scope, f)?;
        self.commit(start)?;
        Ok(())
    }

    pub fn update_name(&mut self, name: &str, scope: Option<u32>, new_name: &str, new_scope: Option<u32>, formula: &str) -> Result<()> {
        let start = self.begin();
        self.um.update_defined_name(name, scope, new_name, new_scope, formula.trim().trim_start_matches('='))?;
        self.commit(start)?;
        Ok(())
    }

    pub fn delete_name(&mut self, name: &str, scope: Option<u32>) -> Result<()> {
        let start = self.begin();
        self.um.delete_defined_name(name, scope)?;
        self.commit(start)?;
        Ok(())
    }

    // ------------------------------------------------------------ hyperlinks

    pub fn link_at(&self, sheet: u32, row: i32, col: i32) -> Result<Option<LinkView>> {
        Ok(self.um.get_cell_link(sheet, row, col)?.map(|l| match l {
            Link::External { target, tooltip } => LinkView { target, internal: false, tooltip },
            Link::Internal { location, tooltip } => LinkView { target: location, internal: true, tooltip },
        }))
    }

    /// Ctrl+K. `text` replaces the cell's content when given.
    pub fn set_link(&mut self, sheet: u32, row: i32, col: i32, target: &str, internal: bool, text: Option<&str>) -> Result<RecalcStats> {
        let start = self.begin();
        let link = if internal {
            Link::Internal { location: target.trim_start_matches('#').to_string(), tooltip: None }
        } else {
            Link::External { target: target.to_string(), tooltip: None }
        };
        let label = text.filter(|t| !t.is_empty()).map(str::to_string).or_else(|| {
            self.input(sheet, row, col).ok().filter(|s| s.is_empty()).map(|_| target.to_string())
        });
        self.um.set_cell_link(sheet, row, col, link, label.as_deref())?;
        self.commit(start)
    }

    pub fn delete_link(&mut self, sheet: u32, row: i32, col: i32) -> Result<RecalcStats> {
        let start = self.begin();
        self.um.delete_cell_link(sheet, row, col)?;
        self.commit(start)
    }

    // ------------------------------------------------------------ goal seek

    /// 목표값 찾기: changes `changing` until `set_cell` equals `target`.
    pub fn goal_seek(&mut self, sheet: u32, set_cell: (i32, i32), target: f64, changing: (i32, i32)) -> Result<GoalSeekResult> {
        if !self.input(sheet, set_cell.0, set_cell.1)?.starts_with('=') {
            return Err(CellxError::Calc("수식 셀에는 수식이 들어 있어야 합니다".into()));
        }
        let original = self.input(sheet, changing.0, changing.1)?;
        if original.starts_with('=') {
            return Err(CellxError::Calc("값을 바꿀 셀에는 수식이 아닌 값이 있어야 합니다".into()));
        }
        let x0 = number_of(&original).unwrap_or(0.0);
        let f = |wb: &mut Workbook, x: f64| -> Result<f64> {
            wb.model_mut().set_user_input(sheet, changing.0, changing.1, format!("{x}"))?;
            wb.model_mut().evaluate();
            match wb.value(sheet, set_cell.0, set_cell.1)? {
                Scalar::Number(n) => Ok(n - target),
                _ => Ok(f64::NAN),
            }
        };
        let mut a = x0;
        let mut fa = f(self, a)?;
        let mut b = if x0 == 0.0 { 1.0 } else { x0 * 1.01 };
        let mut fb = f(self, b)?;
        let mut iterations = 0;
        let mut found = fa.abs() < 1e-7;
        let mut best = (a, fa);
        while !found && iterations < 100 {
            iterations += 1;
            if !fa.is_finite() || !fb.is_finite() {
                break;
            }
            if fb.abs() < best.1.abs() {
                best = (b, fb);
            }
            if fb.abs() < 1e-7 {
                found = true;
                best = (b, fb);
                break;
            }
            let denom = fb - fa;
            let next = if denom.abs() < 1e-300 { b + (b - a).max(1.0) } else { b - fb * (b - a) / denom };
            a = b;
            fa = fb;
            b = next;
            fb = f(self, b)?;
        }
        if !found && fb.is_finite() && fb.abs() < best.1.abs() {
            best = (b, fb);
        }
        // Back to the original value without history, then one undoable change.
        self.model_mut().set_user_input(sheet, changing.0, changing.1, original.clone())?;
        self.model_mut().evaluate();
        let value = (best.0 * 1e10).round() / 1e10;
        self.set_input(sheet, changing.0, changing.1, &format!("{value}"))?;
        let result = match self.value(sheet, set_cell.0, set_cell.1)? {
            Scalar::Number(n) => n,
            _ => f64::NAN,
        };
        Ok(GoalSeekResult { found: found || (result - target).abs() < 1e-6, value, result, iterations })
    }
}

fn replace_text(input: &str, find: &str, replace: &str, match_case: bool, whole: bool) -> Option<String> {
    if whole {
        let same = if match_case { input == find } else { input.to_lowercase() == find.to_lowercase() };
        return same.then(|| replace.to_string());
    }
    if match_case {
        return input.contains(find).then(|| input.replace(find, replace));
    }
    let lower = input.to_lowercase();
    let needle = find.to_lowercase();
    if !lower.contains(&needle) {
        return None;
    }
    let mut out = String::new();
    let mut i = 0;
    let chars: Vec<char> = input.chars().collect();
    let lchars: Vec<char> = lower.chars().collect();
    let n: Vec<char> = needle.chars().collect();
    if chars.len() != lchars.len() {
        return Some(lower.replace(&needle, replace));
    }
    while i < chars.len() {
        if lchars[i..].starts_with(&n) {
            out.push_str(replace);
            i += n.len();
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn row_entries_stay_ordered() {
        use crate::refs::Axis;
        let mut wb = Workbook::new().unwrap();
        // Out of order on purpose: entries are created from the bottom up.
        for r in (2..=40).rev().step_by(3) {
            wb.set_hidden(0, Axis::Row, r, r, true).unwrap();
        }
        wb.set_row_height(0, 10, 12, 33.0).unwrap();
        let ws = &wb.model().workbook.worksheets[0];
        assert!(ws.rows.windows(2).all(|w| w[0].r < w[1].r));
        assert!(ws.is_row_hidden(40).unwrap() && ws.is_row_hidden(37).unwrap() && !ws.is_row_hidden(39).unwrap());
        assert_eq!(ws.row_height(11).unwrap(), 33.0);
        assert_eq!(ws.row_height(40).unwrap(), 0.0);

        wb.insert_rows(0, 5, 2).unwrap();
        let ws = &wb.model().workbook.worksheets[0];
        assert!(ws.rows.windows(2).all(|w| w[0].r < w[1].r));
        assert!(ws.is_row_hidden(42).unwrap() && !ws.is_row_hidden(40).unwrap());
        assert_eq!(ws.row_height(13).unwrap(), 33.0);
        wb.undo().unwrap();
        let ws = &wb.model().workbook.worksheets[0];
        assert!(ws.is_row_hidden(40).unwrap() && !ws.is_row_hidden(42).unwrap());
        assert_eq!(ws.row_height(11).unwrap(), 33.0);

        wb.delete_rows(0, 1, 3).unwrap();
        wb.undo().unwrap();
        let ws = &wb.model().workbook.worksheets[0];
        assert!(ws.rows.windows(2).all(|w| w[0].r < w[1].r));
        assert!(ws.is_row_hidden(4).unwrap() && ws.is_row_hidden(40).unwrap());
    }

    #[test]
    fn sort_keeps_formats_and_formulas() {
        let mut wb = Workbook::new().unwrap();
        let cells: Vec<(i32, i32, String)> = vec![
            (1, 1, "3".into()), (1, 2, "=A1*10".into()),
            (2, 1, "1".into()), (2, 2, "=A2*10".into()),
            (3, 1, "2".into()), (3, 2, "=A3*10".into()),
        ];
        wb.set_inputs(0, &cells).unwrap();
        // Only the row holding 1 is bold: its format has to travel with it.
        wb.set_style(0, CellRange { r0: 2, c0: 1, r1: 2, c1: 1 }, "font.b", "true").unwrap();
        wb.sort_range(0, CellRange { r0: 1, c0: 1, r1: 3, c1: 2 }, &[SortKey { col: 1, descending: false }], false).unwrap();
        let shown = |wb: &Workbook, r, c| wb.model().get_formatted_cell_value(0, r, c).unwrap();
        assert_eq!((shown(&wb, 1, 1), shown(&wb, 2, 1), shown(&wb, 3, 1)), ("1".into(), "2".into(), "3".into()));
        assert_eq!((shown(&wb, 1, 2), shown(&wb, 3, 2)), ("10".into(), "30".into()));
        assert_eq!(wb.input(0, 1, 2).unwrap(), "=A1*10");
        let bold = |wb: &Workbook, r| wb.model().get_style_for_cell(0, r, 1).unwrap().font.b;
        assert!(bold(&wb, 1) && !bold(&wb, 2) && !bold(&wb, 3));
        wb.undo().unwrap();
        assert_eq!(shown(&wb, 2, 1), "1");
        assert!(bold(&wb, 2) && !bold(&wb, 1));
    }

    use super::*;

    fn book(rows: &[&[&str]]) -> Workbook {
        let mut wb = Workbook::new().unwrap();
        let mut cells = Vec::new();
        for (i, r) in rows.iter().enumerate() {
            for (j, v) in r.iter().enumerate() {
                if !v.is_empty() {
                    cells.push((i as i32 + 1, j as i32 + 1, v.to_string()));
                }
            }
        }
        wb.set_inputs(0, &cells).unwrap();
        wb
    }

    fn col(wb: &Workbook, c: i32, rows: std::ops::RangeInclusive<i32>) -> Vec<String> {
        rows.map(|r| wb.model().get_formatted_cell_value(0, r, c).unwrap()).collect()
    }

    #[test]
    fn sort_moves_rows_and_formulas() {
        let mut wb = book(&[&["이름", "점수", "두배"], &["가", "30", "=B2*2"], &["나", "10", "=B3*2"], &["다", "", "=B4*2"], &["라", "20", "=B5*2"]]);
        let r = CellRange { r0: 1, c0: 1, r1: 5, c1: 3 };
        wb.sort_range(0, r, &[SortKey { col: 2, descending: false }], true).unwrap();
        assert_eq!(col(&wb, 1, 2..=5), ["나", "라", "가", "다"]); // blanks last
        assert_eq!(wb.input(0, 2, 3).unwrap(), "=B2*2");
        assert_eq!(col(&wb, 3, 2..=3), ["20", "40"]);
        wb.sort_range(0, r, &[SortKey { col: 2, descending: true }], true).unwrap();
        assert_eq!(col(&wb, 1, 2..=5), ["가", "라", "나", "다"]);
        wb.undo().unwrap();
        assert_eq!(col(&wb, 1, 2..=5), ["나", "라", "가", "다"]);
    }

    #[test]
    fn autofilter_hides_rows_and_lists_values() {
        let mut wb = book(&[&["지역", "금액"], &["서울", "100"], &["부산", "50"], &["서울", "30"], &["대구", "70"]]);
        let f = wb.toggle_auto_filter(0, CellRange { r0: 1, c0: 1, r1: 1, c1: 1 }).unwrap().unwrap();
        assert_eq!(f.range, CellRange { r0: 1, c0: 1, r1: 5, c1: 2 });
        wb.set_filter_column(0, ColumnFilter { col: 1, values: Some(vec!["서울".into()]), conditions: vec![], and: true }).unwrap();
        let hidden = |wb: &Workbook| -> Vec<i32> { (2..=5).filter(|r| wb.model().workbook.worksheet(0).unwrap().is_row_hidden(*r).unwrap()).collect() };
        assert_eq!(hidden(&wb), vec![3, 5]);
        // Amount list only offers values of 서울 rows.
        let vals = wb.filter_values(0, 2).unwrap();
        assert_eq!(vals.iter().map(|v| v.text.as_str()).collect::<Vec<_>>(), ["30", "100"]);
        wb.set_filter_column(
            0,
            ColumnFilter { col: 2, values: None, conditions: vec![FilterCondition { op: FilterOp::GreaterThan, value: "50".into() }], and: true },
        )
        .unwrap();
        assert_eq!(hidden(&wb), vec![3, 4, 5]);
        wb.clear_filters(0).unwrap();
        assert!(hidden(&wb).is_empty());
        // Rows typed below join the filter range.
        wb.set_input(0, 6, 1, "부산").unwrap();
        wb.set_filter_column(0, ColumnFilter { col: 1, values: Some(vec!["서울".into()]), conditions: vec![], and: true }).unwrap();
        assert_eq!(wb.auto_filter(0).unwrap().range.r1, 6);
        assert!(wb.model().workbook.worksheet(0).unwrap().is_row_hidden(6).unwrap());
        wb.toggle_auto_filter(0, f.range).unwrap();
        assert!(hidden(&wb).is_empty() && wb.auto_filter(0).is_none());
    }

    #[test]
    fn splits_text_like_excel() {
        let o = SplitOptions { delimiters: ",".into(), consecutive: false, qualifier: Some('"'), widths: None, dest_row: 1, dest_col: 1 };
        assert_eq!(split_text(r#"a,"b,c",,d"#, &o), ["a", "b,c", "", "d"]);
        let o2 = SplitOptions { delimiters: " ".into(), consecutive: true, ..o.clone() };
        assert_eq!(split_text("홍  길동", &o2), ["홍", "길동"]);
        let o3 = SplitOptions { widths: Some(vec![4, 6]), ..o.clone() };
        assert_eq!(split_text("2026-11-22", &o3), ["2026", "-1", "1-22"]);
        let mut wb = book(&[&["김,철수,30"], &["이,영희,25"]]);
        wb.text_to_columns(0, CellRange { r0: 1, c0: 1, r1: 2, c1: 1 }, &o).unwrap();
        assert_eq!(col(&wb, 2, 1..=2), ["철수", "영희"]);
        assert_eq!(wb.value(0, 2, 3).unwrap(), Scalar::Number(25.0));
    }

    #[test]
    fn removes_duplicates_keeping_first() {
        let mut wb = book(&[&["a", "1"], &["b", "2"], &["A", "1"], &["c", "3"], &["b", "2"]]);
        let res = wb.remove_duplicates(0, CellRange { r0: 1, c0: 1, r1: 5, c1: 2 }, &[], false).unwrap();
        assert_eq!(res, DuplicateResult { removed: 2, remaining: 3 });
        assert_eq!(col(&wb, 1, 1..=5), ["a", "b", "c", "", ""]);
    }

    #[test]
    fn validation_rules() {
        let mut wb = book(&[&["", "", "", "", "서울"], &["", "", "", "", "부산"]]);
        let base = Validation {
            sheet: 0,
            range: String::new(),
            kind: ValidationKind::WholeNumber,
            op: ValidationOp::Between,
            formula1: "1".into(),
            formula2: "10".into(),
            allow_blank: true,
            in_cell_dropdown: true,
            show_input: false,
            input_title: String::new(),
            input_message: String::new(),
            show_error: true,
            error_style: ErrorStyle::Stop,
            error_title: String::new(),
            error_message: String::new(),
        };
        wb.set_validation(0, CellRange { r0: 1, c0: 1, r1: 10, c1: 1 }, base.clone()).unwrap();
        assert!(wb.validate_input(0, 3, 1, "5").is_none());
        assert!(wb.validate_input(0, 3, 1, "5.5").is_some());
        assert!(wb.validate_input(0, 3, 1, "11").is_some());
        assert!(wb.validate_input(0, 3, 1, "").is_none());
        let list = Validation { kind: ValidationKind::List, formula1: "$E$1:$E$2".into(), ..base.clone() };
        wb.set_validation(0, CellRange { r0: 1, c0: 2, r1: 5, c1: 2 }, list).unwrap();
        assert_eq!(wb.list_items(0, 2, 2).unwrap(), ["서울", "부산"]);
        assert!(wb.validate_input(0, 2, 2, "대구").is_some());
        // Clearing part of a rule keeps the rest.
        wb.clear_validation(0, CellRange { r0: 4, c0: 1, r1: 4, c1: 1 }).unwrap();
        assert!(wb.validation_at(0, 4, 1).is_none());
        assert!(wb.validation_at(0, 5, 1).is_some());
        let lit = Validation { kind: ValidationKind::List, formula1: "\"예,아니요\"".into(), ..base };
        wb.set_validation(0, CellRange { r0: 1, c0: 3, r1: 1, c1: 3 }, lit).unwrap();
        assert_eq!(wb.list_items(0, 1, 3).unwrap(), ["예", "아니요"]);
    }

    #[test]
    fn flash_fill_learns_patterns() {
        let mut wb = book(&[&["hong gildong", "hong@cellx.kr", "Gildong"], &["kim chulsoo", "kim@cellx.kr", ""], &["lee younghee", "lee@a.com", ""]]);
        let (n, _) = wb.flash_fill(0, 1, 3).unwrap();
        assert_eq!(n, 2);
        assert_eq!(col(&wb, 3, 1..=3), ["Gildong", "Chulsoo", "Younghee"]);
        // Joining pieces of two columns.
        let mut wb = book(&[&["서울", "강남구", "서울 강남구"], &["부산", "해운대구", ""]]);
        wb.flash_fill(0, 1, 3).unwrap();
        assert_eq!(wb.input(0, 2, 3).unwrap(), "부산 해운대구");
    }

    #[test]
    fn replace_names_links_goal_seek() {
        let mut wb = book(&[&["사과", "=A1&\"즙\""], &["사과나무", "10"]]);
        let (n, _) = wb.replace_all(Some(0), "사과", "배", false, false).unwrap();
        assert_eq!(n, 2);
        assert_eq!(col(&wb, 2, 1..=1), ["배즙"]);
        wb.add_name("단가", None, "=Sheet1!$B$2").unwrap();
        wb.set_input(0, 3, 1, "=단가*3").unwrap();
        assert_eq!(wb.value(0, 3, 1).unwrap(), Scalar::Number(30.0));
        assert_eq!(wb.names()[0].name, "단가");
        wb.set_link(0, 4, 1, "https://example.com", false, Some("사이트")).unwrap();
        assert_eq!(wb.link_at(0, 4, 1).unwrap().unwrap().target, "https://example.com");
        assert_eq!(wb.input(0, 4, 1).unwrap(), "사이트");
        // B5 = B2^2 - 20: find B2 so that B5 = 5 → 5.
        wb.set_input(0, 5, 2, "=B2*B2-20").unwrap();
        let r = wb.goal_seek(0, (5, 2), 5.0, (2, 2)).unwrap();
        assert!(r.found, "{r:?}");
        assert!((r.value - 5.0).abs() < 1e-6);
        assert!(matches!(wb.value(0, 5, 2).unwrap(), Scalar::Number(n) if (n - 5.0).abs() < 1e-6));
        wb.undo().unwrap();
        assert_eq!(wb.input(0, 2, 2).unwrap(), "10");
    }

    #[test]
    fn filters_and_validations_survive_xlsx() {
        let mut wb = book(&[&["지역", "금액"], &["서울", "100"], &["부산", "50"]]);
        wb.toggle_auto_filter(0, CellRange { r0: 1, c0: 1, r1: 3, c1: 2 }).unwrap();
        wb.set_filter_column(0, ColumnFilter { col: 1, values: Some(vec!["서울".into()]), conditions: vec![], and: true }).unwrap();
        wb.set_filter_column(
            0,
            ColumnFilter { col: 2, values: None, conditions: vec![FilterCondition { op: FilterOp::Contains, value: "0".into() }], and: true },
        )
        .unwrap();
        let v = Validation {
            sheet: 0,
            range: String::new(),
            kind: ValidationKind::List,
            op: ValidationOp::Between,
            formula1: "\"서울,부산\"".into(),
            formula2: String::new(),
            allow_blank: true,
            in_cell_dropdown: true,
            show_input: true,
            input_title: "지역".into(),
            input_message: "목록에서 고르세요".into(),
            show_error: true,
            error_style: ErrorStyle::Warning,
            error_title: String::new(),
            error_message: "목록에 없는 지역입니다".into(),
        };
        wb.set_validation(0, CellRange { r0: 2, c0: 1, r1: 20, c1: 1 }, v.clone()).unwrap();
        let dir = crate::project::new_work_dir().unwrap();
        let path = dir.join("f.xlsx");
        wb.save_xlsx(&path).unwrap();
        let back = Workbook::open_xlsx(&path).unwrap();
        let f = back.auto_filter(0).unwrap();
        assert_eq!(f.range, CellRange { r0: 1, c0: 1, r1: 3, c1: 2 });
        assert_eq!(f.columns.len(), 2);
        assert_eq!(f.columns[1].conditions[0].op, FilterOp::Contains);
        assert!(back.model().workbook.worksheet(0).unwrap().is_row_hidden(3).unwrap());
        assert!(back.names().is_empty());
        let got = back.validation_at(0, 5, 1).unwrap();
        assert_eq!(got, Validation { range: "A2:A20".into(), ..v });
    }

    #[test]
    fn dates_to_serials() {
        assert_eq!(date_serial("2026-01-01"), Some(46023.0));
        assert_eq!(date_serial("1900-03-01"), Some(61.0));
    }
}
