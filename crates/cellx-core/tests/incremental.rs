//! Incremental recalculation must give exactly what a full one gives.

use cellx_core::Workbook;

/// Every shown value of the first `rows` × `cols` cells of the first two sheets.
fn snapshot(wb: &Workbook, rows: i32, cols: i32) -> Vec<String> {
    let mut out = Vec::new();
    for sheet in 0..2 {
        for r in 1..=rows {
            for c in 1..=cols {
                out.push(wb.model().get_formatted_cell_value(sheet, r, c).unwrap_or_default());
            }
        }
    }
    out
}

/// After an incremental recalculation, a full one must change nothing.
fn check(wb: &mut Workbook, what: &str) {
    let partial = snapshot(wb, 30, 8);
    wb.recalc().unwrap();
    let full = snapshot(wb, 30, 8);
    for (i, (a, b)) in partial.iter().zip(&full).enumerate() {
        let (sheet, rest) = (i / 240, i % 240);
        assert_eq!(a, b, "{what}: sheet {sheet} row {} col {}", rest / 8 + 1, rest % 8 + 1);
    }
}

fn book() -> Workbook {
    let mut wb = Workbook::new().unwrap();
    wb.add_sheet().unwrap();
    let mut cells = Vec::new();
    for r in 1..=20 {
        cells.push((r, 1, format!("{}", r * 3 % 7)));
        cells.push((r, 2, format!("=A{r}*2")));
        // A chain through the rows: each depends on the one above.
        cells.push((r, 3, if r == 1 { "=B1".to_string() } else { format!("=C{}+B{r}", r - 1) }));
        cells.push((r, 4, format!("=SUM($A$1:A{r})")));
        cells.push((r, 5, format!("=IF(C{r}>50,\"큼\",\"작음\")")));
    }
    cells.push((22, 1, "=SUM(A1:E20)".into()));
    cells.push((23, 1, "=VLOOKUP(5,A1:B20,2,FALSE)".into()));
    cells.push((24, 1, "=INDIRECT(\"A\"&H1)".into()));
    cells.push((25, 1, "=COUNTIF(A:A,\">3\")".into()));
    cells.push((26, 1, "=SUMPRODUCT(A1:A20,B1:B20)".into()));
    cells.push((27, 1, "=LET(x,A1+A2,x*x)".into()));
    cells.push((1, 8, "3".into()));
    wb.set_inputs(0, &cells).unwrap();
    // Another sheet reads this one, and this one reads it back.
    wb.set_inputs(
        1,
        &[
            (1, 1, "=Sheet1!C20*10".into()),
            (2, 1, "=SUM(Sheet1!B1:B20)".into()),
            (3, 1, "7".into()),
        ],
    )
    .unwrap();
    wb.set_input(0, 28, 1, "=Sheet2!A1+Sheet2!A3").unwrap();
    wb
}

#[test]
fn edits_match_full_recalculation() {
    let mut wb = book();
    check(&mut wb, "start");

    wb.set_input(0, 5, 1, "100").unwrap();
    assert!(wb.last_recalc.evaluated.is_some(), "an edit should be incremental");
    check(&mut wb, "value at the top of a chain");

    wb.set_input(0, 20, 1, "1").unwrap();
    check(&mut wb, "value at the bottom");

    wb.set_input(0, 1, 8, "7").unwrap();
    check(&mut wb, "INDIRECT target changes");

    wb.set_input(1, 3, 1, "-4").unwrap();
    check(&mut wb, "other sheet");

    // A formula replaced by another that reads other cells.
    wb.set_input(0, 10, 2, "=A11*100").unwrap();
    check(&mut wb, "formula rewritten");
    wb.set_input(0, 11, 1, "9").unwrap();
    check(&mut wb, "new precedent");
    wb.set_input(0, 10, 1, "50").unwrap();
    check(&mut wb, "old precedent no longer read");

    // A formula turned into a value, and a value into a formula.
    wb.set_input(0, 12, 3, "1000").unwrap();
    check(&mut wb, "formula → value");
    wb.set_input(0, 12, 3, "=C11+B12").unwrap();
    check(&mut wb, "value → formula");

    // Typing into an empty cell inside a range that formulas read.
    wb.set_input(0, 30, 1, "5").unwrap();
    check(&mut wb, "whole-column range");

    // Clearing.
    wb.clear_contents(0, cellx_core::workbook::CellRange { r0: 2, c0: 1, r1: 4, c1: 1 }).unwrap();
    check(&mut wb, "clear contents");

    // Undo and redo go through the same path.
    for i in 0..4 {
        wb.undo().unwrap();
        check(&mut wb, &format!("undo {i}"));
    }
    for i in 0..4 {
        wb.redo().unwrap();
        check(&mut wb, &format!("redo {i}"));
    }

    // Rows inserted: a full recalculation, then incremental again.
    wb.insert_rows(0, 3, 2).unwrap();
    assert!(wb.last_recalc.evaluated.is_none());
    check(&mut wb, "after insert");
    wb.set_input(0, 1, 1, "42").unwrap();
    assert!(wb.last_recalc.evaluated.is_some());
    check(&mut wb, "edit after insert");
}

