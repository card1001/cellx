//! CellX: within one evaluation pass, the values of a lookup range are read
//! once and an exact-match index is built over them, so that thousands of
//! VLOOKUP/HLOOKUP/MATCH formulas over the same range do not each scan it.
//! The cache is emptied at the start and end of every pass: values only stay
//! put while a pass runs.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use crate::calc_result::CalcResult;
use crate::expressions::types::CellReferenceIndex;
use crate::model::Model;
use crate::number_format::to_excel_precision;

use super::binary_search::binary_search_on_array;
use super::util::{from_wildcard_to_regex, result_matches_regex};

#[derive(Hash, PartialEq, Eq)]
enum Key {
    Number(u64),
    Text(String),
    Bool(bool),
}

fn number_key(n: f64) -> Key {
    // -0 and 0 are the same number.
    Key::Number(if n == 0.0 { 0 } else { n.to_bits() })
}

/// How a lookup function decides that two values are equal.
#[derive(Clone, Copy)]
pub(crate) enum Equality {
    /// `compare_values(x, target) == 0` (VLOOKUP, HLOOKUP): numbers at 15
    /// digits, an empty cell equal to 0 and to FALSE.
    Compare,
    /// `values_are_equal` (MATCH): numbers as they are, empty cells never.
    Equal,
}

pub(crate) struct LookupVector {
    pub(crate) values: Vec<CalcResult>,
    compare: OnceLock<HashMap<Key, usize>>,
    equal: OnceLock<HashMap<Key, usize>>,
}

impl LookupVector {
    fn index(&self, equality: Equality) -> &HashMap<Key, usize> {
        let (cell, compare) = match equality {
            Equality::Compare => (&self.compare, true),
            Equality::Equal => (&self.equal, false),
        };
        cell.get_or_init(|| {
            let mut map = HashMap::with_capacity(self.values.len());
            for (i, v) in self.values.iter().enumerate() {
                let keys: [Option<Key>; 2] = match v {
                    CalcResult::Number(n) if compare => [Some(number_key(to_excel_precision(*n, 15))), None],
                    CalcResult::Number(n) => [Some(number_key(*n)), None],
                    CalcResult::String(s) => [Some(Key::Text(s.to_lowercase())), None],
                    CalcResult::Boolean(b) => [Some(Key::Bool(*b)), None],
                    CalcResult::EmptyCell if compare => [Some(number_key(0.0)), Some(Key::Bool(false))],
                    _ => [None, None],
                };
                for k in keys.into_iter().flatten() {
                    map.entry(k).or_insert(i);
                }
            }
            map
        })
    }

    /// The first position equal to `target`, as the linear search of the
    /// function would find it. Text with wildcards is matched one by one.
    pub(crate) fn find_exact(&self, target: &CalcResult, equality: Equality) -> Option<usize> {
        match target {
            CalcResult::String(s) if s.contains(['*', '?', '~']) => {
                let reg = from_wildcard_to_regex(&s.to_lowercase(), true).ok()?;
                self.values.iter().position(|x| result_matches_regex(x, &reg))
            }
            CalcResult::String(s) => self.index(equality).get(&Key::Text(s.to_lowercase())).copied(),
            CalcResult::Number(n) => {
                let n = match equality {
                    Equality::Compare => to_excel_precision(*n, 15),
                    Equality::Equal => *n,
                };
                self.index(equality).get(&number_key(n)).copied()
            }
            CalcResult::Boolean(b) => self.index(equality).get(&Key::Bool(*b)).copied(),
            CalcResult::EmptyCell => match equality {
                Equality::Compare => self.index(equality).get(&number_key(0.0)).copied(),
                Equality::Equal => None,
            },
            _ => None,
        }
    }

    /// The approximate match of a sorted lookup.
    pub(crate) fn find_sorted(&self, target: &CalcResult) -> Option<usize> {
        match binary_search_on_array(target, &self.values) {
            -2 => None,
            l => Some(l as usize),
        }
    }
}

impl Model<'_> {
    /// The values of the cells from `left` to `right` (one row or one column),
    /// read once per pass.
    pub(crate) fn lookup_vector(
        &mut self,
        left: CellReferenceIndex,
        right: CellReferenceIndex,
    ) -> Arc<LookupVector> {
        let key = (left.sheet, left.row, left.column, right.row, right.column);
        if self.evaluation.in_pass {
            if let Some(v) = self.evaluation.lookup_cache.get(&key) {
                return v.clone();
            }
        }
        let mut values = Vec::with_capacity(
            ((right.row - left.row + 1) as usize).max((right.column - left.column + 1) as usize),
        );
        for row in left.row..=right.row {
            for column in left.column..=right.column {
                values.push(self.evaluate_cell(CellReferenceIndex {
                    sheet: left.sheet,
                    row,
                    column,
                }));
            }
        }
        let vector = Arc::new(LookupVector {
            values,
            compare: OnceLock::new(),
            equal: OnceLock::new(),
        });
        // A read cut short by an unwind or a restart is not the real thing.
        if self.evaluation.in_pass && !self.evaluation.unwinding && self.evaluation.restart.is_none() {
            self.evaluation.lookup_cache.insert(key, vector.clone());
        }
        vector
    }
}
