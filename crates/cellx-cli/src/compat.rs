//! `cellx compat`: compares the values Excel saved in xlsx files with what the
//! CellX calculation engine computes, cell by cell.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use cellx_core::value::{from_ironcalc, Scalar};
use cellx_core::workbook::load_xlsx;
use serde::Serialize;

#[derive(Default, Serialize, Clone)]
pub struct FunctionStat {
    pub cells: u64,
    pub mismatches: u64,
}

#[derive(Serialize)]
pub struct FileResult {
    pub file: String,
    pub formula_cells: u64,
    pub mismatches: u64,
    pub load_ms: f64,
    pub eval_ms: f64,
    pub error: Option<String>,
    pub examples: Vec<String>,
}

#[derive(Serialize)]
pub struct Report {
    pub files: Vec<FileResult>,
    pub functions: BTreeMap<String, FunctionStat>,
    pub formula_cells: u64,
    pub mismatches: u64,
    pub match_rate: f64,
}

pub fn collect_files(inputs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in inputs {
        if p.is_dir() {
            walk(p, &mut out);
        } else {
            out.push(p.clone());
        }
    }
    out.sort();
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("xlsx"))
            && !p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("~$"))
        {
            out.push(p);
        }
    }
}

/// Function names used in a formula, e.g. `=SUM(A1:A3)+_xlfn.XLOOKUP(...)` → SUM, XLOOKUP.
pub fn functions_in(formula: &str) -> Vec<String> {
    let mut names = Vec::new();
    let chars: Vec<char> = formula.chars().collect();
    let mut in_string = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            in_string = !in_string;
            i += 1;
            continue;
        }
        if in_string || !(c.is_ascii_alphabetic() || c == '_') {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '.') {
            i += 1;
        }
        if i < chars.len() && chars[i] == '(' {
            let name: String = chars[start..i].iter().collect::<String>().to_ascii_uppercase();
            let name = name
                .trim_start_matches("_XLFN.")
                .trim_start_matches("_XLWS.")
                .to_string();
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

fn close(a: f64, b: f64) -> bool {
    let d = (a - b).abs();
    d <= 1e-9 || d <= 1e-9 * a.abs().max(b.abs())
}

fn same(excel: &Scalar, ours: &Scalar) -> bool {
    match (excel, ours) {
        (Scalar::Number(a), Scalar::Number(b)) => close(*a, *b),
        // An empty cached value and an empty string are the same thing on screen.
        (Scalar::Empty, Scalar::Text(t)) | (Scalar::Text(t), Scalar::Empty) => t.is_empty(),
        (a, b) => a == b,
    }
}

fn check_file(path: &Path, functions: &mut BTreeMap<String, FunctionStat>) -> FileResult {
    let file = path.display().to_string();
    let t = Instant::now();
    let cached = match load_xlsx(path) {
        Ok(m) => m,
        Err(e) => {
            return FileResult {
                file,
                formula_cells: 0,
                mismatches: 0,
                load_ms: 0.0,
                eval_ms: 0.0,
                error: Some(e.to_string()),
                examples: vec![],
            }
        }
    };
    let load_ms = t.elapsed().as_secs_f64() * 1000.0;
    let mut evaluated = match load_xlsx(path) {
        Ok(m) => m,
        Err(e) => {
            return FileResult {
                file,
                formula_cells: 0,
                mismatches: 0,
                load_ms,
                eval_ms: 0.0,
                error: Some(e.to_string()),
                examples: vec![],
            }
        }
    };
    let t = Instant::now();
    evaluated.evaluate();
    let eval_ms = t.elapsed().as_secs_f64() * 1000.0;

    let sheet_names = cached.workbook.get_worksheet_names();
    let mut formula_cells = 0;
    let mut mismatches = 0;
    let mut examples = Vec::new();
    for cell in cached.get_all_cells() {
        let Ok(Some(formula)) = cached.get_cell_formula(cell.index, cell.row, cell.column) else {
            continue;
        };
        formula_cells += 1;
        let excel = cached
            .get_cell_value_by_index(cell.index, cell.row, cell.column)
            .map(from_ironcalc)
            .unwrap_or(Scalar::Empty);
        let ours = evaluated
            .get_cell_value_by_index(cell.index, cell.row, cell.column)
            .map(from_ironcalc)
            .unwrap_or(Scalar::Empty);
        let ok = same(&excel, &ours);
        for f in functions_in(&formula) {
            let s = functions.entry(f).or_default();
            s.cells += 1;
            if !ok {
                s.mismatches += 1;
            }
        }
        if !ok {
            mismatches += 1;
            if examples.len() < 5 {
                let col = ironcalc_base::expressions::utils::number_to_column(cell.column).unwrap_or_default();
                examples.push(format!(
                    "{}!{}{}  {}  Excel={}  CellX={}",
                    sheet_names.get(cell.index as usize).cloned().unwrap_or_default(),
                    col,
                    cell.row,
                    formula,
                    excel.display(),
                    ours.display()
                ));
            }
        }
    }
    FileResult { file, formula_cells, mismatches, load_ms, eval_ms, error: None, examples }
}

#[derive(Serialize, serde::Deserialize)]
pub struct OneFile {
    pub result: FileResultData,
    pub functions: BTreeMap<String, (u64, u64)>,
}

/// Serializable mirror of [`FileResult`] for the child-process protocol.
#[derive(Serialize, serde::Deserialize)]
pub struct FileResultData {
    pub file: String,
    pub formula_cells: u64,
    pub mismatches: u64,
    pub load_ms: f64,
    pub eval_ms: f64,
    pub error: Option<String>,
    pub examples: Vec<String>,
}

/// Checks one file and prints the result as JSON (runs in a child process).
pub fn run_one(path: &Path) {
    let mut local = BTreeMap::new();
    let r = check_file(path, &mut local);
    let out = OneFile {
        result: FileResultData {
            file: r.file,
            formula_cells: r.formula_cells,
            mismatches: r.mismatches,
            load_ms: r.load_ms,
            eval_ms: r.eval_ms,
            error: r.error,
            examples: r.examples,
        },
        functions: local.into_iter().map(|(k, v)| (k, (v.cells, v.mismatches))).collect(),
    };
    println!("{}", serde_json::to_string(&out).unwrap_or_default());
}

fn failed(path: &Path, why: String) -> FileResult {
    FileResult {
        file: path.display().to_string(),
        formula_cells: 0,
        mismatches: 0,
        load_ms: 0.0,
        eval_ms: 0.0,
        error: Some(why),
        examples: vec![],
    }
}

/// Runs every file in its own process: IronCalc can abort on some inputs (for
/// example an allocation failure), which would otherwise end the whole run.
pub fn run(inputs: &[PathBuf], timeout: Duration) -> Report {
    let files = collect_files(inputs);
    let exe = std::env::current_exe().expect("current exe");
    let mut functions: BTreeMap<String, FunctionStat> = BTreeMap::new();
    let mut results = Vec::new();
    let total = files.len();
    for (i, path) in files.into_iter().enumerate() {
        eprint!("\r[{}/{}] {}", i + 1, total, path.file_name().and_then(|n| n.to_str()).unwrap_or(""));
        let child = Command::new(&exe)
            .arg("compat-one")
            .arg(&path)
            .env("RUST_BACKTRACE", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                results.push(failed(&path, format!("검사 프로세스를 시작하지 못했습니다: {e}")));
                continue;
            }
        };
        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) if start.elapsed() > timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(_) => break None,
            }
        };
        let mut stdout = String::new();
        if let Some(mut out) = child.stdout.take() {
            let _ = out.read_to_string(&mut stdout);
        }
        let parsed = serde_json::from_str::<OneFile>(stdout.trim());
        match (status, parsed) {
            (Some(_), Ok(one)) => {
                for (k, (cells, mism)) in one.functions {
                    let s = functions.entry(k).or_default();
                    s.cells += cells;
                    s.mismatches += mism;
                }
                let r = one.result;
                results.push(FileResult {
                    file: r.file,
                    formula_cells: r.formula_cells,
                    mismatches: r.mismatches,
                    load_ms: r.load_ms,
                    eval_ms: r.eval_ms,
                    error: r.error,
                    examples: r.examples,
                });
            }
            (None, _) => results.push(failed(&path, format!("{}초 안에 끝나지 않았습니다", timeout.as_secs()))),
            (Some(s), Err(_)) => results.push(failed(&path, format!("계산 엔진이 이 파일에서 중단되었습니다 ({s})"))),
        }
    }
    eprintln!();
    let formula_cells: u64 = results.iter().map(|r| r.formula_cells).sum();
    let mismatches: u64 = results.iter().map(|r| r.mismatches).sum();
    let match_rate = if formula_cells == 0 {
        0.0
    } else {
        1.0 - mismatches as f64 / formula_cells as f64
    };
    Report { files: results, functions, formula_cells, mismatches, match_rate }
}

