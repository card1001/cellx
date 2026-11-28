use cellx_core::data::{ColumnFilter, DataStore, Sort};
use cellx_core::Scalar;

fn ids(page: &cellx_core::data::Page) -> Vec<i64> {
    page.row_ids.clone()
}

#[test]
fn sorted_pages_are_deterministic_across_duplicate_and_null_keys() {
    let mut store = DataStore::open_in_memory().unwrap();
    store.create_table_from_sql(
        "items",
        "SELECT * FROM (VALUES
            ('a', 2), ('b', 1), ('c', 2), ('d', NULL),
            ('e', 1), ('f', NULL), ('g', 2), ('h', 1)
        ) AS t(label, rank)",
    ).unwrap();

    let sort = Sort { column: "rank".into(), descending: false };
    let first = store.page("items", 0, 3, Some(&sort), &[]).unwrap();
    let second = store.page("items", 3, 3, Some(&sort), &[]).unwrap();
    let third = store.page("items", 6, 3, Some(&sort), &[]).unwrap();

    let all_ids = [ids(&first), ids(&second), ids(&third)].concat();
    assert_eq!(all_ids.len(), 8);
    let mut unique = all_ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 8, "pages must neither duplicate nor omit row ids");

    // Equal sort values are ordered by _rid; NULL values are always last.
    assert_eq!(first.rows.iter().map(|r| r[0].clone()).collect::<Vec<_>>(), vec![
        Scalar::Text("b".into()), Scalar::Text("e".into()), Scalar::Text("h".into())
    ]);
    assert_eq!(second.rows.iter().map(|r| r[0].clone()).collect::<Vec<_>>(), vec![
        Scalar::Text("a".into()), Scalar::Text("c".into()), Scalar::Text("g".into())
    ]);
    assert_eq!(third.rows.iter().map(|r| r[0].clone()).collect::<Vec<_>>(), vec![
        Scalar::Text("d".into()), Scalar::Text("f".into())
    ]);
    assert_eq!(first.total, 8);
    assert_eq!(third.total, 8);
}

#[test]
fn descending_sorted_pages_keep_duplicate_tie_breaker_and_nulls_last() {
    let mut store = DataStore::open_in_memory().unwrap();
    store.create_table_from_sql(
        "items",
        "SELECT * FROM (VALUES ('a', 2), ('b', 1), ('c', 2), ('d', NULL), ('e', 1)) AS t(label, rank)",
    ).unwrap();

    let sort = Sort { column: "rank".into(), descending: true };
    let first = store.page("items", 0, 2, Some(&sort), &[]).unwrap();
    let second = store.page("items", 2, 2, Some(&sort), &[]).unwrap();
    let third = store.page("items", 4, 2, Some(&sort), &[]).unwrap();
    let labels = [first, second, third].into_iter()
        .flat_map(|p| p.rows.into_iter().map(|r| r[0].clone()))
        .collect::<Vec<_>>();

    assert_eq!(labels, vec![
        Scalar::Text("a".into()), Scalar::Text("c".into()),
        Scalar::Text("b".into()), Scalar::Text("e".into()),
        Scalar::Text("d".into())
    ]);
}

#[test]
fn sorted_filtered_view_is_rebuilt_after_sort_key_mutation() {
    let mut store = DataStore::open_in_memory().unwrap();
    store.create_table_from_sql(
        "items",
        "SELECT * FROM (VALUES ('a', 10, 'keep'), ('b', 20, 'keep'), ('c', 30, 'skip'), ('d', 40, 'keep')) AS t(label, rank, category)",
    ).unwrap();
    let sort = Sort { column: "rank".into(), descending: false };
    let filters = [ColumnFilter { column: "category".into(), criterion: "keep".into() }];

    let before = store.page("items", 0, 10, Some(&sort), &filters).unwrap();
    assert_eq!(before.rows.len(), 3);
    let changed_rid = before.row_ids[2];
    store.update_cell("items", changed_rid, "rank", "5").unwrap();

    let after = store.page("items", 0, 10, Some(&sort), &filters).unwrap();
    assert_eq!(after.total, 3);
    assert_eq!(after.rows[0][0], Scalar::Text("d".into()));
    assert_eq!(after.row_ids[0], changed_rid);
}
