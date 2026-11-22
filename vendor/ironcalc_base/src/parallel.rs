//! CellX: full evaluation on several threads.
//!
//! The formulas are split into groups that do not read each other: the
//! connected parts of the dependency graph (`dependencies.rs`). Each thread
//! evaluates some of the groups on its own copy of the model, and the values
//! come back into this one. A group with a formula whose inputs are not known
//! in advance (INDIRECT, OFFSET, a defined name, a volatile function) runs
//! afterwards on this model, when every other value is in place, as does
//! everything on a sheet with dynamic arrays (those are left to `evaluate`).
//! The result is that of `evaluate()`: each formula is evaluated once, from
//! values that are final.

use std::collections::{HashMap, HashSet};

use crate::dependencies::DepIndex;
use crate::evaluation::{CellKey, KeyBuild};
use crate::expressions::types::CellReferenceIndex;
use crate::model::Model;
use crate::types::Cell;

/// Below this many formulas a copy of the model costs more than it saves.
const MIN_FORMULAS: usize = 20_000;

/// Splitting the work costs about as much as evaluating light formulas
/// (building the groups, copying the model): only a workbook whose last full
/// evaluation took longer than this is worth it.
pub(crate) const MIN_SERIAL_MS: f64 = 250.0;

struct UnionFind {
    parent: Vec<usize>,
    size: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        UnionFind { parent: (0..n).collect(), size: vec![1; n] }
    }
    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }
    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a == b {
            return;
        }
        let (big, small) = if self.size[a] >= self.size[b] { (a, b) } else { (b, a) };
        self.parent[small] = big;
        self.size[big] += self.size[small];
    }
}

