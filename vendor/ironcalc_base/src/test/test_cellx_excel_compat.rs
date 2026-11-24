#![allow(clippy::unwrap_used)]

//! CellX: behaviours changed to match Excel (see VENDORED.md, patches 24-28).

use crate::test::user_model::util::new_empty_user_model;
use crate::test::util::new_empty_model;

#[test]
fn deleted_sheet_reference_keeps_sheet_name() {
    let mut model = new_empty_user_model();
    model.new_sheet().unwrap();
    model
        .set_user_input(0, 1, 1, "=IFERROR(Sheet2!#REF!,\"caught\")")
        .unwrap();
    assert_eq!(model.get_formatted_cell_value(0, 1, 1), Ok("caught".to_string()));
    assert_eq!(
        model.get_cell_content(0, 1, 1),
        Ok("=IFERROR(Sheet2!#REF!,\"caught\")".to_string())
    );
}

#[test]
fn sheet_scoped_names() {
    let mut model = new_empty_user_model();
    model.new_sheet().unwrap();
    model.rename_sheet(1, "Q1 Sales").unwrap();
    model.set_user_input(1, 1, 1, "42").unwrap();
    model
        .new_defined_name("Total", Some(1), "'Q1 Sales'!$A$1")
        .unwrap();
    model.set_user_input(0, 1, 1, "='Q1 Sales'!Total+1").unwrap();
    assert_eq!(model.get_formatted_cell_value(0, 1, 1), Ok("43".to_string()));

    model.rename_sheet(1, "Data").unwrap();
    assert_eq!(model.get_cell_content(0, 1, 1), Ok("=Data!Total+1".to_string()));
    assert_eq!(model.get_formatted_cell_value(0, 1, 1), Ok("43".to_string()));
}

#[test]
fn formula_and_constant_names() {
    let mut model = new_empty_user_model();
    model.set_user_input(0, 1, 1, "10").unwrap();
    model.set_user_input(0, 2, 1, "20").unwrap();
    model.new_defined_name("Pi", None, "3.14159").unwrap();
    model.new_defined_name("Total", None, "SUM(Sheet1!$A$1:$A$2)").unwrap();
    model.new_defined_name("Again", None, "1").unwrap();
    model
        .update_defined_name("Again", None, "Again", None, "Again+1")
        .unwrap();
    model.set_user_input(0, 1, 2, "=Pi*2").unwrap();
    model.set_user_input(0, 2, 2, "=Total").unwrap();
    model.set_user_input(0, 3, 2, "=Again").unwrap();
    assert_eq!(model.get_formatted_cell_value(0, 1, 2), Ok("6.28318".to_string()));
    assert_eq!(model.get_formatted_cell_value(0, 2, 2), Ok("30".to_string()));
    assert_eq!(model.get_formatted_cell_value(0, 3, 2), Ok("#CIRC!".to_string()));
}

#[test]
fn single_argument_functions_over_arrays() {
    let mut model = new_empty_model();
    model._set("A1", "65");
    model._set("A2", "66");
    model._set("B1", "=CONCAT(CHAR(A1:A2))");
    model._set("B2", "=TEXTJOIN(\",\",TRUE,NOT({0,1}))");
    model._set("B3", "=SUM(--ISNUMBER({1,\"a\",3}))");
    model._set("B4", "=CONCAT(SEQUENCE(3))");
    model.evaluate();
    assert_eq!(model._get_text("B1"), *"AB");
    assert_eq!(model._get_text("B2"), *"TRUE,FALSE");
    assert_eq!(model._get_text("B3"), *"2");
    assert_eq!(model._get_text("B4"), *"123");
}

#[test]
fn numbers_as_text() {
    let mut model = new_empty_model();
    model._set("A1", "=\"x\"&1/3");
    model._set("A2", "=\"x\"&1E+20");
    model._set("A3", "=\"x\"&123456789012345678");
    model._set("A4", "=\"x\"&0.00001");
    model._set("A5", "=LEN(1/3)");
    model.evaluate();
    assert_eq!(model._get_text("A1"), *"x0.333333333333333");
    assert_eq!(model._get_text("A2"), *"x1E+20");
    assert_eq!(model._get_text("A3"), *"x1.23456789012346E+17");
    assert_eq!(model._get_text("A4"), *"x0.00001");
    assert_eq!(model._get_text("A5"), *"17");
}

