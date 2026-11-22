//! CellX: structured references (`표1[금액]`) are kept as written. When a
//! table grows, shrinks, is renamed or goes away, the formulas that use it are
//! worked out again from what they say.

use std::sync::Arc;

use crate::expressions::lexer::LexerMode;
use crate::expressions::parser::static_analysis::run_static_analysis_on_node;
use crate::expressions::parser::stringify::to_rc_format;
use crate::expressions::parser::{Node, StructuredRef};
use crate::expressions::types::CellReferenceRC;
use crate::language::get_default_language;
use crate::locale::get_default_locale;
use crate::model::{build_shared_formula_lookup, Model};

/// Calls `f` on every structured reference in `node`; true if there was one.
fn visit(node: &mut Node, f: &mut dyn FnMut(&mut Option<Box<StructuredRef>>)) -> bool {
    use Node::*;
    match node {
        ReferenceKind { structured, .. } | RangeKind { structured, .. } => {
            let had = structured.is_some();
            if had {
                f(structured);
            }
            had
        }
        OpRangeKind { left, right }
        | OpConcatenateKind { left, right }
        | OpSumKind { left, right, .. }
        | OpProductKind { left, right, .. }
        | OpPowerKind { left, right }
        | CompareKind { left, right, .. } => {
            let a = visit(left, f);
            visit(right, f) || a
        }
        UnaryKind { right, .. } => visit(right, f),
        FunctionKind { args, .. } | NamedFunctionKind { args, .. } => {
            let mut any = false;
            for a in args {
                any |= visit(a, f);
            }
            any
        }
        LambdaCallKind { lambda, args } => {
            let mut any = visit(lambda, f);
            for a in args {
                any |= visit(a, f);
            }
            any
        }
        LambdaDefKind { body, .. } => visit(body, f),
        ImplicitIntersection { child, .. } | SpillRangeOperator { child } => visit(child, f),
        _ => false,
    }
}

impl Model<'_> {
    /// Works the structured references out again against the tables as they
    /// are now. `rename` changes a table's name in them (표 이름 바꾸기);
    /// `drop` turns those of a table that is no longer one into the plain
    /// references they stand for (범위로 변환). Returns how many formulas changed.
    pub fn restructure(&mut self, rename: Option<(&str, &str)>, drop: Option<&str>) -> usize {
        self.refresh_parser_tables();
        let lower = |s: &str| s.to_lowercase();
        let rename = rename.map(|(a, b)| (lower(a), b.to_string()));
        let drop = drop.map(lower);
        let (locale, language) = (self.locale, self.language);
        self.parser.set_locale(get_default_locale());
        self.parser.set_language(get_default_language());
        self.parser.set_lexer_mode(LexerMode::R1C1);
        let mut changed = 0;
        for sheet in 0..self.parsed_formulas.len() {
            let Some(name) = self.workbook.worksheets.get(sheet).map(|w| w.get_name()) else { continue };
            let context = CellReferenceRC { sheet: name, row: 1, column: 1 };
            for i in 0..self.parsed_formulas[sheet].len() {
                let mut node = (*self.parsed_formulas[sheet][i].0).clone();
                let has = visit(&mut node, &mut |s| {
                    let Some(sr) = s else { return };
                    if let Some((from, to)) = &rename {
                        if lower(&sr.table) == *from {
                            sr.table = to.clone();
                        }
                    }
                    if drop.as_deref() == Some(lower(&sr.table).as_str()) {
                        *s = None;
                    }
                });
                if !has {
                    continue;
                }
                let text = to_rc_format(&node);
                let node = self.parser.parse(&text, &context);
                let sheet_formulas = &mut self.workbook.worksheets[sheet].shared_formulas;
                if sheet_formulas[i] == text && *self.parsed_formulas[sheet][i].0 == node {
                    continue;
                }
                sheet_formulas[i] = text;
                let result = run_static_analysis_on_node(&node);
                self.parsed_formulas[sheet][i] = (Arc::new(node), result);
                changed += 1;
            }
        }
        self.parser.set_lexer_mode(LexerMode::A1);
        self.parser.set_locale(locale);
        self.parser.set_language(language);
        if changed > 0 {
            for (sheet, lookup) in self.shared_formula_lookup.iter_mut().enumerate() {
                if let Some(w) = self.workbook.worksheets.get(sheet) {
                    *lookup = build_shared_formula_lookup(&w.shared_formulas);
                }
            }
            self.evaluation.deps = None;
        }
        changed
    }
}
