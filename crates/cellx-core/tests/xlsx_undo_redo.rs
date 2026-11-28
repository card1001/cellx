use cellx_core::Workbook;

#[test]
fn undo_redo_and_xlsx_roundtrip_preserve_cell_inputs_and_formulas() {
    let path = std::env::temp_dir().join(format!("cellx-undo-redo-{}.xlsx", std::process::id()));
    let _ = std::fs::remove_file(&path);

    let mut workbook = Workbook::new().unwrap();
    workbook.set_input(0, 1, 1, "10").unwrap();
    workbook.set_input(0, 1, 2, "=A1*2").unwrap();
    workbook.set_input(0, 1, 1, "20").unwrap();

    workbook.undo().unwrap();
    assert_eq!(workbook.detail(0, 1, 1).unwrap().input, "10");
    assert_eq!(workbook.detail(0, 1, 2).unwrap().input, "=A1*2");

    workbook.redo().unwrap();
    assert_eq!(workbook.detail(0, 1, 1).unwrap().input, "20");
    assert_eq!(workbook.detail(0, 1, 2).unwrap().input, "=A1*2");

    workbook.save_xlsx(&path).unwrap();
    let reopened = Workbook::open_xlsx(&path).unwrap();
    assert_eq!(reopened.detail(0, 1, 1).unwrap().input, "20");
    assert_eq!(reopened.detail(0, 1, 2).unwrap().input, "=A1*2");

    let _ = std::fs::remove_file(path);
}

#[test]
fn redo_is_cleared_after_a_new_edit_following_undo() {
    let mut workbook = Workbook::new().unwrap();
    workbook.set_input(0, 1, 1, "first").unwrap();
    workbook.set_input(0, 1, 1, "second").unwrap();

    workbook.undo().unwrap();
    assert_eq!(workbook.detail(0, 1, 1).unwrap().input, "first");

    workbook.set_input(0, 1, 1, "branch").unwrap();
    workbook.redo().unwrap();
    assert_eq!(
        workbook.detail(0, 1, 1).unwrap().input,
        "branch",
        "a new edit must invalidate the abandoned redo branch"
    );
}