#[test]
fn excel_1900_calendar() {
    let mut model = new_empty_model();
    model._set("A1", "=DATE(1900,2,29)");
    model._set("A2", "=TEXT(60,\"yyyy-mm-dd\")");
    model._set("A3", "=TEXT(0,\"m/d/yyyy\")");
    model._set("A4", "=WEEKDAY(1)");
    model._set("A5", "=TEXT(0.5006898,\"h:mm:ss.0\")");
    model._set("A6", "=TEXT(18/24+57/1440+59.6/86400,\"h:mm\")");
    model.evaluate();
    assert_eq!(model._get_text("A1"), *"2/29/1900");
    assert_eq!(model._get_text("A2"), *"1900-02-29");
    assert_eq!(model._get_text("A3"), *"1/0/1900");
    assert_eq!(model._get_text("A4"), *"1");
    assert_eq!(model._get_text("A5"), *"12:00:59.6");
    assert_eq!(model._get_text("A6"), *"18:58");
}

#[test]
fn date1904_does_not_leak_between_models() {
    let mut m1904 = new_empty_model();
    m1904.workbook.settings.date1904 = true;
    m1904._set("A1", "=YEAR(0)");
    m1904._set("A2", "=TEXT(0,\"yyyy-mm-dd\")");
    let mut m1900 = new_empty_model();
    m1900._set("A1", "=YEAR(0)");
    m1900._set("A2", "=TEXT(1,\"yyyy-mm-dd\")");

    m1904.evaluate();
    m1900.evaluate();
    // Evaluated in turn on one thread: each keeps its own date system,
    // also when read after the other was evaluated.
    assert_eq!(m1904._get_text("A1"), *"1904");
    assert_eq!(m1904._get_text("A2"), *"1904-01-01");
    assert_eq!(m1900._get_text("A1"), *"1900");
    assert_eq!(m1900._get_text("A2"), *"1900-01-01");
}

#[test]
fn scalar_functions_over_ranges_and_arrays() {
    let mut model = new_empty_model();
    model._set("A1", "1.4");
    model._set("A2", "2.6");
    model._set("A3", "3.5");
    model._set("B1", "=SUM(ROUND(A1:A3,0))");
    model._set("B2", "=INDEX(PMT({0.05,0.06},10,1000),2)");
    model._set("B3", "=SUM(DAYS({10,20},{1;2}))");
    model._set("B4", "=AVERAGEA({1,TRUE,\"a\"})");
    model._set("B5", "=SUM(HOUR({0.25,0.5}))");
    model._set("B6", "=LARGE(NA(),1)");
    model.evaluate();
    assert_eq!(model._get_text("B1"), *"8");
    assert_eq!(model._get_text("B2"), *"-135.86795822");
    assert_eq!(model._get_text("B3"), *"54");
    assert_eq!(model._get_text("B4"), *"0.666666667");
    assert_eq!(model._get_text("B5"), *"18");
    assert_eq!(model._get_text("B6"), *"#N/A");
}

#[test]
fn relative_names_and_their_dependencies() {
    let mut model = new_empty_user_model();
    // Relative row, counted from A1: used in row 3 it is column A of row 3.
    model.new_defined_name("RowValue", None, "Sheet1!$A1").unwrap();
    model.new_defined_name("Twice", None, "Sheet1!$A$1*2").unwrap();
    model.set_user_input(0, 1, 1, "5").unwrap();
    model.set_user_input(0, 3, 1, "7").unwrap();
    model.set_user_input(0, 3, 2, "=RowValue").unwrap();
    model.set_user_input(0, 4, 2, "=Twice").unwrap();
    assert_eq!(model.get_formatted_cell_value(0, 3, 2), Ok("7".to_string()));
    assert_eq!(model.get_formatted_cell_value(0, 4, 2), Ok("10".to_string()));

    // An incremental recalculation reaches the cells through the names.
    let m = model.get_model_mut();
    m.evaluate();
    m._set("A1", "6");
    m._set("A3", "8");
    m.evaluate_changed(&[(0, 1, 1), (0, 3, 1)]);
    assert_eq!(m._get_text("B3"), *"8");
    assert_eq!(m._get_text("B4"), *"12");
}
