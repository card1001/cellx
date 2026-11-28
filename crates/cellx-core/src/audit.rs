//! 수식 분석: 참조되는 셀 추적, 참조하는 셀 추적, 수식 계산.

use serde::Serialize;

use crate::error::Result;
use crate::workbook::Workbook;

/// A range a trace arrow points at or comes from.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct TraceArea {
    pub sheet: u32,
    pub row1: i32,
    pub col1: i32,
    pub row2: i32,
    pub col2: i32,
}

/// One step of 수식 계산. `text` marks the part evaluated next with
/// `\u{2}` … `\u{3}`; `value` is what it comes to.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct EvalStep {
    pub text: String,
    pub value: String,
}

/// One formula cell in a bounded dependency-graph snapshot.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct FormulaNode {
    pub id: String,
    pub sheet: u32,
    pub row: i32,
    pub col: i32,
    pub formula: String,
}

/// A directed edge from a formula precedent to the formula that consumes it.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct FormulaEdge {
    pub from: String,
    pub to: String,
}

/// A bounded snapshot backed by the calculation engine's dependency index.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct FormulaDependencyGraph {
    pub nodes: Vec<FormulaNode>,
    pub edges: Vec<FormulaEdge>,
    pub truncated: bool,
}

impl Workbook {
    /// Returns a bounded formula dependency graph using IronCalc's parsed
    /// dependency index. Range precedents are expanded only up to the edge cap.
    pub fn formula_dependency_graph(&self) -> Result<FormulaDependencyGraph> {
        use std::collections::HashMap;

        const MAX_NODES: usize = 5_000;
        const MAX_EDGES: usize = 20_000;
        let mut nodes = Vec::new();
        let mut truncated = false;
        let cells = self.model().get_all_cells();

        // Inspect one formula beyond the output cap so an exactly-full graph
        // is not incorrectly reported as truncated.
        for cell in cells {
            if let Some(formula) = self.model().get_cell_formula(cell.index, cell.row, cell.column)? {
                if nodes.len() == MAX_NODES {
                    truncated = true;
                    break;
                }
                nodes.push(FormulaNode {
                    id: format!("{}!{}:{}", cell.index, cell.row, cell.column),
                    sheet: cell.index,
                    row: cell.row,
                    col: cell.column,
                    formula,
                });
            }
        }

        let node_ids = nodes.iter().map(|node| ((node.sheet, node.row, node.col), node.id.clone()))
            .collect::<HashMap<_, _>>();
        let mut edges = Vec::new();

        'formula: for node in &nodes {
            for area in self.model().cell_precedents(node.sheet, node.row, node.col) {
                for row in area.row1..=area.row2 {
                    for col in area.column1..=area.column2 {
                        if let Some(from) = node_ids.get(&(area.sheet, row, col)) {
                            if edges.len() >= MAX_EDGES {
                                truncated = true;
                                break 'formula;
                            }
                            edges.push(FormulaEdge { from: from.clone(), to: node.id.clone() });
                        }
                    }
                }
            }
        }

