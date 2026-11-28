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
    bench_formulas(&mut wb, runs)
}

/// Like `bench_data`, but the orders are generated inside a DuckDB file
/// (`db`), with the same columns and distributions as `generate`, so row
/// counts far beyond what fits on disk as CSV can be measured.
pub fn bench_data_generated(rows: u64, db: &Path, runs: usize) -> Result<()> {
    let mut wb = Workbook::new()?;
    println!("DuckDB {}", wb.data().duckdb_version()?);
    let _ = std::fs::remove_file(db);
    let _ = std::fs::remove_file(db.with_extension("duckdb.wal"));
    wb.persist_data_to(db)?;
    let list = |items: &[&str]| {
        items.iter().map(|s| format!("'{s}'")).collect::<Vec<_>>().join(", ")
    };
    // hash(i, k) gives each column its own pseudo-random stream.
    let sql = format!(
        "SELECT id, customer_id, product_id, order_date, category, region, quantity, unit_price, \
                CASE WHEN hash(id, 9) % 100 = 0 THEN NULL ELSE quantity * unit_price END AS amount, status \
         FROM (SELECT i AS id, \
                      CAST(1 + hash(i, 1) % {cust} AS BIGINT) AS customer_id, \
                      CAST(1 + hash(i, 2) % 5000 AS BIGINT) AS product_id, \
                      DATE '2024-01-01' + CAST(hash(i, 3) % 900 AS INTEGER) AS order_date, \
                      [{cats}][CAST(1 + hash(i, 4) % {ncat} AS BIGINT)] AS category, \
                      [{regs}][CAST(1 + hash(i, 5) % {nreg} AS BIGINT)] AS region, \
                      CAST(1 + hash(i, 6) % 20 AS BIGINT) AS quantity, \
                      CAST(1000 + hash(i, 7) % 199 * 1000 AS BIGINT) AS unit_price, \
                      CASE WHEN hash(i, 8) % 100 < 90 THEN '정상' WHEN hash(i, 8) % 100 < 97 THEN '취소' ELSE '환불' END AS status \
               FROM range(1, {end}) t(i))",
        cust = rows / 10 + 1,
        cats = list(CATEGORIES),
        ncat = CATEGORIES.len(),
        regs = list(REGIONS),
        nreg = REGIONS.len(),
        end = rows + 1,
    );
    let t = Instant::now();
    let info = wb.data().create_table_from_sql("orders", &sql)?;
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    wb.data().checkpoint()?;
    let size = std::fs::metadata(db).map(|m| m.len()).unwrap_or(0);
    println!(
        "생성: {} 행, {:.0} ms, 데이터 파일 {:.1} MB",
        info.rows,
        ms,
        size as f64 / 1e6
    );
    bench_sorted_keyset(&mut wb, runs)?;
    bench_formulas(&mut wb, runs)
}

/// Compares deep sorted OFFSET requests with keyset requests at the same page boundaries.
/// Cursor setup is deliberately outside the timed section; this measures per-page latency,
/// not the cumulative cost of traversing from the first page.
fn bench_sorted_keyset(wb: &mut Workbook, runs: usize) -> Result<()> {
    use cellx_core::data::Sort;
    const PAGE: u64 = 100;
    let sort = Sort { column: "amount".into(), descending: true };
    let total = wb.data_page("orders", 0, 1, Some(&sort), &[])?.total;
    let depths = [0_u64, 10, 100];
    println!("\n정렬 페이지 OFFSET vs Keyset (페이지 크기 {PAGE}, 커서 준비 시간 제외)");
    println!("{:<16} {:>12} {:>12} {:>12} {:>12}", "페이지", "OFFSET ms", "Keyset ms", "OFFSET P95", "Keyset P95");

    for page_index in depths {
        let offset = page_index.saturating_mul(PAGE);
        if offset >= total { continue; }
        let mut cursor = None;
        for _ in 0..page_index {
            let result = wb.data_page_after_sort("orders", cursor.as_ref(), PAGE, &sort, &[])?;
            cursor = result.next_cursor;
            if cursor.is_none() { break; }
        }
        let mut offset_times = Vec::with_capacity(runs.max(1));
        let mut keyset_times = Vec::with_capacity(runs.max(1));
        let mut offset_ids = Vec::new();
        let mut keyset_ids = Vec::new();
        for _ in 0..runs.max(1) {
            let started = Instant::now();
            let offset_page = wb.data_page("orders", offset, PAGE, Some(&sort), &[])?;
            offset_times.push(started.elapsed().as_secs_f64() * 1000.0);
            let started = Instant::now();
            let keyset_page = wb.data_page_after_sort("orders", cursor.as_ref(), PAGE, &sort, &[])?;
            keyset_times.push(started.elapsed().as_secs_f64() * 1000.0);
            offset_ids = offset_page.row_ids;
            keyset_ids = keyset_page.page.row_ids;
        }
        if offset_ids != keyset_ids {
            anyhow::bail!("OFFSET/Keyset 결과 불일치: page={page_index}");
        }
        println!("{:<16} {:>12.2} {:>12.2} {:>12.2} {:>12.2}",
            page_index, median(offset_times.clone()), median(keyset_times.clone()),
            p95(offset_times), p95(keyset_times));
    }
    Ok(())
}

