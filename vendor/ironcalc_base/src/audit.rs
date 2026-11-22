//! CellX: 수식 분석 — which cells a formula reads (참조되는 셀), which formulas
//! read a cell (참조하는 셀), and a formula worked out one step at a time
//! (수식 계산), as Excel's Formula Auditing shows them.

use crate::calc_result::CalcResult;
use crate::dependencies::{collect, DepIndex};
use crate::evaluation::FormulaCell;
use crate::expressions::parser::stringify::to_localized_string;
use crate::expressions::parser::{ArrayNode, Node};
use crate::expressions::types::{CellReferenceIndex, CellReferenceRC};
use crate::functions::Function;
use crate::model::Model;
use crate::number_format::to_excel_precision_str;

/// A rectangle of cells, 1-based and inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditArea {
    pub sheet: u32,
    pub row1: i32,
    pub column1: i32,
    pub row2: i32,
    pub column2: i32,
}

/// One step of 수식 계산: the formula as it stands, with the part evaluated
/// next between `\u{2}` and `\u{3}`, and what that part comes to.
#[derive(Debug, Clone, PartialEq)]
pub struct FormulaStep {
    pub text: String,
    pub value: String,
}

/// Arrays and ranges up to this many values are shown as `{…}` literals.
const MAX_SHOWN: usize = 100;

fn children(node: &Node) -> Vec<&Node> {
    use Node::*;
    match node {
        OpRangeKind { left, right }
        | OpConcatenateKind { left, right }
        | OpSumKind { left, right, .. }
        | OpProductKind { left, right, .. }
        | OpPowerKind { left, right }
        | CompareKind { left, right, .. } => vec![left, right],
        UnaryKind { right, .. } => vec![right],
        FunctionKind { args, .. } | NamedFunctionKind { args, .. } => args.iter().collect(),
        ImplicitIntersection { child, .. } | SpillRangeOperator { child } => vec![child],
        LambdaCallKind { lambda, args } => std::iter::once(&**lambda).chain(args.iter()).collect(),
        _ => Vec::new(),
    }
}

fn child_mut(node: &mut Node, i: usize) -> Option<&mut Node> {
    use Node::*;
    match node {
        OpRangeKind { left, right }
        | OpConcatenateKind { left, right }
        | OpSumKind { left, right, .. }
        | OpProductKind { left, right, .. }
        | OpPowerKind { left, right }
        | CompareKind { left, right, .. } => Some(if i == 0 { left } else { right }),
        UnaryKind { right, .. } => Some(right),
        FunctionKind { args, .. } | NamedFunctionKind { args, .. } => args.get_mut(i),
        ImplicitIntersection { child, .. } | SpillRangeOperator { child } => Some(child),
        LambdaCallKind { lambda, args } => {
            if i == 0 {
                Some(lambda)
            } else {
                args.get_mut(i - 1)
            }
        }
        _ => None,
    }
}

fn at_mut<'n>(node: &'n mut Node, path: &[usize]) -> Option<&'n mut Node> {
    let mut n = node;
    for &i in path {
        n = child_mut(n, i)?;
    }
    Some(n)
}

fn at<'n>(node: &'n Node, path: &[usize]) -> Option<&'n Node> {
    let mut n = node;
    for &i in path {
        n = *children(n).get(i)?;
    }
    Some(n)
}

/// Whether a node is worth a step of its own: anything but a literal.
fn evaluable(node: &Node) -> bool {
    !matches!(
        node,
        Node::BooleanKind(_)
            | Node::NumberKind(_)
            | Node::StringKind(_)
            | Node::ErrorKind(_)
            | Node::EmptyArgKind
            | Node::ArrayKind(_)
            | Node::ImplicitIntersection { automatic: true, .. }
            | Node::LambdaDefKind { .. }
            | Node::NamedVariableKind { .. }
            | Node::ParseErrorKind { .. }
    )
}