#[test]
fn cycles_made_and_broken() {
    let mut wb = Workbook::new().unwrap();
    wb.set_inputs(0, &[(1, 1, "=B1".into()), (1, 2, "1".into()), (1, 3, "=A1+1".into())]).unwrap();
    wb.set_input(0, 1, 2, "=A1").unwrap();
    let shown = |wb: &Workbook, c| wb.model().get_formatted_cell_value(0, 1, c).unwrap();
    assert_eq!(shown(&wb, 1), "#CIRC!");
    check(&mut wb, "cycle made");
    wb.set_input(0, 1, 2, "5").unwrap();
    assert_eq!((shown(&wb, 1), shown(&wb, 3)), ("5".into(), "6".into()));
    check(&mut wb, "cycle broken");
}

#[test]
fn only_dependents_are_evaluated() {
    let mut wb = Workbook::new().unwrap();
    let mut cells = Vec::new();
    for r in 1..=2000 {
        cells.push((r, 1, format!("{r}")));
        cells.push((r, 2, format!("=A{r}*2")));
    }
    wb.set_inputs(0, &cells).unwrap();
    wb.set_input(0, 7, 1, "1").unwrap();
    assert_eq!(wb.last_recalc.evaluated, Some(1));
    assert_eq!(wb.model().get_formatted_cell_value(0, 7, 2).unwrap(), "2");
    // Formatting alone evaluates nothing but volatile formulas.
    wb.set_style(0, cellx_core::workbook::CellRange { r0: 1, c0: 1, r1: 5, c1: 1 }, "font.b", "true").unwrap();
    assert_eq!(wb.last_recalc.evaluated, Some(0));
}

