//! A full recalculation on several threads gives exactly what one thread gives.

use cellx_core::Workbook;

fn values(wb: &Workbook, rows: i32, cols: i32) -> Vec<String> {
    let mut out = Vec::new();
    for s in 0..2 {
        for r in 1..=rows {
            for c in 1..=cols {
                out.push(wb.model().get_formatted_cell_value(s, r, c).unwrap_or_default());
            }
        }
    }
    out
}

fn book(threads: usize) -> Workbook {
    book_with(threads, true)
}

fn book_with(threads: usize, linked: bool) -> Workbook {
    let mut wb = Workbook::new().unwrap();
    wb.set_calc_threads(threads);
    wb.set_parallel_threshold_ms(0.0);
    wb.add_sheet().unwrap();
    let mut cells = Vec::new();
    for r in 1..=8000 {
        cells.push((r, 1, format!("{}", (r * 37) % 101)));
        cells.push((r, 2, format!("{}", (r * 11) % 7)));
        // Independent rows: many small groups.
        cells.push((r, 3, format!("=A{r}*B{r}+1")));
        cells.push((r, 4, format!("=IF(C{r}>50,ROUND(SQRT(C{r}),2),C{r}^2)")));
        cells.push((r, 5, format!("=TEXT(D{r},\"0.00\")&\"-\"&B{r}")));
    }
    // A chain, a lookup over a whole column, a volatile formula and a defined-name-free INDIRECT.
    for r in 1..=200 {
        cells.push((r, 7, if r == 1 { "=C1".into() } else { format!("=G{}+C{r}", r - 1) }));
    }
    if linked {
        cells.push((1, 8, "=SUM(D:D)".into()));
        cells.push((2, 8, "=VLOOKUP(17,A1:C8000,3,FALSE)".into()));
        cells.push((3, 8, "=INDIRECT(\"C\"&B5)+H1".into()));
        cells.push((4, 8, "=COUNTIF(E1:E8000,\"*-3\")".into()));
    } else {
        cells.push((3, 8, "=INDIRECT(\"A\"&B5)*2".into()));
        cells.push((4, 8, "=NOW()>0".into()));
    }
    cells.push((5, 8, "=H3+G200".into()));
    wb.set_inputs(0, &cells).unwrap();
    wb.set_inputs(1, &[(1, 1, "=Sheet1!H5*2".into()), (2, 1, "=SUM(Sheet1!C1:C100)".into())]).unwrap();
    wb
}

#[test]
fn threads_change_nothing() {
    let mut one = book(1);
    let mut four = book(4);
    one.recalc().unwrap();
    four.recalc().unwrap();
    assert_eq!(values(&one, 300, 8), values(&four, 300, 8));
    let a = values(&one, 8000, 5);
    let b = values(&four, 8000, 5);
    assert_eq!(a, b);
    // And after edits (incremental), then a full recalculation again.
    for wb in [&mut one, &mut four] {
        wb.set_input(0, 5, 2, "3").unwrap();
        wb.set_input(0, 17, 1, "99").unwrap();
        wb.recalc().unwrap();
    }
    assert_eq!(values(&one, 300, 8), values(&four, 300, 8));
    assert_eq!(one.model().get_formatted_cell_value(1, 1, 1).unwrap(), four.model().get_formatted_cell_value(1, 1, 1).unwrap());
}

#[test]
fn independent_rows_use_the_threads() {
    let mut one = book_with(1, false);
    let mut four = book_with(4, false);
    one.recalc().unwrap();
    four.recalc().unwrap();
    assert_eq!(four.model().last_evaluation_threads(), 4);
    assert_eq!(values(&one, 8000, 8), values(&four, 8000, 8));
    for wb in [&mut one, &mut four] {
        wb.set_input(0, 5, 2, "3").unwrap();
        wb.recalc().unwrap();
    }
    assert_eq!(values(&one, 8000, 8), values(&four, 8000, 8));
}

/// Heavy formulas on independent rows: how long a full recalculation takes on
/// one thread and on every core (`cargo test --release --test parallel -- --ignored --nocapture`).
#[test]
#[ignore]
fn heavy_formulas_timing() {
    for threads in [1, cellx_core::workbook::calc_threads().max(2)] {
        let mut wb = Workbook::new().unwrap();
        wb.set_calc_threads(threads);
        let mut cells = Vec::new();
        for r in 1..=3000 {
            cells.push((r, 1, format!("{}", r % 50)));
            cells.push((r, 2, format!("{}", (r * 7) % 13)));
        }
        for r in 1..=25000 {
            cells.push((r, 4, format!("=SUMPRODUCT(($A$1:$A$3000={})*$B$1:$B$3000)+{}", r % 50, r)));
        }
        wb.set_inputs(0, &cells).unwrap();
        let mut times = Vec::new();
        for _ in 0..3 {
            let t = std::time::Instant::now();
            wb.recalc().unwrap();
            times.push(t.elapsed().as_millis());
        }
        println!("threads {threads}: full recalc {times:?} ms, used {}", wb.model().last_evaluation_threads());
    }
}