impl Model<'_> {
    /// The cells formula (`sheet`, `row`, `column`) reads, as written in it.
    pub fn cell_precedents(&self, sheet: u32, row: i32, column: i32) -> Vec<AuditArea> {
        let Some(node) = self.formula_node(sheet, row, column) else { return Vec::new() };
        let mut refs = Vec::new();
        collect(&node, &mut refs);
        let mut out: Vec<AuditArea> = refs
            .iter()
            .map(|r| {
                let (r1, r2) = (r.row1.at_pub(row), r.row2.at_pub(row));
                let (c1, c2) = (r.column1.at_pub(column), r.column2.at_pub(column));
                AuditArea { sheet: r.sheet, row1: r1.min(r2), column1: c1.min(c2), row2: r1.max(r2), column2: c1.max(c2) }
            })
            .collect();
        out.dedup();
        out
    }

    /// The formulas that read cell (`sheet`, `row`, `column`).
    pub fn cell_dependents(&self, sheet: u32, row: i32, column: i32) -> Vec<(u32, i32, i32)> {
        let mut out = Vec::new();
        let mut push = |k: (u32, i32, i32)| {
            if !out.contains(&k) {
                out.push(k);
            }
        };
        match &self.evaluation.deps {
            Some(d) if d.still_valid(self) => d.dependents((sheet, row, column), &mut push),
            _ => DepIndex::build(self).dependents((sheet, row, column), &mut push),
        }
        out.sort_unstable();
        out
    }

    fn formula_node(&self, sheet: u32, row: i32, column: i32) -> Option<Node> {
        let cell = self.workbook.worksheet(sheet).ok()?.sheet_data.cell(row, column)?;
        let f = FormulaCell::of(&cell)?;
        self.parsed_formulas.get(sheet as usize)?.get(f.f as usize).map(|(n, _)| (**n).clone())
    }

    fn shown(&mut self, result: &CalcResult) -> (String, Option<Node>) {
        let literal = |v: &ArrayNode| match v {
            ArrayNode::Number(n) => to_excel_precision_str(*n),
            ArrayNode::String(s) => format!("\"{s}\""),
            ArrayNode::Boolean(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
            ArrayNode::Error(e) => e.to_string(),
            ArrayNode::Empty => "0".to_string(),
        };
        let array = |rows: &[Vec<ArrayNode>]| {
            let body: Vec<String> = rows.iter().map(|r| r.iter().map(literal).collect::<Vec<_>>().join(",")).collect();
            format!("{{{}}}", body.join(";"))
        };
        match result {
            CalcResult::Number(n) => (to_excel_precision_str(*n), Some(Node::NumberKind(*n))),
            CalcResult::String(s) => (format!("\"{s}\""), Some(Node::StringKind(s.clone()))),
            CalcResult::Boolean(b) => (if *b { "TRUE" } else { "FALSE" }.into(), Some(Node::BooleanKind(*b))),
            CalcResult::Error { error, .. } => (error.to_string(), Some(Node::ErrorKind(error.clone()))),
            CalcResult::EmptyCell | CalcResult::EmptyArg => ("0".into(), Some(Node::NumberKind(0.0))),
            CalcResult::Array(rows) => {
                if rows.iter().map(Vec::len).sum::<usize>() <= MAX_SHOWN {
                    (array(rows), Some(Node::ArrayKind(rows.clone())))
                } else {
                    ("{…}".into(), None)
                }
            }
            CalcResult::Range { left, right } => {
                let count = ((right.row - left.row + 1) as i64 * (right.column - left.column + 1) as i64) as usize;
                if count == 1 {
                    let v = self.evaluate_cell(*left);
                    return self.shown(&v);
                }
                if count > MAX_SHOWN {
                    return ("{…}".into(), None);
                }
                let mut rows = Vec::new();
                for r in left.row..=right.row {
                    let mut line = Vec::new();
                    for c in left.column..=right.column {
                        let v = self.evaluate_cell(CellReferenceIndex { sheet: left.sheet, row: r, column: c });
                        line.push(match v {
                            CalcResult::Number(n) => ArrayNode::Number(n),
                            CalcResult::String(s) => ArrayNode::String(s),
                            CalcResult::Boolean(b) => ArrayNode::Boolean(b),
                            CalcResult::Error { error, .. } => ArrayNode::Error(error),
                            _ => ArrayNode::Empty,
                        });
                    }
                    rows.push(line);
                }
                (array(&rows), Some(Node::ArrayKind(rows)))
            }
            CalcResult::Lambda(_) => ("LAMBDA".into(), None),
        }
    }

    /// The paths of the nodes to evaluate, innermost first, in the order Excel's
    /// 수식 계산 goes: arguments left to right, then the function. IF and
    /// IFERROR evaluate their condition first and then only the branch taken.
    fn plan(&mut self, node: &Node, path: &mut Vec<usize>, cell: CellReferenceIndex, out: &mut Vec<Vec<usize>>) {
        if let Node::FunctionKind { kind: Function::If, args } = node {
            if !args.is_empty() {
                path.push(0);
                self.plan(&args[0], path, cell, out);
                path.pop();
                let taken = match self.evaluate_node_in_context(&args[0], cell) {
                    CalcResult::Boolean(b) => Some(if b { 1 } else { 2 }),
                    CalcResult::Number(n) => Some(if n != 0.0 { 1 } else { 2 }),
                    CalcResult::EmptyCell => Some(2),
                    _ => None,
                };
                if let Some(i) = taken.filter(|i| *i < args.len()) {
                    path.push(i);
                    self.plan(&args[i], path, cell, out);
                    path.pop();
                }
                out.push(path.clone());
                return;
            }
        }
        if let Node::FunctionKind { kind: Function::Iferror | Function::Ifna, args } = node {
            if args.len() == 2 {
                path.push(0);
                self.plan(&args[0], path, cell, out);
                path.pop();
                if matches!(self.evaluate_node_in_context(&args[0], cell), CalcResult::Error { .. }) {
                    path.push(1);
                    self.plan(&args[1], path, cell, out);
                    path.pop();
                }
                out.push(path.clone());
                return;
            }
        }
        for (i, child) in children(node).into_iter().enumerate() {
            path.push(i);
            self.plan(child, path, cell, out);
            path.pop();
        }
        if evaluable(node) {
            out.push(path.clone());
        }
    }

    /// 수식 계산: the formula of a cell, one step at a time. Each step shows the
    /// formula with what was worked out so far filled in and the part done next
    /// marked; the last entry is the result.
    pub fn formula_steps(&mut self, sheet: u32, row: i32, column: i32) -> Result<Vec<FormulaStep>, String> {
        let root = self.formula_node(sheet, row, column).ok_or_else(|| "수식이 없는 셀입니다".to_string())?;
        let cell = CellReferenceIndex { sheet, row, column };
        let context = CellReferenceRC { sheet: self.workbook.worksheet(sheet)?.get_name(), row, column };
        let mut plan = Vec::new();
        self.plan(&root, &mut Vec::new(), cell, &mut plan);
        let (locale, language) = (self.locale, self.language);
        let text = |n: &Node| format!("={}", to_localized_string(n, &context, locale, language));
        const MARK: &str = "\u{1}CELLX\u{1}";
        let mut tree = root;
        let mut steps = Vec::new();
        for path in plan.into_iter().take(500) {
            let Some(node) = at(&tree, &path).cloned() else { continue };
            // Already replaced by a value that stands for it (an outer node of a
            // part that was skipped): nothing left to do here.
            let result = self.evaluate_node_in_context(&node, cell);
            let (value, literal) = self.shown(&result);
            let original = to_localized_string(&node, &context, locale, language);
            let mut marked = tree.clone();
            if let Some(slot) = at_mut(&mut marked, &path) {
                *slot = Node::StringKind(MARK.into());
            }
            let shown = text(&marked).replacen(&format!("\"{MARK}\""), &format!("\u{2}{original}\u{3}"), 1);
            steps.push(FormulaStep { text: shown, value: value.clone() });
            if let (Some(lit), Some(slot)) = (literal, at_mut(&mut tree, &path)) {
                *slot = lit;
            }
            if path.is_empty() {
                // The whole formula: its value is the cell's.
                steps.push(FormulaStep { text: format!("={value}"), value });
                break;
            }
        }
        Ok(steps)
    }
}