#[test]
fn lookups_through_the_index_match_excel() {
    let mut wb = Workbook::new().unwrap();
    let data: &[(&str, &str)] = &[
        ("사과", "1"),
        ("Banana", "2"),
        ("사과", "3"),
        ("5", "4"),
        ("", "5"),
        ("TRUE", "6"),
        ("0.3", "7"),
        ("cherry pie", "8"),
    ];
    let mut cells = Vec::new();
    for (i, (k, v)) in data.iter().enumerate() {
        let r = i as i32 + 1;
        if !k.is_empty() {
            cells.push((r, 1, k.to_string()));
        }
        cells.push((r, 2, v.to_string()));
    }
    // A text "5" in A10 and the number 5 above it as text-looking input.
    cells.push((10, 1, "'5".into()));
    cells.push((10, 2, "10".into()));
    let formulas = [
        ("=VLOOKUP(\"사과\",A1:B10,2,FALSE)", "1"),
        ("=VLOOKUP(\"banana\",A1:B10,2,FALSE)", "2"),
        ("=VLOOKUP(5,A1:B10,2,FALSE)", "4"),
        ("=VLOOKUP(\"5\",A1:B10,2,FALSE)", "10"),
        ("=VLOOKUP(0,A1:B10,2,FALSE)", "5"),
        ("=VLOOKUP(TRUE,A1:B10,2,FALSE)", "6"),
        ("=VLOOKUP(0.1+0.2,A1:B10,2,FALSE)", "7"),
        ("=VLOOKUP(\"cher*\",A1:B10,2,FALSE)", "8"),
        ("=VLOOKUP(\"없음\",A1:B10,2,FALSE)", "#N/A"),
        ("=MATCH(\"사과\",A1:A10,0)", "1"),
        ("=MATCH(5,A1:A10,0)", "4"),
        ("=MATCH(\"?anana\",A1:A10,0)", "2"),
        ("=MATCH(0,A1:A10,0)", "#N/A"),
        ("=HLOOKUP(\"y\",D1:G2,2,FALSE)", "20"),
        ("=VLOOKUP(2.5,B1:B8,1,TRUE)", "2"),
    ];
    cells.extend([(1, 4, "x".into()), (1, 5, "y".into()), (1, 6, "y".into()), (2, 4, "10".into()), (2, 5, "20".into()), (2, 6, "30".into())]);
    for (i, (f, _)) in formulas.iter().enumerate() {
        cells.push((20 + i as i32, 1, f.to_string()));
    }
    wb.set_inputs(0, &cells).unwrap();
    for (i, (f, want)) in formulas.iter().enumerate() {
        let got = wb.model().get_formatted_cell_value(0, 20 + i as i32, 1).unwrap();
        assert_eq!(&got, want, "{f}");
    }
    // The index lives for one pass only: a changed key is found next time.
    wb.set_input(0, 1, 1, "포도").unwrap();
    assert_eq!(wb.model().get_formatted_cell_value(0, 20, 1).unwrap(), "3");
    check(&mut wb, "lookup range edited");
}

#[test]
fn inserted_rows_move_references_like_excel() {
    let mut wb = Workbook::new().unwrap();
    wb.add_sheet().unwrap();
    let cells: Vec<(i32, i32, String)> = vec![
        (1, 1, "1".into()),
        (2, 1, "2".into()),
        (3, 1, "3".into()),
        (4, 1, "4".into()),
        (5, 1, "5".into()),
        // Above the insert, reading across it.
        (1, 2, "=A5".into()),
        (1, 3, "=$A$5+A$4+$A3".into()),
        (1, 4, "=SUM(A2:A5)".into()),
        (1, 5, "=SUM(A:A)".into()),
        (2, 2, "=A1".into()),
        // Below the insert, reading above, across and below.
        (4, 2, "=A1+A4".into()),
        (4, 3, "=SUM($A$1:A4)".into()),
        (5, 2, "=SUM(A3:A5)*$A$1".into()),
        (5, 3, "=COUNTIF(A1:A5,\">2\")".into()),
    ];
    wb.set_inputs(0, &cells).unwrap();
    wb.set_inputs(1, &[(1, 1, "=Sheet1!A4+Sheet1!$A$2".into()), (2, 1, "=SUM(Sheet1!A1:A5)".into())]).unwrap();
    let before: Vec<String> = (1..=8).map(|r| wb.model().get_formatted_cell_value(0, r, 2).unwrap()).collect();

    wb.insert_rows(0, 3, 2).unwrap();
    let input = |wb: &Workbook, s, r, c| wb.input(s, r, c).unwrap();
    assert_eq!(input(&wb, 0, 1, 2), "=A7");
    assert_eq!(input(&wb, 0, 1, 3), "=$A$7+A$6+$A5");
    assert_eq!(input(&wb, 0, 1, 4), "=SUM(A2:A7)");
    assert_eq!(input(&wb, 0, 1, 5), "=SUM(A:A)");
    assert_eq!(input(&wb, 0, 2, 2), "=A1");
    assert_eq!(input(&wb, 0, 6, 2), "=A1+A6");
    assert_eq!(input(&wb, 0, 6, 3), "=SUM($A$1:A6)");
    assert_eq!(input(&wb, 0, 7, 2), "=SUM(A5:A7)*$A$1");
    assert_eq!(input(&wb, 0, 7, 3), "=COUNTIF(A1:A7,\">2\")");
    assert_eq!(input(&wb, 0, 7, 1), "5");
    assert_eq!(input(&wb, 0, 3, 1), "");
    assert_eq!(input(&wb, 1, 1, 1), "=Sheet1!A6+Sheet1!$A$2");
    assert_eq!(input(&wb, 1, 2, 1), "=SUM(Sheet1!A1:A7)");
    // Same values, shifted.
    assert_eq!(wb.model().get_formatted_cell_value(0, 1, 2).unwrap(), before[0]);
    assert_eq!(wb.model().get_formatted_cell_value(0, 6, 2).unwrap(), before[3]);
    assert_eq!(wb.model().get_formatted_cell_value(0, 7, 2).unwrap(), before[4]);
    check(&mut wb, "after fast insert");

    wb.undo().unwrap();
    assert_eq!(input(&wb, 0, 4, 2), "=A1+A4");
    assert_eq!(input(&wb, 0, 1, 3), "=$A$5+A$4+$A3");
    assert_eq!(input(&wb, 1, 2, 1), "=SUM(Sheet1!A1:A5)");
    check(&mut wb, "undo insert");
}