/// What the data-sheet grid asks for while scrolling, sorting and filtering.
fn bench_grid(wb: &mut Workbook, runs: usize) -> Result<()> {
    use cellx_core::data::{ColumnFilter, Sort};
    let total = wb.data_page("orders", 0, 1, None, &[])?.total;
    let f = |c: &str, v: &str| ColumnFilter { column: c.into(), criterion: v.into() };
    let cases: Vec<(&str, u64, Option<Sort>, Vec<ColumnFilter>)> = vec![
        ("첫 화면 (100행)", 0, None, vec![]),
        ("가운데로 스크롤", total / 2, None, vec![]),
        ("금액 내림차순 정렬", 0, Some(Sort { column: "amount".into(), descending: true }), vec![]),
        ("정렬 후 가운데", total / 2, Some(Sort { column: "amount".into(), descending: true }), vec![]),
        ("필터 지역=서울", 0, None, vec![f("region", "서울")]),
        ("필터 2개 + 정렬", 0, Some(Sort { column: "order_date".into(), descending: false }),
            vec![f("region", "서울"), f("category", "*가전")]),
    ];
    println!("\n{:<24} {:>12} {:>12} {:>14}", "격자 작업", "첫 요청 ms", "중앙값 ms", "일치 행");
    for (label, offset, sort, filters) in cases {
        let mut times = Vec::with_capacity(runs);
        let mut matched = 0;
        for _ in 0..runs.max(1) {
            let t = Instant::now();
            let page = wb.data_page("orders", offset, 100, sort.as_ref(), &filters)?;
            times.push(t.elapsed().as_secs_f64() * 1000.0);
            matched = page.total;
        }
        println!("{:<24} {:>12.1} {:>12.1} {:>14}", label, times[0], median(times.clone()), matched);
    }
    Ok(())
}

