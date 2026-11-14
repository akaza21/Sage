use super::*;

const NOW: i64 = 1_800_000_000_000;

fn visit(tab: &str, url: &str, visited_at: i64) -> Write {
    Write::Visit {
        tab: tab.into(),
        url: url.into(),
        title: None,
        visited_at,
        same_document: false,
    }
}

fn moved(tab: &str, url: &str, title: Option<&str>, visited_at: i64) -> Write {
    Write::Visit {
        tab: tab.into(),
        url: url.into(),
        title: title.map(str::to_owned),
        visited_at,
        same_document: true,
    }
}

fn title(tab: &str, title: &str) -> Write {
    Write::Title {
        tab: tab.into(),
        title: title.into(),
    }
}

fn rows(connection: &Connection) -> Vec<(String, Option<String>)> {
    store::page(connection, "", None, 100)
        .unwrap()
        .visits
        .into_iter()
        .rev()
        .map(|visit| (visit.url, visit.title))
        .collect()
}

fn row(url: &str, title: Option<&str>) -> (String, Option<String>) {
    (url.into(), title.map(str::to_owned))
}

#[test]
fn a_failed_commit_keeps_each_tabs_committed_visit() {
    let mut connection = store::open(":memory:").unwrap();
    let mut writer = Writer::default();
    writer
        .apply(
            &mut connection,
            &[visit("browser-1", "https://a.example/", NOW)],
            NOW,
        )
        .unwrap();
    // This constraint is deferred until COMMIT, after the writer has
    // inserted and remembered the candidate visit.
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE commit_parent (id INTEGER PRIMARY KEY);
             CREATE TABLE commit_child (
               parent INTEGER REFERENCES commit_parent(id) DEFERRABLE INITIALLY DEFERRED
             );
             CREATE TRIGGER fail_history_commit AFTER INSERT ON browser_visits
             BEGIN INSERT INTO commit_child(parent) VALUES (1); END;",
        )
        .unwrap();
    assert!(
        writer
            .apply(
                &mut connection,
                &[visit("browser-1", "https://failed.example/", NOW + 1)],
                NOW,
            )
            .is_err()
    );
    let rolled_back_id = connection.last_insert_rowid();
    assert_eq!(rows(&connection), [row("https://a.example/", None)]);
    connection
        .execute_batch("DROP TRIGGER fail_history_commit;")
        .unwrap();
    writer
        .apply(
            &mut connection,
            &[
                visit("browser-2", "https://b.example/", NOW + 2),
                title("browser-2", "From two"),
            ],
            NOW,
        )
        .unwrap();
    let second = store::page(&connection, "b.example", None, 1).unwrap();
    assert_eq!(second.visits[0].id, rolled_back_id);
    writer
        .apply(&mut connection, &[title("browser-1", "From one")], NOW)
        .unwrap();
    assert_eq!(
        rows(&connection),
        [
            row("https://a.example/", Some("From one")),
            row("https://b.example/", Some("From two")),
        ]
    );
    writer
        .apply(
            &mut connection,
            &[moved(
                "browser-1",
                "https://a.example/recovered",
                Some("Recovered"),
                NOW + 3,
            )],
            NOW,
        )
        .unwrap();
    assert_eq!(
        rows(&connection),
        [
            row("https://a.example/recovered", Some("Recovered")),
            row("https://b.example/", Some("From two")),
        ]
    );
}

#[test]
fn a_title_names_its_own_tabs_latest_visit_even_when_another_tab_shares_the_url() {
    let mut connection = store::open(":memory:").unwrap();
    let mut writer = Writer::default();
    writer
        .apply(
            &mut connection,
            &[
                visit("browser-1", "https://a.example/", NOW - 3),
                visit("browser-2", "https://a.example/", NOW - 2),
                title("browser-1", "From one"),
                title("browser-3", "Unknown tab"),
            ],
            NOW,
        )
        .unwrap();
    writer
        .apply(&mut connection, &[title("browser-2", "From two")], NOW)
        .unwrap();
    assert_eq!(
        rows(&connection),
        [
            row("https://a.example/", Some("From one")),
            row("https://a.example/", Some("From two")),
        ]
    );
}

