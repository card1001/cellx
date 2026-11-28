use cellx_core::data::{ColumnFilter, DataStore, Sort, SortCursor};
use cellx_core::Scalar;

fn labels(page: &cellx_core::data::Page) -> Vec<String> {
    page.rows.iter().map(|row| row[0].display()).collect()
}

#[test]
fn sorted_keyset_pages_are_stable_for_duplicates_and_nulls_ascending() {
    let mut store = DataStore::open_in_memory().unwrap();
    store.create_table_from_sql(
        "items",
        "SELECT * FROM (VALUES
            ('a', 2), ('b', 1), ('c', 2), ('d', NULL),
            ('e', 1), ('f', NULL), ('g', 2), ('h', 1)
        ) AS t(label, rank)",
    ).unwrap();

    let sort = Sort { column: "rank".into(), descending: false };
    let first = store.page_after_sort("items", None, 3, &sort, &[]).unwrap();
    let second = store.page_after_sort("items", first.next_cursor.as_ref(), 3, &sort, &[]).unwrap();
    let third = store.page_after_sort("items", second.next_cursor.as_ref(), 3, &sort, &[]).unwrap();

    assert_eq!(labels(&first.page), vec!["b", "e", "h"]);
    assert_eq!(labels(&second.page), vec!["a", "c", "g"]);
    assert_eq!(labels(&third.page), vec!["d", "f"]);
    assert!(third.next_cursor.is_none());
    assert_eq!(first.page.total, 8);
    let ids = [first.page.row_ids, second.page.row_ids, third.page.row_ids].concat();
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 8);
}

#[test]
fn sorted_keyset_pages_keep_nulls_last_for_descending_order() {
    let mut store = DataStore::open_in_memory().unwrap();
    store.create_table_from_sql(
        "items",
        "SELECT * FROM (VALUES ('a', 2), ('b', 1), ('c', 2), ('d', NULL), ('e', 1)) AS t(label, rank)",
    ).unwrap();
    let sort = Sort { column: "rank".into(), descending: true };

    let first = store.page_after_sort("items", None, 2, &sort, &[]).unwrap();
    let second = store.page_after_sort("items", first.next_cursor.as_ref(), 2, &sort, &[]).unwrap();
    let third = store.page_after_sort("items", second.next_cursor.as_ref(), 2, &sort, &[]).unwrap();

    assert_eq!([labels(&first.page), labels(&second.page), labels(&third.page)].concat(),
        vec!["a", "c", "b", "e", "d"]);
    assert!(third.next_cursor.is_none());
}

#[test]
fn sorted_keyset_cursor_supports_text_and_filtered_views() {
    let mut store = DataStore::open_in_memory().unwrap();
    store.create_table_from_sql(
        "items",
        "SELECT * FROM (VALUES
            ('a', 'z', 'keep'), ('b', 'a', 'keep'), ('c', 'm', 'skip'),
            ('d', 'b', 'keep'), ('e', 'c', 'keep')
        ) AS t(label, rank, category)",
    ).unwrap();
    let sort = Sort { column: "rank".into(), descending: false };
    let filters = [ColumnFilter { column: "category".into(), criterion: "keep".into() }];

    let first = store.page_after_sort("items", None, 2, &sort, &filters).unwrap();
    let second = store.page_after_sort("items", first.next_cursor.as_ref(), 2, &sort, &filters).unwrap();
    assert_eq!([labels(&first.page), labels(&second.page)].concat(), vec!["b", "d", "e", "a"]);
    assert_eq!(first.page.total, 4);
}

#[test]
fn sorted_keyset_cursor_is_bound_to_stable_row_id_tie_breaker() {
    let mut store = DataStore::open_in_memory().unwrap();
    store.create_table_from_sql(
        "items",
        "SELECT * FROM (VALUES ('a', 1), ('b', 1), ('c', 1), ('d', 1)) AS t(label, rank)",
    ).unwrap();
    let sort = Sort { column: "rank".into(), descending: false };
    let first = store.page_after_sort("items", None, 2, &sort, &[]).unwrap();
    let cursor: SortCursor = first.next_cursor.unwrap();
    let second = store.page_after_sort("items", Some(&cursor), 2, &sort, &[]).unwrap();
    assert_eq!([labels(&first.page), labels(&second.page)].concat(), vec!["a", "b", "c", "d"]);
}
