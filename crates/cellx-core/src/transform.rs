//! Typed, validated data-transformation pipelines.
//!
//! This module deliberately produces parameterized SQL instead of accepting raw
//! SQL fragments. The resulting plan can be reviewed before execution and reused
//! by the desktop UI, workflow runtime, and local AI tools.

use serde::{Deserialize, Serialize};

use crate::data::{is_valid_table_name, quote_ident};
use crate::error::{CellxError, Result};

/// A serializable transformation recipe stored in a CellX project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransformPipeline {
    pub source_table: String,
    pub steps: Vec<TransformStep>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransformStep {
    SelectColumns { columns: Vec<String> },
    Filter { predicate: FilterExpr },
    RenameColumn { from: String, to: String },
    CastColumn { column: String, target: CastType },
    Sort { keys: Vec<SortKey> },
    Limit { rows: u64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum FilterExpr {
    Compare {
        column: String,
        operator: CompareOperator,
        value: TransformValue,
    },
    IsNull {
        column: String,
        negated: bool,
    },
    And {
        conditions: Vec<FilterExpr>,
    },
    Or {
        conditions: Vec<FilterExpr>,
    },
    Not {
        expression: Box<FilterExpr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompareOperator {
    Equal,
    NotEqual,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
}

impl CompareOperator {
    fn sql(self) -> &'static str {
        match self {
            Self::Equal => "=",
            Self::NotEqual => "<>",
            Self::GreaterThan => ">",
            Self::GreaterThanOrEqual => ">=",
            Self::LessThan => "<",
            Self::LessThanOrEqual => "<=",
        }
    }
}

/// Values are always bound as parameters; they are never interpolated into SQL.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum TransformValue {
    Text(String),
    Number(f64),
    Boolean(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CastType {
    Text,
    Integer,
    BigInt,
    Double,
    Boolean,
    Date,
    Timestamp,
}

impl CastType {
    fn sql(self) -> &'static str {
        match self {
            Self::Text => "VARCHAR",
            Self::Integer => "INTEGER",
            Self::BigInt => "BIGINT",
            Self::Double => "DOUBLE",
            Self::Boolean => "BOOLEAN",
            Self::Date => "DATE",
            Self::Timestamp => "TIMESTAMP",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortKey {
    pub column: String,
    pub descending: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TransformPlan {
    /// SQL uses positional parameters and contains no user-supplied value literals.
    pub sql: String,
    pub parameters: Vec<TransformValue>,
    /// Visible output columns; the internal row identity is intentionally omitted.
    pub output_columns: Vec<String>,
}

const MAX_PIPELINE_LIMIT: u64 = 100_000_000;

impl TransformPipeline {
    /// Validate the recipe against the source table schema and compile a safe plan.
    pub fn compile(&self, source_columns: &[String]) -> Result<TransformPlan> {
        if !is_valid_table_name(&self.source_table) {
            return Err(CellxError::InvalidName(self.source_table.clone()));
        }
        if source_columns.is_empty() {
            return Err(CellxError::Bridge("원본 데이터에 열이 없습니다".into()));
        }

        let mut columns = source_columns.to_vec();
        ensure_unique_columns(&columns)?;
        let mut sql = format!("SELECT * FROM {}", quote_ident(&self.source_table));
        let mut parameters = Vec::new();

        for step in &self.steps {
            match step {
                TransformStep::SelectColumns { columns: selected } => {
                    validate_column_list(selected, &columns)?;
                    let mut projection = vec![quote_ident("_rid")];
                    projection.extend(selected.iter().map(|name| quote_ident(name)));
                    sql = wrap_select(&sql, &projection);
                    columns = selected.clone();
                }
                TransformStep::Filter { predicate } => {
                    let condition = compile_predicate(predicate, &columns, &mut parameters)?;
                    sql = format!(
                        "SELECT * FROM ({sql}) AS {} WHERE {condition}",
                        quote_ident("_cellx_step")
                    );
                }
                TransformStep::RenameColumn { from, to } => {
                    let index = find_column(&columns, from)?;
                    if to.trim().is_empty() {
                        return Err(CellxError::InvalidName(to.clone()));
                    }
                    if columns.iter().enumerate().any(|(i, name)| {
                        i != index && name.eq_ignore_ascii_case(to)
                    }) {
                        return Err(CellxError::Bridge(format!(
                            "이름을 바꾸면 열 이름이 중복됩니다: {to}"
                        )));
                    }
                    let mut renamed = columns.clone();
                    renamed[index] = to.clone();
                    let projection = identity_projection(&columns, Some((index, to)));
                    sql = wrap_select(&sql, &projection);
                    columns = renamed;
                }
                TransformStep::CastColumn { column, target } => {
                    let index = find_column(&columns, column)?;
                    let projection = cast_projection(&columns, index, *target);
                    sql = wrap_select(&sql, &projection);
                }
                TransformStep::Sort { keys } => {
                    if keys.is_empty() {
                        return Err(CellxError::Bridge("정렬 기준이 비어 있습니다".into()));
                    }
                    let mut order = Vec::with_capacity(keys.len() + 1);
                    for key in keys {
                        let actual = &columns[find_column(&columns, &key.column)?];
                        order.push(format!(
                            "{} {} NULLS LAST",
                            quote_ident(actual),
                            if key.descending { "DESC" } else { "ASC" }
                        ));
                    }
                    // _rid is a stable final tie-breaker for rows with equal sort keys.
                    order.push(format!("{} ASC", quote_ident("_rid")));
                    sql = format!(
                        "SELECT * FROM ({sql}) AS {} ORDER BY {}",
                        quote_ident("_cellx_step"),
                        order.join(", ")
                    );
                }
                TransformStep::Limit { rows } => {
                    if *rows > MAX_PIPELINE_LIMIT {
                        return Err(CellxError::Bridge(format!(
                            "행 제한은 {MAX_PIPELINE_LIMIT} 이하여야 합니다"
                        )));
                    }
                    sql = format!(
                        "SELECT * FROM ({sql}) AS {} LIMIT {rows}",
                        quote_ident("_cellx_step")
                    );
                }
            }
        }

        Ok(TransformPlan {
            sql,
            parameters,
            output_columns: columns,
        })
    }
}

fn wrap_select(sql: &str, projection: &[String]) -> String {
    format!(
        "SELECT {} FROM ({sql}) AS {}",
        projection.join(", "),
        quote_ident("_cellx_step")
    )
}

fn identity_projection(columns: &[String], rename: Option<(usize, &String)>) -> Vec<String> {
    let mut projection = Vec::with_capacity(columns.len() + 1);
    projection.push(quote_ident("_rid"));
    for (index, column) in columns.iter().enumerate() {
        match rename {
            Some((target, name)) if target == index => {
                projection.push(format!(
                    "{} AS {}",
                    quote_ident(column),
                    quote_ident(name)
                ));
            }
            _ => projection.push(quote_ident(column)),
        }
    }
    projection
}

fn cast_projection(columns: &[String], target_index: usize, target: CastType) -> Vec<String> {
    let mut projection = Vec::with_capacity(columns.len() + 1);
    projection.push(quote_ident("_rid"));
    for (index, column) in columns.iter().enumerate() {
        if index == target_index {
            projection.push(format!(
                "CAST({} AS {}) AS {}",
                quote_ident(column),
                target.sql(),
                quote_ident(column)
            ));
        } else {
            projection.push(quote_ident(column));
        }
    }
    projection
}

fn validate_column_list(selected: &[String], available: &[String]) -> Result<()> {
    if selected.is_empty() {
        return Err(CellxError::Bridge("선택할 열이 비어 있습니다".into()));
    }
    ensure_unique_columns(selected)?;
    for column in selected {
        find_column(available, column)?;
    }
    Ok(())
}

fn ensure_unique_columns(columns: &[String]) -> Result<()> {
    for (index, column) in columns.iter().enumerate() {
        if column.trim().is_empty() {
            return Err(CellxError::InvalidName(column.clone()));
        }
        if columns[..index]
            .iter()
            .any(|previous| previous.eq_ignore_ascii_case(column))
        {
            return Err(CellxError::Bridge(format!(
                "열 이름이 중복됩니다: {column}"
            )));
        }
    }
    Ok(())
}

fn find_column(columns: &[String], requested: &str) -> Result<usize> {
    columns
        .iter()
        .position(|column| column.eq_ignore_ascii_case(requested))
        .ok_or_else(|| CellxError::UnknownColumn(requested.to_string()))
}

fn compile_predicate(
    predicate: &FilterExpr,
    columns: &[String],
    parameters: &mut Vec<TransformValue>,
) -> Result<String> {
    match predicate {
        FilterExpr::Compare {
            column,
            operator,
            value,
        } => {
            find_column(columns, column)?;
            if let TransformValue::Number(number) = value {
                if !number.is_finite() {
                    return Err(CellxError::Bridge(
                        "필터 숫자는 유한한 값이어야 합니다".into(),
                    ));
                }
            }
            parameters.push(value.clone());
            Ok(format!(
                "{} {} ?",
                quote_ident(&columns[find_column(columns, column)?]),
                operator.sql()
            ))
        }
        FilterExpr::IsNull { column, negated } => {
            find_column(columns, column)?;
            Ok(format!(
                "{} IS {}NULL",
                quote_ident(&columns[find_column(columns, column)?]),
                if *negated { "NOT " } else { "" }
            ))
        }
        FilterExpr::And { conditions } | FilterExpr::Or { conditions } => {
            if conditions.is_empty() {
                return Err(CellxError::Bridge(
                    "AND/OR 조건은 하나 이상의 항목이 필요합니다".into(),
                ));
            }
            let mut parts = Vec::with_capacity(conditions.len());
            for condition in conditions {
                parts.push(format!(
                    "({})",
                    compile_predicate(condition, columns, parameters)?
                ));
            }
            let operator = if matches!(predicate, FilterExpr::And { .. }) {
                " AND "
            } else {
                " OR "
            };
            Ok(parts.join(operator))
        }
        FilterExpr::Not { expression } => Ok(format!(
            "NOT ({})",
            compile_predicate(expression, columns, parameters)?
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> Vec<String> {
        vec!["region".into(), "amount".into(), "order date".into()]
    }

    #[test]
    fn compiles_a_parameterized_pipeline_and_tracks_schema() {
        let pipeline = TransformPipeline {
            source_table: "orders".into(),
            steps: vec![
                TransformStep::Filter {
                    predicate: FilterExpr::Compare {
                        column: "region".into(),
                        operator: CompareOperator::Equal,
                        value: TransformValue::Text("Seoul' OR 1=1 --".into()),
                    },
                },
                TransformStep::SelectColumns {
                    columns: vec!["region".into(), "amount".into()],
                },
                TransformStep::CastColumn {
                    column: "amount".into(),
                    target: CastType::Double,
                },
                TransformStep::Sort {
                    keys: vec![SortKey {
                        column: "amount".into(),
                        descending: true,
                    }],
                },
                TransformStep::Limit { rows: 100 },
            ],
        };

        let plan = pipeline.compile(&schema()).unwrap();
        assert!(plan.sql.contains("WHERE \"region\" = ?"));
        assert!(!plan.sql.contains("Seoul"));
        assert!(plan.sql.contains("CAST(\"amount\" AS DOUBLE)"));
        assert!(plan.sql.contains("\"_rid\" ASC"));
        assert_eq!(plan.parameters, vec![TransformValue::Text("Seoul' OR 1=1 --".into())]);
        assert_eq!(plan.output_columns, vec!["region", "amount"]);
    }

    #[test]
    fn quotes_unusual_column_names() {
        let pipeline = TransformPipeline {
            source_table: "orders".into(),
            steps: vec![TransformStep::SelectColumns {
                columns: vec!["order date".into()],
            }],
        };
        let plan = pipeline.compile(&schema()).unwrap();
        assert!(plan.sql.contains("\"order date\""));
    }

    #[test]
    fn rejects_unknown_columns() {
        let pipeline = TransformPipeline {
            source_table: "orders".into(),
            steps: vec![TransformStep::SelectColumns {
                columns: vec!["missing".into()],
            }],
        };
        assert!(matches!(
            pipeline.compile(&schema()),
            Err(CellxError::UnknownColumn(_))
        ));
    }

    #[test]
    fn rejects_duplicate_renamed_columns() {
        let pipeline = TransformPipeline {
            source_table: "orders".into(),
            steps: vec![TransformStep::RenameColumn {
                from: "region".into(),
                to: "amount".into(),
            }],
        };
        assert!(pipeline.compile(&schema()).is_err());
    }

    #[test]
    fn rejects_invalid_source_names_and_unbounded_limits() {
        let invalid_source = TransformPipeline {
            source_table: "orders; DROP TABLE orders".into(),
            steps: vec![],
        };
        assert!(invalid_source.compile(&schema()).is_err());

        let oversized = TransformPipeline {
            source_table: "orders".into(),
            steps: vec![TransformStep::Limit {
                rows: MAX_PIPELINE_LIMIT + 1,
            }],
        };
        assert!(oversized.compile(&schema()).is_err());
    }

    #[test]
    fn rejects_empty_boolean_groups_and_non_finite_numbers() {
        let empty = TransformPipeline {
            source_table: "orders".into(),
            steps: vec![TransformStep::Filter {
                predicate: FilterExpr::And { conditions: vec![] },
            }],
        };
        assert!(empty.compile(&schema()).is_err());

        let invalid_number = TransformPipeline {
            source_table: "orders".into(),
            steps: vec![TransformStep::Filter {
                predicate: FilterExpr::Compare {
                    column: "amount".into(),
                    operator: CompareOperator::GreaterThan,
                    value: TransformValue::Number(f64::NAN),
                },
            }],
        };
        assert!(invalid_number.compile(&schema()).is_err());
    }
}
