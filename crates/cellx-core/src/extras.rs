//! Objects that live beside the cells: charts and pivot tables.
//! (Memos are stored in IronCalc's worksheet `comments`.)

use serde::{Deserialize, Serialize};

use crate::refs;
use crate::value::Scalar;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartKind {
    /// Clustered column (세로 막대형).
    Column,
    /// Clustered bar (가로 막대형).
    Bar,
    Line,
    Pie,
    Doughnut,
    Area,
    Scatter,
}

/// Where a chart sits: its top-left cell (1-based), an offset inside it, and its size, in pixels.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Anchor {
    pub row: i32,
    pub col: i32,
    pub dx: f64,
    pub dy: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Chart {
    pub id: u32,
    pub sheet: u32,
    pub kind: ChartKind,
    /// Empty: no title (or the single series name, like Excel).
    pub title: String,
    /// Source range, e.g. `A1:C10` or `Sheet2!A1:C10`.
    pub range: String,
    pub series_in_rows: bool,
    pub legend: bool,
    pub anchor: Anchor,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Series {
    pub name: String,
    /// Absolute reference of the name cell (for xlsx), if any.
    pub name_ref: Option<(i32, i32)>,
    pub values: Vec<Option<f64>>,
    /// Data cells (first, last).
    pub values_ref: ((i32, i32), (i32, i32)),
}

/// What a chart draws, resolved from its range.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ChartData {
    pub title: String,
    pub categories: Vec<String>,
    /// Numeric x values (scatter charts).
    pub x_values: Option<Vec<Option<f64>>>,
    pub categories_ref: Option<((i32, i32), (i32, i32))>,
    pub series: Vec<Series>,
    /// Sheet the data comes from.
    pub sheet: u32,
}

/// A range on a sheet: (sheet name if given, r0, c0, r1, c1).
pub fn parse_range(range: &str) -> Option<(Option<String>, i32, i32, i32, i32)> {
    let spans = refs::ref_spans(&format!("={}", range.trim()));
    let s = spans.first()?;
    Some((s.sheet.clone(), s.r0, s.c0, s.r1, s.c1))
}

fn is_label(v: &Scalar) -> bool {
    matches!(v, Scalar::Text(_) | Scalar::Bool(_))
}

fn number(v: &Scalar) -> Option<f64> {
    match v {
        Scalar::Number(n) => Some(*n),
        _ => None,
    }
}

/// Excel's default: one series per column unless the range is wider than it is tall.
pub fn default_series_in_rows(r0: i32, c0: i32, r1: i32, c1: i32) -> bool {
    (c1 - c0) > (r1 - r0)
}

/// Resolves chart data from a range, finding header rows and columns as Excel does:
/// a first row or column holding text is used for names and categories.
pub fn chart_data(
    kind: ChartKind,
    title: &str,
    sheet: u32,
    (r0, c0, r1, c1): (i32, i32, i32, i32),
    series_in_rows: bool,
    value: impl Fn(i32, i32) -> Scalar,
    text: impl Fn(i32, i32) -> String,
) -> ChartData {
    // Work in a transposed frame when series run along rows: "lines" are series.
    let (lines0, lines1, pos0, pos1) = if series_in_rows { (r0, r1, c0, c1) } else { (c0, c1, r0, r1) };
    let at = |line: i32, pos: i32| if series_in_rows { (line, pos) } else { (pos, line) };
    let get = |line: i32, pos: i32| {
        let (r, c) = at(line, pos);
        value(r, c)
    };
    let corner = get(lines0, pos0);
    let names_present = (lines0..=lines1).any(|l| is_label(&get(l, pos0)));
    let mut categories_present = (pos0..=pos1).skip(1).any(|p| is_label(&get(lines0, p)))
        || (matches!(corner, Scalar::Empty) && names_present && lines1 > lines0);
    if kind == ChartKind::Scatter && lines1 > lines0 {
        // Scatter: the first line holds the x values.
        categories_present = true;
    }
    let first_pos = if names_present { pos0 + 1 } else { pos0 };
    let first_line = if categories_present { lines0 + 1 } else { lines0 };

    let categories: Vec<String>;
    let mut x_values = None;
    let mut categories_ref = None;
    if categories_present {
        categories = (first_pos..=pos1).map(|p| {
            let (r, c) = at(lines0, p);
            text(r, c)
        }).collect();
        if kind == ChartKind::Scatter {
            x_values = Some((first_pos..=pos1).map(|p| number(&get(lines0, p))).collect());
        }
        categories_ref = Some((at(lines0, first_pos), at(lines0, pos1)));
    } else {
        categories = (first_pos..=pos1).map(|i| (i - first_pos + 1).to_string()).collect();
    }

    let mut series = Vec::new();
    for line in first_line..=lines1 {
        let (name, name_ref) = if names_present {
            let (r, c) = at(line, pos0);
            (text(r, c), Some((r, c)))
        } else {
            (format!("계열{}", line - first_line + 1), None)
        };
        series.push(Series {
            name,
            name_ref,
            values: (first_pos..=pos1).map(|p| number(&get(line, p))).collect(),
            values_ref: (at(line, first_pos), at(line, pos1)),
        });
    }
    let title = if !title.is_empty() {
        title.to_string()
    } else if series.len() == 1 && names_present {
        series[0].name.clone()
    } else {
        String::new()
    };
    ChartData { title, categories, x_values, categories_ref, series, sheet }
}

// ---------------------------------------------------------------- pivot tables

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aggregate {
    Sum,
    Count,
    Average,
    Max,
    Min,
}

impl Aggregate {
    pub fn label(self) -> &'static str {
        match self {
            Aggregate::Sum => "합계",
            Aggregate::Count => "개수",
            Aggregate::Average => "평균",
            Aggregate::Max => "최대",
            Aggregate::Min => "최소",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PivotValue {
    pub field: String,
    pub agg: Aggregate,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind")]
pub enum PivotSource {
    /// A range with a header row on a sheet, e.g. `Sheet1!A1:F500`.
    Range { range: String },
    /// A data sheet (DuckDB table).
    Table { name: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PivotDef {
    pub id: u32,
    pub source: PivotSource,
    pub rows: Vec<String>,
    pub columns: Option<String>,
    pub values: Vec<PivotValue>,
    /// Output position.
    pub sheet: u32,
    pub row: i32,
    pub col: i32,
    /// Size of the last output (rows, columns), cleared on refresh.
    #[serde(default)]
    pub extent: (i32, i32),
}