impl Model<'_> {
    /// CellX: use up to `threads` threads for full evaluations (1: never).
    pub fn set_evaluation_threads(&mut self, threads: usize) {
        self.evaluation.threads = threads.max(1);
    }

    /// CellX: share out a full evaluation only when the last one took at least
    /// this long on one thread (250 ms unless set).
    pub fn set_parallel_threshold_ms(&mut self, ms: f64) {
        self.evaluation.parallel_min_ms = ms.max(0.0);
    }

    /// CellX: how many threads the last full evaluation used.
    pub fn last_evaluation_threads(&self) -> usize {
        self.evaluation.last_threads
    }

    /// The area of a dynamic anchor (width, height).
    fn array_area(&self, (sheet, row, column): CellKey) -> Option<(i32, i32)> {
        match self.fetch_cell(CellReferenceIndex { sheet, row, column }).as_deref() {
            Some(Cell::ArrayFormula { r, .. }) => Some(*r),
            _ => None,
        }
    }

    /// A copy that can evaluate: the workbook and the parsed formulas, with
    /// fresh evaluation state.
    fn evaluation_copy(&self) -> Self {
        Model {
            workbook: self.workbook.clone(),
            parsed_formulas: self.parsed_formulas.clone(),
            parsed_defined_names: self.parsed_defined_names.clone(),
            shared_strings: HashMap::new(),
            parser: self.parser.clone(),
            locale: self.locale,
            language: self.language,
            tz: self.tz.clone(),
            variable_stack: HashMap::new(),
            last_variable_id: 0,
            lambdas: HashMap::new(),
            last_lambda_id: 0,
            evaluation: crate::evaluation::Evaluation { max_depth: self.evaluation.max_depth, ..Default::default() },
            cf_cache: HashMap::new(),
            links: HashMap::new(),
            shared_formula_lookup: Vec::new(),
        }
    }

    /// The groups of formulas that can run apart, and the formulas that run
    /// after them (unknown inputs, dynamic formulas, and what reads them).
    fn split(deps: &DepIndex, anchors: &[CellKey]) -> (Vec<Vec<CellKey>>, Vec<CellKey>) {
        let keys: Vec<CellKey> = deps.formula_keys();
        let index: HashMap<CellKey, usize, KeyBuild> = keys.iter().enumerate().map(|(i, k)| (*k, i)).collect();
        let mut uf = UnionFind::new(keys.len());
        for (i, k) in keys.iter().enumerate() {
            deps.dependents(*k, |d| {
                if let Some(j) = index.get(&d) {
                    uf.union(i, *j);
                }
            });
        }
        let mut late_root: HashSet<usize> = HashSet::new();
        for k in deps.volatile.iter().chain(anchors.iter()) {
            if let Some(i) = index.get(k) {
                late_root.insert(uf.find(*i));
            }
        }
        let mut groups: HashMap<usize, Vec<CellKey>> = HashMap::new();
        let mut late: Vec<CellKey> = Vec::new();
        for (i, k) in keys.iter().enumerate() {
            let r = uf.find(i);
            if late_root.contains(&r) {
                late.push(*k);
            } else {
                groups.entry(r).or_default().push(*k);
            }
        }
        late.sort_unstable();
        (groups.into_values().collect(), late)
    }

    /// Evaluates the workbook on several threads when that pays; returns
    /// false when it did nothing and `evaluate` has to do it all.
    pub(crate) fn evaluate_in_parallel(&mut self) -> bool {
        let threads = self.evaluation.threads;
        if threads < 2 || self.evaluation.last_full_ms < self.evaluation.parallel_min_ms {
            return false;
        }
        self.sync_anchor_order();
        // Dynamic formulas (IronCalc keeps INDIRECT, LET and the like as
        // one-cell dynamic arrays) run with the late ones; one that spills
        // already leaves it all to `evaluate`.
        let anchors: Vec<CellKey> = self.evaluation.anchor_order.iter().map(|a| (a.sheet, a.row, a.column)).collect();
        let spilled = |m: &Self| anchors.iter().any(|k| m.array_area(*k) != Some((1, 1)));
        if spilled(self) {
            return false;
        }
        let mut deps = match self.evaluation.deps.take() {
            Some(d) if d.still_valid(self) => d,
            _ => DepIndex::build(self),
        };
        if deps.formula_count() < MIN_FORMULAS || !deps.arrays.is_empty() {
            self.evaluation.deps = Some(deps);
            return false;
        }
        // The split is kept with the index until a formula changes.
        let (groups, late) = match &deps.groups {
            Some(g) => g.clone(),
            None => {
                let g = Self::split(&deps, &anchors);
                deps.groups = Some(g.clone());
                g
            }
        };
        self.evaluation.deps = Some(deps);
        let parallel: usize = groups.iter().map(Vec::len).sum();
        let biggest = groups.iter().map(Vec::len).max().unwrap_or(0);
        // One group holding most formulas cannot be shared out.
        if parallel < MIN_FORMULAS / 2 || biggest * 10 > parallel * 7 {
            return false;
        }
        // Buckets of about the same size, biggest groups first.
        let mut groups = groups;
        groups.sort_unstable_by_key(|g| std::cmp::Reverse(g.len()));
        let mut buckets: Vec<Vec<CellKey>> = vec![Vec::new(); threads];
        for g in groups {
            let b = (0..threads).min_by_key(|b| buckets[*b].len()).unwrap_or(0);
            buckets[b].extend(g);
        }
        for b in &mut buckets {
            b.sort_unstable();
        }
        let start = std::time::Instant::now();
        let mine = buckets.remove(0);
        let copies: Vec<Model<'_>> = buckets.iter().map(|_| self.evaluation_copy()).collect();
        let results: Vec<Option<Vec<(CellKey, Cell)>>> = std::thread::scope(|scope| {
            let handles: Vec<_> = copies
                .into_iter()
                .zip(buckets)
                .map(|(mut copy, bucket)| {
                    std::thread::Builder::new()
                        .stack_size(64 << 20)
                        .spawn_scoped(scope, move || {
                            let only: HashSet<CellKey, KeyBuild> = bucket.iter().copied().collect();
                            if copy.run_partial_pass(bucket.clone(), only).is_some() {
                                return None;
                            }
                            let mut out = Vec::with_capacity(bucket.len());
                            for (sheet, row, column) in bucket {
                                if let Some(c) = copy.fetch_cell(CellReferenceIndex { sheet, row, column }) {
                                    out.push(((sheet, row, column), c.into_owned()));
                                }
                            }
                            Some(out)
                        })
                        .ok()
                })
                .collect();
            let only: HashSet<CellKey, KeyBuild> = mine.iter().copied().collect();
            let own = self.run_partial_pass(mine, only);
            let mut all: Vec<Option<Vec<(CellKey, Cell)>>> = handles.into_iter().map(|h| h.and_then(|h| h.join().ok()).flatten()).collect();
            if own.is_some() {
                all.push(None);
            }
            all
        });
        if results.iter().any(Option::is_none) {
            // Something needed a restart (not expected without dynamic arrays):
            // the plain evaluation starts over and gets it right.
            return false;
        }
        for (key, cell) in results.into_iter().flatten().flatten() {
            if let Ok(ws) = self.workbook.worksheet_mut(key.0) {
                let _ = ws.update_cell(key.1, key.2, cell);
            }
        }
        // The rest, with every other value in place.
        if !late.is_empty() {
            let only: HashSet<CellKey, KeyBuild> = late.iter().copied().collect();
            if self.run_partial_pass(late, only).is_some() || spilled(self) {
                // A formula now spills over cells others may have read.
                return false;
            }
        }
        self.evaluate_conditional_formatting();
        self.evaluation.last_partial = None;
        self.evaluation.last_threads = threads;
        self.evaluation.last_full_ms = start.elapsed().as_secs_f64() * 1000.0 * threads as f64;
        true
    }
}