fn bench_formulas(wb: &mut Workbook, runs: usize) -> Result<()> {
    bench_grid(wb, runs)?;
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
        wb.set_column_width(0, col, col, w)?;
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

/// Pads `s` to `width` terminal columns (Hangul and other wide characters take two).
fn pad(s: &str, width: usize) -> String {
    let w: usize = s.chars().map(|c| if c >= '\u{1100}' { 2 } else { 1 }).sum();
    format!("{s}{}", " ".repeat(width.saturating_sub(w)))
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// Times what a person does with a large ordinary sheet: build, save and open xlsx,
/// scroll, sort, filter, insert rows, format, copy and paste, undo.
pub fn bench_sheet(rows: i32, dir: &Path) -> Result<()> {
    use cellx_core::datatools::{ColumnFilter, SortKey};
    use cellx_core::workbook::CellRange;
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let mut wb = Workbook::new()?;
    let header = ["주문번호", "지역", "카테고리", "주문일", "수량", "단가", "금액", "구분"];
    let mut cells = Vec::with_capacity(rows as usize * 8 + 8);
    for (i, h) in header.iter().enumerate() {
        cells.push((1, i as i32 + 1, h.to_string()));
    }
    for r in 2..=rows + 1 {
        let (y, m, d) = civil(19723 + rng.below(900) as i64);
        cells.push((r, 1, format!("{}", 100000 + r)));
        cells.push((r, 2, REGIONS[rng.below(REGIONS.len() as u64) as usize].to_string()));
        cells.push((r, 3, CATEGORIES[rng.below(CATEGORIES.len() as u64) as usize].to_string()));
        cells.push((r, 4, format!("{y:04}-{m:02}-{d:02}")));
        cells.push((r, 5, format!("{}", 1 + rng.below(20))));
        cells.push((r, 6, format!("{}", 1000 + rng.below(199) * 1000)));
        cells.push((r, 7, format!("=E{r}*F{r}")));
        cells.push((r, 8, format!("=IF(G{r}>=100000,\"대\",\"소\")")));
    }
    let last = rows + 1;
    let all = CellRange { r0: 1, c0: 1, r1: last, c1: 8 };
    println!("시트: {rows}행 × 8열 (값 {}개, 수식 {}개)\n", rows * 6, rows * 2);
    let report = |label: &str, v: f64| println!("{} {v:>9.0} ms", pad(label, 40));

    let t = Instant::now();
    wb.set_inputs(0, &cells)?;
    report("입력 + 전체 계산", ms(t));
    drop(cells);

    let path = dir.join(format!("bench-{rows}.xlsx"));
    let t = Instant::now();
    wb.save_xlsx(&path)?;
    let save = ms(t);
    let size = std::fs::metadata(&path)?.len();
    report(&format!("xlsx 저장 ({:.1} MB)", size as f64 / 1e6), save);

    let t = Instant::now();
    let mut wb = Workbook::open_xlsx(&path)?;
    report("xlsx 열기", ms(t));
    let t = Instant::now();
    wb.recalc()?;
    report("열고 나서 전체 재계산", ms(t));

    let mut times = Vec::new();
    for _ in 0..30 {
        let r0 = 1 + rng.below(last.max(2) as u64 - 1) as i32;
        let t = Instant::now();
        std::hint::black_box(wb.viewport(0, r0, 1, r0 + 50, 20)?);
        times.push(ms(t));
    }
    println!("{} {:>9.2} ms (P95 {:.2})", pad("화면 한 장(50행×20열) 그리기", 40), median(times.clone()), p95(times));

    let t = Instant::now();
    wb.set_input(0, 2, 5, "7")?;
    report("첫 수정 (의존성 인덱스 생성 포함)", ms(t));
    let mut edits = Vec::new();
    for i in 0..20 {
        let r = 3 + (rng.below(rows as u64) as i32);
        let t = Instant::now();
        wb.set_input(0, r, 5, &format!("{}", 1 + i % 9))?;
        edits.push(ms(t));
    }
    println!("{} {:>9.2} ms (계산한 수식 {}개)", pad("셀 하나 수정 + 재계산 (중앙값)", 40), median(edits), wb.last_recalc.evaluated.map_or("전체".into(), |n| n.to_string()));
    let t = Instant::now();
    wb.set_input(0, 2, 6, "=SUM(G3:G1000)")?;
    report("수식 하나 입력", ms(t));

    let t = Instant::now();
    wb.sort_range(0, all, &[SortKey { col: 2, descending: false }, SortKey { col: 6, descending: true }], true)?;
    report("정렬 (지역 오름차순, 단가 내림차순)", ms(t));
    let t = Instant::now();
    wb.undo()?;
    report("정렬 실행 취소", ms(t));

    let t = Instant::now();
    wb.toggle_auto_filter(0, all)?;
    wb.set_filter_column(0, ColumnFilter { col: 2, values: Some(vec!["서울".into()]), conditions: vec![], and: true })?;
    report("자동 필터 (지역 = 서울)", ms(t));
    wb.toggle_auto_filter(0, all)?;

    let t = Instant::now();
    wb.insert_rows(0, 2, 1)?;
    report("맨 위에 행 삽입 (아래 수식 전부 이동)", ms(t));
    let t = Instant::now();
    wb.undo()?;
    report("행 삽입 실행 취소", ms(t));
    let t = Instant::now();
    wb.delete_rows(0, 2, 1)?;
    report("맨 위 행 삭제", ms(t));
    wb.undo()?;

    let t = Instant::now();
    wb.insert_columns(0, 1, 1)?;
    report("맨 앞에 열 삽입", ms(t));
    wb.undo()?;
    let t = Instant::now();
    wb.delete_columns(0, 1, 1)?;
    report("맨 앞 열 삭제", ms(t));
    wb.undo()?;

    let t = Instant::now();
    wb.set_style(0, CellRange { r0: 2, c0: 7, r1: last, c1: 7 }, "num_fmt", "#,##0")?;
    report("열 전체 서식 (#,##0)", ms(t));

    let n = rows.min(10_000);
    let t = Instant::now();
    let text = wb.copy(0, CellRange { r0: 2, c0: 1, r1: n + 1, c1: 8 }, false)?;
    wb.paste(0, 2, 10, &text)?;
    report(&format!("{n}행 × 8열 복사 + 붙여넣기"), ms(t));

    let t = Instant::now();
    std::hint::black_box(wb.find(0, "환불없음", (1, 1), false)?);
    report("찾기 (없는 값, 시트 전체)", ms(t));
    if std::env::var_os("CELLX_KEEP_BENCH_FILE").is_none() {
        std::fs::remove_file(&path).ok();
    }
    Ok(())
}