#[test]
fn same_document_changes_soon_after_a_visit_fold_into_it_and_later_ones_add_rows() {
    let mut connection = store::open(":memory:").unwrap();
    let mut writer = Writer::default();
    writer
        .apply(
            &mut connection,
            &[
                visit("browser-1", "https://maps.example/", NOW),
                title("browser-1", "Maps"),
                moved(
                    "browser-1",
                    "https://maps.example/@1",
                    Some("Maps"),
                    NOW + 100,
                ),
                moved("browser-1", "https://maps.example/@2", None, NOW + 200),
                moved(
                    "browser-1",
                    "https://maps.example/place",
                    Some("Place"),
                    NOW + SAME_DOCUMENT_FOLD_MS,
                ),
                visit(
                    "browser-1",
                    "https://other.example/",
                    NOW + SAME_DOCUMENT_FOLD_MS + 1,
                ),
            ],
            NOW,
        )
        .unwrap();
    assert_eq!(
        rows(&connection),
        [
            row("https://maps.example/@2", Some("Maps")),
            row("https://maps.example/place", Some("Place")),
            row("https://other.example/", None),
        ]
    );
}

#[test]
fn a_fold_into_a_removed_visit_records_a_new_one() {
    let mut connection = store::open(":memory:").unwrap();
    let mut writer = Writer::default();
    writer
        .apply(
            &mut connection,
            &[visit("browser-1", "https://a.example/", NOW)],
            NOW,
        )
        .unwrap();
    store::remove_range(&connection, i64::MIN, i64::MAX).unwrap();
    writer
        .apply(
            &mut connection,
            &[moved(
                "browser-1",
                "https://a.example/#b",
                Some("A"),
                NOW + 1,
            )],
            NOW,
        )
        .unwrap();
    assert_eq!(rows(&connection), [row("https://a.example/#b", Some("A"))]);
}

#[test]
fn inadmissible_visits_are_skipped() {
    let mut connection = store::open(":memory:").unwrap();
    let mut writer = Writer::default();
    writer
        .apply(
            &mut connection,
            &[
                visit("browser-1", "", NOW),
                visit("browser-1", "https://a.example/", -1),
                visit("browser-1", "https://ok.example/", NOW),
            ],
            NOW,
        )
        .unwrap();
    assert_eq!(rows(&connection), [row("https://ok.example/", None)]);
}

#[test]
fn pruning_runs_on_the_first_write_and_then_every_few_hundred_visits() {
    let mut connection = store::open(":memory:").unwrap();
    let mut writer = Writer::default();
    let expired = NOW - store::RETENTION_MS - 1;
    {
        let transaction = connection.transaction().unwrap();
        store::insert(&transaction, "https://stale.example/", None, expired).unwrap();
        transaction.commit().unwrap();
    }
    writer
        .apply(
            &mut connection,
            &[visit("browser-1", "https://a.example/", NOW)],
            NOW,
        )
        .unwrap();
    assert_eq!(rows(&connection), [row("https://a.example/", None)]);
    {
        let transaction = connection.transaction().unwrap();
        store::insert(&transaction, "https://stale.example/", None, expired).unwrap();
        transaction.commit().unwrap();
    }
    let batch = (0..PRUNE_EVERY - 1)
        .map(|index| visit("browser-1", &format!("https://{index}.example/"), NOW))
        .collect::<Vec<_>>();
    writer.apply(&mut connection, &batch, NOW).unwrap();
    assert!(
        !store::page(&connection, "stale", None, 1)
            .unwrap()
            .visits
            .is_empty()
    );
    writer
        .apply(
            &mut connection,
            &[visit("browser-1", "https://due.example/", NOW)],
            NOW,
        )
        .unwrap();
    assert!(
        store::page(&connection, "stale", None, 1)
            .unwrap()
            .visits
            .is_empty()
    );
}

#[test]
fn a_removed_visits_id_is_never_reused_for_a_later_visit() {
    let mut connection = store::open(":memory:").unwrap();
    let mut writer = Writer::default();
    writer
        .apply(
            &mut connection,
            &[
                visit("browser-1", "https://a.example/", NOW),
                visit("browser-2", "https://b.example/", NOW + 1),
            ],
            NOW,
        )
        .unwrap();
    let newest = store::page(&connection, "", None, 1).unwrap().visits[0].id;
    assert_eq!(store::remove(&connection, &[newest]).unwrap(), 1);
    writer
        .apply(
            &mut connection,
            &[
                visit("browser-1", "https://c.example/", NOW + 2),
                title("browser-2", "Bravo"),
            ],
            NOW,
        )
        .unwrap();
    assert_eq!(
        rows(&connection),
        [
            row("https://a.example/", None),
            row("https://c.example/", None)
        ]
    );
}
