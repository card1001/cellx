//! Synthetic data and performance measurements.

use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use cellx_core::bridge;
use cellx_core::Workbook;

/// Small deterministic PRNG so every run produces the same file.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const REGIONS: &[&str] = &[
    "서울", "부산", "대구", "인천", "광주", "대전", "울산", "세종", "경기", "강원", "충북", "충남",
    "전북", "전남", "경북", "경남", "제주",
];
const CATEGORIES: &[&str] = &[
    "생활가전", "주방가전", "디지털", "패션", "뷰티", "식품", "도서", "스포츠", "완구", "가구",
    "반려동물", "자동차용품",
];

fn civil(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Writes an orders CSV with mixed types, duplicates, NULLs, low- and high-cardinality columns.
pub fn generate(rows: u64, out: &Path) -> Result<()> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut w = BufWriter::with_capacity(1 << 20, std::fs::File::create(out)?);
    writeln!(w, "id,customer_id,product_id,order_date,category,region,quantity,unit_price,amount,status")?;
    // 2024-01-01 is day 19723 since 1970-01-01.
    for id in 1..=rows {
        let (y, m, d) = civil(19723 + rng.below(900) as i64);
        let qty = 1 + rng.below(20);
        let price = 1000 + rng.below(199) * 1000;
        let status = match rng.below(100) {
            0..=89 => "정상",
            90..=96 => "취소",
            _ => "환불",
        };
        let amount = if rng.below(100) == 0 { String::new() } else { (qty * price).to_string() };
        writeln!(
            w,
            "{id},{},{},{y:04}-{m:02}-{d:02},{},{},{qty},{price},{amount},{status}",
            1 + rng.below(rows / 10 + 1),
            1 + rng.below(5000),
            CATEGORIES[rng.below(CATEGORIES.len() as u64) as usize],
            REGIONS[rng.below(REGIONS.len() as u64) as usize],
        )?;
    }
    w.flush()?;
    Ok(())
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    if n == 0 {
        0.0
    } else if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

fn p95(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    if v.is_empty() {
        return 0.0;
    }
    let idx = ((v.len() as f64) * 0.95).ceil() as usize;
    v[idx.saturating_sub(1).min(v.len() - 1)]
}

pub const DATA_FORMULAS: &[(&str, &str)] = &[
    ("SUM 전체", "=SUM(orders[amount])"),
    ("SUMIFS 조건 1개", "=SUMIFS(orders[amount], orders[region], \"서울\")"),
    (
        "SUMIFS 조건 2개(날짜)",
        "=SUMIFS(orders[amount], orders[region], \"서울\", orders[order_date], \">=2025-01-01\")",
    ),
    ("COUNTIFS 와일드카드", "=COUNTIFS(orders[status], \"취소\", orders[category], \"*가전\")"),
    ("AVERAGEIFS", "=AVERAGEIFS(orders[amount], orders[quantity], \">=10\")"),
    ("XLOOKUP", "=XLOOKUP(500000, orders[id], orders[amount], \"없음\")"),
    (
        "DQUERY 그룹 집계",
        "=DQUERY(\"SELECT region, SUM(amount) AS total FROM orders GROUP BY 1 ORDER BY 2 DESC\")",
    ),
];

/// Imports a CSV as the `orders` data sheet and times bridge formulas on it.
pub fn bench_data(csv: &Path, runs: usize) -> Result<()> {
    let mut wb = Workbook::new()?;
    println!("DuckDB {}", wb.data().duckdb_version()?);
    let size = std::fs::metadata(csv)?.len();
    let t = Instant::now();
    let info = wb.import_data(csv, "orders")?;
    let import_ms = t.elapsed().as_secs_f64() * 1000.0;
    println!(
        "가져오기: {} 행, {:.1} MB, {:.0} ms ({:.0} 만 행/초)",
        info.rows,
        size as f64 / 1e6,
        import_ms,
        info.rows as f64 / (import_ms / 1000.0) / 1e4
    );
    println!("\n{:<24} {:>12} {:>12} {:>12} {:>14}  결과", "수식", "첫 계산 ms", "중앙값 ms", "P95 ms", "캐시 적중 ms");
    for (i, (label, formula)) in DATA_FORMULAS.iter().enumerate() {
        let row = i as i32 + 1;
        let stats = wb.set_input(0, row, 1, formula)?;
        let first = stats.bridge_ms + stats.calc_ms;

        // Engine time without the cache, measured on the compiled SQL directly.
        let calls = bridge::find_calls(formula, &|n| n.eq_ignore_ascii_case("orders"));
        let call = calls.into_iter().next().expect("bridge call")?;
        struct NoCells;
        impl bridge::CellResolver for NoCells {
            fn cell_value(&self, _: &bridge::CellRef) -> cellx_core::Scalar {
                cellx_core::Scalar::Empty
            }
        }
        let compiled = bridge::compile(&call, wb.data(), &NoCells)?;
        let mut times = Vec::with_capacity(runs);
        for _ in 0..runs {
            let t = Instant::now();
            let v = bridge::execute(&compiled, wb.data())?;
            std::hint::black_box(&v);
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }

        // Same formula in another cell: served from the bridge cache.
        let cached = wb.set_input(0, row, 3, formula)?;
        let shown = wb.detail(0, row, 1)?.text;
        let shown = if shown.chars().count() > 28 {
            format!("{}…", shown.chars().take(28).collect::<String>())
        } else {
            shown
        };
        println!(
            "{:<24} {:>12.1} {:>12.1} {:>12.1} {:>14.2}  {}",
            label,
            first,
            median(times.clone()),
            p95(times),
            cached.bridge_ms + cached.calc_ms,
            shown
        );
    }
    Ok(())
}

/// Times the calculation engine on a generated sheet of ordinary formulas.
pub fn bench_calc(rows: i32) -> Result<()> {
    let mut wb = Workbook::new()?;
    let mut cells = Vec::with_capacity(rows as usize * 5);
    for r in 1..=rows {
        cells.push((r, 1, format!("{}", (r * 7919) % 1000)));
        cells.push((r, 2, format!("=A{r}*2+1")));
        cells.push((r, 3, format!("=IF(B{r}>1000,B{r},0)")));
        cells.push((r, 4, format!("=ROUND(C{r}/3,2)")));
        cells.push((r, 5, format!("=TEXT(D{r},\"0.00\")&\"원\"")));
    }
    let t = Instant::now();
    let stats = wb.set_inputs(0, &cells)?;
    let total = t.elapsed().as_secs_f64() * 1000.0;
    println!(
        "수식 {}개 입력 + 전체 계산: {:.0} ms (계산 {:.0} ms)",
        rows * 4,
        total,
        stats.calc_ms
    );
    let mut edits = Vec::new();
    for i in 0..5 {
        let s = wb.set_input(0, 1 + i * 10, 1, &format!("{}", 500 + i))?;
        edits.push(s.calc_ms);
    }
    println!("셀 하나 수정 후 재계산: 중앙값 {:.1} ms", median(edits));

    // Lookups: the pattern that makes large Excel workbooks slow.
    let lookups = (rows / 10).clamp(100, 5000);
    let mut cells = Vec::new();
    for r in 1..=lookups {
        cells.push((r, 7, format!("=VLOOKUP({},A$1:B${rows},2,FALSE)", (r * 31) % 1000)));
    }
    let s = wb.set_inputs(0, &cells)?;
    println!("VLOOKUP {lookups}개 (범위 {rows}행) 추가 후 전체 계산: {:.0} ms", s.calc_ms);
    Ok(())
}

/// Builds a sample project: an `orders` data sheet and a summary sheet of bridge formulas.
pub fn make_demo(csv: &Path, out: &Path) -> Result<()> {
    let mut wb = Workbook::new()?;
    let info = wb.import_data(csv, "orders")?;
    wb.rename_sheet(0, "요약")?;
    let mut cells: Vec<(i32, i32, String)> = vec![
        (1, 1, "지역별 매출 요약 — 데이터 시트 orders".into()),
        (2, 1, format!("원본 {}행 · 수식은 orders[열]을 참조하고 DuckDB에서 계산됩니다", info.rows)),
        (4, 1, "지역".into()),
        (4, 2, "매출 합계".into()),
        (4, 3, "주문 수".into()),
        (4, 4, "취소율(%)".into()),
        (4, 5, "기준일 이후 매출".into()),
        (4, 7, "기준일".into()),
        (4, 8, "2025-01-01".into()),
    ];
    for (i, region) in REGIONS.iter().enumerate() {
        let r = 5 + i as i32;
        cells.push((r, 1, region.to_string()));
        cells.push((r, 2, format!("=SUMIFS(orders[amount], orders[region], A{r})")));
        cells.push((r, 3, format!("=COUNTIFS(orders[region], A{r})")));
        cells.push((
            r,
            4,
            format!("=ROUND(COUNTIFS(orders[region], A{r}, orders[status], \"취소\") / C{r} * 100, 2)"),
        ));
        cells.push((r, 5, format!("=SUMIFS(orders[amount], orders[region], A{r}, orders[order_date], \">=\"&$H$4)")));
    }
    let last = 4 + REGIONS.len() as i32;
    cells.push((last + 1, 1, "합계".into()));
    cells.push((last + 1, 2, format!("=SUM(B5:B{last})")));
    cells.push((last + 1, 3, format!("=SUM(C5:C{last})")));
    cells.push((last + 1, 5, format!("=SUM(E5:E{last})")));
    cells.push((last + 3, 1, "카테고리별 주문 수 (DQUERY)".into()));
    cells.push((
        last + 4,
        1,
        "=DQUERY(\"SELECT category, COUNT(*) AS orders, SUM(amount) AS total FROM orders GROUP BY 1 ORDER BY 3 DESC\")".into(),
    ));
    let t = Instant::now();
    let stats = wb.set_inputs(0, &cells)?;
    for (col, w) in [(1, 150.0), (2, 150.0), (3, 110.0), (4, 100.0), (5, 160.0), (7, 70.0), (8, 100.0)] {
        wb.set_column_width(0, col, w)?;
    }
    println!(
        "수식 {}개 계산: {:.0} ms (데이터 엔진 {:.0} ms, 호출 {}회)",
        wb.bridge_formula_count(),
        t.elapsed().as_secs_f64() * 1000.0,
        stats.bridge_ms,
        stats.bridge_calls
    );
    cellx_core::project::save_project(&mut wb, out)?;
    println!("저장: {}", out.display());
    Ok(())
}