#[test]
fn deleted_rows_move_references_like_excel() {
    let mut wb = Workbook::new().unwrap();
    wb.add_sheet().unwrap();
    let mut cells: Vec<(i32, i32, String)> = (1..=8).map(|r| (r, 1, format!("{}", r * 10))).collect();
    cells.extend([
        (1, 2, "=A8".to_string()),
        (1, 3, "=SUM(A1:A8)".to_string()),
        (1, 4, "=A3".to_string()),
        (1, 5, "=$A$6*2".to_string()),
        (1, 6, "=SUM(A:A)".to_string()),
        (7, 2, "=A1+A7".to_string()),
        (8, 2, "=SUM(A6:A8)".to_string()),
    ]);
    wb.set_inputs(0, &cells).unwrap();
    wb.set_inputs(1, &[(1, 1, "=Sheet1!A8".into())]).unwrap();
    wb.delete_rows(0, 3, 2).unwrap();
    let input = |wb: &Workbook, s, r, c| wb.input(s, r, c).unwrap();
    assert_eq!(input(&wb, 0, 1, 2), "=A6");
    assert_eq!(input(&wb, 0, 1, 3), "=SUM(A1:A6)");
    assert!(input(&wb, 0, 1, 4).contains("#REF!"), "{}", input(&wb, 0, 1, 4));
    assert_eq!(input(&wb, 0, 1, 5), "=$A$4*2");
    assert_eq!(input(&wb, 0, 1, 6), "=SUM(A:A)");
    assert_eq!(input(&wb, 0, 5, 2), "=A1+A5");
    assert_eq!(input(&wb, 0, 6, 2), "=SUM(A4:A6)");
    assert_eq!(input(&wb, 0, 3, 1), "50");
    assert_eq!(input(&wb, 1, 1, 1), "=Sheet1!A6");
    let v = |wb: &Workbook, r, c| wb.model().get_formatted_cell_value(0, r, c).unwrap();
    assert_eq!((v(&wb, 1, 2), v(&wb, 1, 3), v(&wb, 1, 5)), ("80".into(), "290".into(), "120".into()));
    check(&mut wb, "after delete");
    wb.undo().unwrap();
    assert_eq!(input(&wb, 0, 1, 4), "=A3");
    assert_eq!(input(&wb, 0, 8, 2), "=SUM(A6:A8)");
    check(&mut wb, "undo delete");
}