pub fn print(report: &Report, verbose: bool) {
    let failed: Vec<&FileResult> = report.files.iter().filter(|f| f.error.is_some()).collect();
    println!("파일 {}개 (열기 실패 {}개)", report.files.len(), failed.len());
    println!(
        "수식 셀 {}개, 불일치 {}개, 일치율 {:.2}%",
        report.formula_cells,
        report.mismatches,
        report.match_rate * 100.0
    );
    let mut worst: Vec<(&String, &FunctionStat)> =
        report.functions.iter().filter(|(_, s)| s.mismatches > 0).collect();
    worst.sort_by(|a, b| b.1.mismatches.cmp(&a.1.mismatches));
    if !worst.is_empty() {
        println!("\n불일치가 많은 함수:");
        println!("  {:<20} {:>8} {:>8}", "함수", "셀", "불일치");
        for (name, s) in worst.iter().take(20) {
            println!("  {:<20} {:>8} {:>8}", name, s.cells, s.mismatches);
        }
    }
    if !failed.is_empty() {
        println!("\n열지 못한 파일:");
        for f in failed.iter().take(if verbose { usize::MAX } else { 10 }) {
            println!("  {}: {}", f.file, f.error.as_deref().unwrap_or(""));
        }
    }
    let mut slow: Vec<&FileResult> = report.files.iter().filter(|f| f.error.is_none()).collect();
    slow.sort_by(|a, b| (b.load_ms + b.eval_ms).total_cmp(&(a.load_ms + a.eval_ms)));
    println!("\n오래 걸린 파일 (열기 / 전체 재계산):");
    for f in slow.iter().take(5) {
        println!("  {:>9.1} ms / {:>9.1} ms  {}", f.load_ms, f.eval_ms, f.file);
    }
    if verbose {
        println!("\n불일치 예시:");
        for f in report.files.iter().filter(|f| !f.examples.is_empty()) {
            println!("  {}", f.file);
            for e in &f.examples {
                println!("    {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_function_names() {
        assert_eq!(
            functions_in("=SUM(A1:A3)+_xlfn.XLOOKUP(B1,C:C,D:D)&\"MAX(x)\""),
            vec!["SUM", "XLOOKUP"]
        );
    }
}