impl crate::dependencies::Coord {
    pub(crate) fn at_pub(self, base: i32) -> i32 {
        if self.absolute {
            self.value
        } else {
            self.value + base
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test::util::new_empty_model;

    #[test]
    fn steps_precedents_dependents() {
        let mut model = new_empty_model();
        model.set_user_input(0, 1, 1, "3".into()).unwrap();
        model.set_user_input(0, 2, 1, "4".into()).unwrap();
        model.set_user_input(0, 1, 2, "=IF(A1>2,SUM(A1:A2)*2,1/0)".into()).unwrap();
        model.evaluate();
        let steps = model.formula_steps(0, 1, 2).unwrap();
        let shown: Vec<(String, String)> = steps.iter().map(|s| (s.text.clone(), s.value.clone())).collect();
        assert_eq!(shown[0], ("=IF(\u{2}A1\u{3}>2,SUM(A1:A2)*2,1/0)".into(), "3".into()));
        assert_eq!(shown[1], ("=IF(\u{2}3>2\u{3},SUM(A1:A2)*2,1/0)".into(), "TRUE".into()));
        assert_eq!(shown[2], ("=IF(TRUE,SUM(\u{2}A1:A2\u{3})*2,1/0)".into(), "{3;4}".into()));
        assert_eq!(shown[3].1, "7");
        assert_eq!(shown[4].1, "14");
        assert_eq!(shown.last().unwrap(), &("=14".to_string(), "14".to_string()));
        // The branch not taken (1/0) is never evaluated.
        assert!(shown.iter().all(|(t, _)| !t.contains("\u{2}1/0")));
        assert_eq!(
            model.cell_precedents(0, 1, 2),
            vec![
                super::AuditArea { sheet: 0, row1: 1, column1: 1, row2: 1, column2: 1 },
                super::AuditArea { sheet: 0, row1: 1, column1: 1, row2: 2, column2: 1 },
            ]
        );
        assert_eq!(model.cell_dependents(0, 2, 1), vec![(0, 1, 2)]);
    }
}