#[test]
fn hiding_rows_updates_subtotal_incrementally() {
    use cellx_core::refs::Axis;
    let mut wb = Workbook::new().unwrap();
    let mut cells: Vec<(i32, i32, String)> = (1..=10).map(|r| (r, 1, format!("{r}"))).collect();
    cells.push((12, 1, "=SUBTOTAL(109,A1:A10)".into()));
    cells.push((13, 1, "=SUM(A1:A10)".into()));
    cells.push((14, 1, "=A13*2".into()));
    wb.set_inputs(0, &cells).unwrap();
    let v = |wb: &Workbook, r| wb.model().get_formatted_cell_value(0, r, 1).unwrap();
    assert_eq!(v(&wb, 12), "55");
    wb.set_hidden(0, Axis::Row, 2, 4, true).unwrap();
    assert!(wb.last_recalc.evaluated.is_some(), "hiding rows should be incremental");
    assert_eq!((v(&wb, 12), v(&wb, 13)), ("46".into(), "55".into()));
    check(&mut wb, "rows hidden");
    // Undo goes the same way, and so does a plain edit after it.
    wb.undo().unwrap();
    assert!(wb.last_recalc.evaluated.is_some(), "undo should be incremental");
    assert_eq!(v(&wb, 12), "55");
    wb.set_input(0, 1, 1, "100").unwrap();
    assert_eq!((v(&wb, 12), v(&wb, 14)), ("154".into(), "308".into()));
    check(&mut wb, "edit after undo");
}

#[test]
fn sorting_moves_whole_cells() {
    use cellx_core::datatools::SortKey;
    use cellx_core::workbook::CellRange;
    let mut wb = Workbook::new().unwrap();
    let cells: Vec<(i32, i32, String)> = vec![
        (1, 1, "3".into()),
        (1, 2, "=A1*10+$D$1".into()),
        (1, 3, "=0.1+0.2".into()),
        (2, 1, "1".into()),
        (2, 2, "=A2*10+$D$1".into()),
        (2, 3, "0.30000000000000004".into()),
        (3, 1, "2".into()),
        (3, 2, "=A1+A3".into()),
        (4, 1, "2".into()),
        (4, 2, "dup".into()),
        (1, 4, "1000".into()),
    ];
    wb.set_inputs(0, &cells).unwrap();
    // A style on an empty cell travels with its row too.
    wb.set_style(0, CellRange { r0: 2, c0: 3, r1: 2, c1: 3 }, "font.b", "true").unwrap();
    wb.set_style(0, CellRange { r0: 3, c0: 3, r1: 3, c1: 3 }, "font.i", "true").unwrap();
    let full = CellRange { r0: 1, c0: 1, r1: 4, c1: 3 };
    let exact = |wb: &Workbook, r, c| match wb.model().get_cell_value_by_index(0, r, c).unwrap() {
        ironcalc_base::cell::CellValue::Number(n) => n,
        other => panic!("{other:?}"),
    };
    let before = exact(&wb, 2, 3);
    wb.sort_range(0, full, &[SortKey { col: 1, descending: false }], false).unwrap();
    let input = |wb: &Workbook, r, c| wb.input(0, r, c).unwrap();
    let shown = |wb: &Workbook, r, c| wb.model().get_formatted_cell_value(0, r, c).unwrap();
    assert_eq!((input(&wb, 1, 1), input(&wb, 2, 1), input(&wb, 3, 1), input(&wb, 4, 1)), ("1".into(), "2".into(), "2".into(), "3".into()));
    // Relative references move with the row, absolute ones stay.
    assert_eq!(input(&wb, 1, 2), "=A1*10+$D$1");
    assert_eq!(input(&wb, 4, 2), "=A4*10+$D$1");
    // The row that read "the row two above" still does (now above the top: #REF!), as Excel does.
    assert_eq!(input(&wb, 2, 2), "=#REF!+A2");
    assert_eq!(input(&wb, 3, 2), "dup");
    assert_eq!(shown(&wb, 1, 2), "1010");
    // The value is the very same number, not one rounded through text.
    assert_eq!(exact(&wb, 1, 3), before);
    assert!(wb.model().get_style_for_cell(0, 1, 3).unwrap().font.b);
    assert!(wb.model().get_style_for_cell(0, 2, 3).unwrap().font.i);
    check(&mut wb, "after sort");
    wb.undo().unwrap();
    assert_eq!((input(&wb, 1, 1), input(&wb, 3, 2)), ("3".into(), "=A1+A3".into()));
    assert!(wb.model().get_style_for_cell(0, 2, 3).unwrap().font.b);
    check(&mut wb, "undo sort");
    wb.redo().unwrap();
    assert_eq!(input(&wb, 4, 2), "=A4*10+$D$1");
    check(&mut wb, "redo sort");

    // Remove duplicates by column A: rows with 2 keep the first.
    let result = wb.remove_duplicates(0, full, &[1], false).unwrap();
    assert_eq!((result.removed, result.remaining), (1, 3));
    assert_eq!((input(&wb, 3, 1), input(&wb, 4, 1), input(&wb, 4, 2)), ("3".into(), "".into(), "".into()));
    check(&mut wb, "after remove duplicates");
    wb.undo().unwrap();
    assert_eq!(input(&wb, 4, 1), "3");
}