        Ok(FormulaDependencyGraph { nodes, edges, truncated })
    }

    /// The viewport with formulas shown in place of their values (수식 표시,
    /// Ctrl+`): formula cells show their input, left-aligned as text.
    pub fn viewport_formulas(&self, sheet: u32, row0: i32, col0: i32, row1: i32, col1: i32) -> Result<crate::workbook::Viewport> {
        let mut view = self.viewport(sheet, row0, col0, row1, col1)?;
        for cell in &mut view.cells {
            if let Ok(Some(formula)) = self.model().get_cell_formula(sheet, cell.row, cell.col) {
                cell.text = self.input(sheet, cell.row, cell.col).unwrap_or(formula);
                cell.numeric = false;
                cell.error = false;
                cell.general = None;
            }
        }
        Ok(view)
    }

    /// The ranges the formula in a cell reads (참조되는 셀).
    pub fn trace_precedents(&self, sheet: u32, row: i32, col: i32) -> Vec<TraceArea> {
        self.model()
            .cell_precedents(sheet, row, col)
            .into_iter()
            .map(|a| TraceArea { sheet: a.sheet, row1: a.row1, col1: a.column1, row2: a.row2, col2: a.column2 })
            .collect()
    }

    /// The formula cells that read a cell (참조하는 셀).
    pub fn trace_dependents(&self, sheet: u32, row: i32, col: i32) -> Vec<TraceArea> {
        self.model()
            .cell_dependents(sheet, row, col)
            .into_iter()
            .map(|(s, r, c)| TraceArea { sheet: s, row1: r, col1: c, row2: r, col2: c })
            .collect()
    }

    /// 수식 계산: the formula of a cell worked out step by step. Reads values
    /// only, so the next recalculation can still be incremental.
    pub fn formula_steps(&mut self, sheet: u32, row: i32, col: i32) -> Result<Vec<EvalStep>> {
        let steps = self.um.get_model_mut().formula_steps(sheet, row, col)?;
        Ok(steps.into_iter().map(|s| EvalStep { text: s.text, value: s.value }).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_graph_links_formula_precedents() {
        let mut wb = Workbook::new().unwrap();
        wb.set_input(0, 1, 1, "10").unwrap();
        wb.set_input(0, 1, 2, "=A1*2").unwrap();
        wb.set_input(0, 1, 3, "=B1+1").unwrap();

        let graph = wb.formula_dependency_graph().unwrap();
        assert_eq!(graph.nodes.len(), 2);
        assert!(graph.edges.iter().any(|edge| edge.from == "0!1:2" && edge.to == "0!1:3"));
        assert!(!graph.truncated);
    }

    #[test]
    fn dependency_graph_expands_formula_ranges_into_edges() {
        let mut wb = Workbook::new().unwrap();
        wb.set_input(0, 1, 1, "=1+1").unwrap();
        wb.set_input(0, 2, 1, "=2+2").unwrap();
        wb.set_input(0, 1, 2, "=SUM(A1:A2)").unwrap();

        let graph = wb.formula_dependency_graph().unwrap();
        assert_eq!(graph.nodes.len(), 3);
        assert!(graph.edges.iter().any(|edge| edge.from == "0!1:1" && edge.to == "0!1:2"));
        assert!(graph.edges.iter().any(|edge| edge.from == "0!2:1" && edge.to == "0!1:2"));
        assert!(!graph.truncated);
    }

    #[test]
    fn steps_and_traces() {
        let mut wb = Workbook::new().unwrap();
        wb.set_input(0, 1, 1, "3").unwrap();
        wb.set_input(0, 2, 1, "4").unwrap();
        wb.set_input(0, 1, 2, "=IF(A1>2,SUM(A1:A2)*2,1/0)").unwrap();
        wb.set_input(0, 1, 3, "=IFERROR(B1/0,\"x\")&\"!\"").unwrap();
        let steps = wb.formula_steps(0, 1, 2).unwrap();
        let shown: Vec<(&str, &str)> = steps.iter().map(|s| (s.text.as_str(), s.value.as_str())).collect();
        assert_eq!(
            shown,
            vec![
                ("=IF(\u{2}A1\u{3}>2,SUM(A1:A2)*2,1/0)", "3"),
                ("=IF(\u{2}3>2\u{3},SUM(A1:A2)*2,1/0)", "TRUE"),
                ("=IF(TRUE,SUM(\u{2}A1:A2\u{3})*2,1/0)", "{3;4}"),
                ("=IF(TRUE,\u{2}SUM({3;4})\u{3}*2,1/0)", "7"),
                ("=IF(TRUE,\u{2}7*2\u{3},1/0)", "14"),
                ("=\u{2}IF(TRUE,14,1/0)\u{3}", "14"),
                ("=14", "14"),
            ]
        );
        let steps = wb.formula_steps(0, 1, 3).unwrap();
        assert_eq!(steps.last().unwrap().value, "\"x!\"");
        assert!(wb.formula_steps(0, 1, 1).is_err());

        assert_eq!(
            wb.trace_precedents(0, 1, 2),
            vec![
                TraceArea { sheet: 0, row1: 1, col1: 1, row2: 1, col2: 1 },
                TraceArea { sheet: 0, row1: 1, col1: 1, row2: 2, col2: 1 },
            ]
        );
        assert_eq!(wb.trace_dependents(0, 2, 1), vec![TraceArea { sheet: 0, row1: 1, col1: 2, row2: 1, col2: 2 }]);
        let deps = wb.trace_dependents(0, 1, 2);
        assert_eq!(deps, vec![TraceArea { sheet: 0, row1: 1, col1: 3, row2: 1, col2: 3 }]);
        let view = wb.viewport_formulas(0, 1, 1, 2, 3).unwrap();
        let b1 = view.cells.iter().find(|c| c.row == 1 && c.col == 2).unwrap();
        assert_eq!(b1.text, "=IF(A1>2,SUM(A1:A2)*2,1/0)");
        assert!(!b1.numeric);
        assert_eq!(view.cells.iter().find(|c| c.row == 1 && c.col == 1).unwrap().text, "3");
        // Reading steps leaves the next recalculation incremental.
        let stats = wb.set_input(0, 2, 1, "5").unwrap();
        assert!(stats.evaluated.is_some());
    }
}