#[test]
fn sorting_with_an_array_formula_goes_the_text_way() {
    use cellx_core::datatools::SortKey;
    use cellx_core::workbook::CellRange;
    let mut wb = Workbook::new().unwrap();
    wb.set_inputs(0, &[(1, 1, "2".into()), (2, 1, "1".into()), (1, 2, "=SEQUENCE(2)".into())]).unwrap();
    wb.sort_range(0, CellRange { r0: 1, c0: 1, r1: 2, c1: 1 }, &[SortKey { col: 1, descending: false }], false).unwrap();
    assert_eq!(wb.input(0, 1, 1).unwrap(), "1");
    check(&mut wb, "sort next to a spill");
}

#[test]
fn columns_inserted_and_deleted_move_references_like_excel() {
    let mut wb = Workbook::new().unwrap();
    wb.add_sheet().unwrap();
    let cells: Vec<(i32, i32, String)> = vec![
        (1, 1, "1".into()),
        (1, 2, "2".into()),
        (1, 3, "3".into()),
        (1, 4, "4".into()),
        (2, 1, "=D1".into()),
        (2, 2, "=SUM(A1:D1)".into()),
        (2, 3, "=$C$1+B1".into()),
        (2, 4, "=SUM(1:1)".into()),
        (2, 5, "=A1+C1".into()),
    ];
    wb.set_inputs(0, &cells).unwrap();
    wb.set_inputs(1, &[(1, 1, "=Sheet1!C1+Sheet1!$D$1".into())]).unwrap();
    let input = |wb: &Workbook, s, r, c| wb.input(s, r, c).unwrap();
    wb.insert_columns(0, 2, 2).unwrap();
    assert_eq!(input(&wb, 0, 2, 1), "=F1");
    assert_eq!(input(&wb, 0, 2, 4), "=SUM(A1:F1)");
    assert_eq!(input(&wb, 0, 2, 5), "=$E$1+D1");
    assert_eq!(input(&wb, 0, 2, 6), "=SUM(1:1)");
    assert_eq!(input(&wb, 0, 2, 7), "=A1+E1");
    assert_eq!(input(&wb, 0, 1, 4), "2");
    assert_eq!(input(&wb, 1, 1, 1), "=Sheet1!E1+Sheet1!$F$1");
    check(&mut wb, "columns inserted");
    wb.undo().unwrap();
    assert_eq!(input(&wb, 0, 2, 2), "=SUM(A1:D1)");
    check(&mut wb, "insert undone");

    wb.delete_columns(0, 3, 1).unwrap();
    assert_eq!(input(&wb, 0, 2, 1), "=C1");
    assert_eq!(input(&wb, 0, 2, 2), "=SUM(A1:C1)");
    assert!(input(&wb, 0, 2, 4).contains("#REF!"), "{}", input(&wb, 0, 2, 4));
    assert_eq!(input(&wb, 0, 2, 3), "=SUM(1:1)");
    assert!(input(&wb, 1, 1, 1).contains("#REF!"));
    check(&mut wb, "column deleted");
    wb.undo().unwrap();
    assert_eq!(input(&wb, 0, 2, 5), "=A1+C1");
    assert_eq!(input(&wb, 1, 1, 1), "=Sheet1!C1+Sheet1!$D$1");
    assert_eq!(input(&wb, 0, 2, 3), "=$C$1+B1");
    check(&mut wb, "delete undone");
}
