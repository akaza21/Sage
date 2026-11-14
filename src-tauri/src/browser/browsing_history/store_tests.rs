use super::*;

const DAY: i64 = 24 * 60 * 60 * 1000;
const NOW: i64 = 1_800_000_000_000;

fn seed(connection: &mut Connection, rows: &[(&str, Option<&str>, i64)]) {
    let transaction = connection.transaction().unwrap();
    for (url, title, visited_at) in rows {
        insert(&transaction, url, *title, *visited_at).unwrap();
    }
    transaction.commit().unwrap();
}

fn urls(page: &VisitPage) -> Vec<&str> {
    page.visits.iter().map(|visit| visit.url.as_str()).collect()
}

fn memory() -> Connection {
    open(":memory:").unwrap()
}

fn count(connection: &Connection) -> i64 {
    connection
        .query_row("SELECT count(*) FROM browser_visits", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn visits_read_newest_first_with_their_titles() {
    let mut connection = memory();
    seed(
        &mut connection,
        &[
            ("https://a.example/", None, NOW - 3),
            ("https://b.example/", Some("Bravo"), NOW - 2),
            ("https://a.example/", Some("Alpha"), NOW - 1),
        ],
    );
    let page = page(&connection, "", None, 10).unwrap();
    assert_eq!(
        urls(&page),
        [
            "https://a.example/",
            "https://b.example/",
            "https://a.example/"
        ]
    );
    assert_eq!(page.visits[0].title.as_deref(), Some("Alpha"));
    assert_eq!(page.visits[2].title, None);
    assert_eq!(page.visits[0].visited_at, NOW - 1);
    assert_eq!(page.next, None);
}

#[test]
fn titles_and_url_replacement_target_one_row_by_id() {
    let mut connection = memory();
    let transaction = connection.transaction().unwrap();
    let first = insert(&transaction, "https://a.example/", None, NOW - 2).unwrap();
    let second = insert(&transaction, "https://a.example/", None, NOW - 1).unwrap();
    set_title(&transaction, first, "First").unwrap();
    set_title(&transaction, second, "").unwrap();
    assert!(replace_url(&transaction, first, "https://a.example/#moved", None).unwrap());
    assert!(!replace_url(&transaction, 9_999, "https://x.example/", None).unwrap());
    transaction.commit().unwrap();
    let page = page(&connection, "", None, 10).unwrap();
    assert_eq!(
        page.visits
            .iter()
            .map(|visit| (visit.url.as_str(), visit.title.as_deref(), visit.visited_at))
            .collect::<Vec<_>>(),
        [
            ("https://a.example/", None, NOW - 1),
            ("https://a.example/#moved", Some("First"), NOW - 2),
        ]
    );
}

#[test]
fn pages_continue_strictly_after_the_cursor_even_within_one_millisecond() {
    let mut connection = memory();
    let rows = (0..5)
        .map(|index| format!("https://{index}.example/"))
        .collect::<Vec<_>>();
    seed(
        &mut connection,
        &rows
            .iter()
            .map(|url| (url.as_str(), None, NOW))
            .collect::<Vec<_>>(),
    );
    let first = page(&connection, "", None, 2).unwrap();
    assert_eq!(urls(&first), ["https://4.example/", "https://3.example/"]);
    let second = page(&connection, "", first.next, 2).unwrap();
    assert_eq!(urls(&second), ["https://2.example/", "https://1.example/"]);
    let last = page(&connection, "", second.next, 2).unwrap();
    assert_eq!(urls(&last), ["https://0.example/"]);
    assert_eq!(last.next, None);
}

#[test]
fn search_matches_url_or_title_case_insensitively_and_treats_wildcards_literally() {
    let mut connection = memory();
    seed(
        &mut connection,
        &[
            ("https://docs.example/guide", Some("Rust Guide"), NOW - 4),
            ("https://news.example/100%25", None, NOW - 3),
            ("https://other.example/a_b", None, NOW - 2),
            ("https://other.example/axb", None, NOW - 1),
        ],
    );
    assert_eq!(
        urls(&page(&connection, "rust", None, 10).unwrap()),
        ["https://docs.example/guide"]
    );
    assert_eq!(
        urls(&page(&connection, "DOCS", None, 10).unwrap()),
        ["https://docs.example/guide"]
    );
    assert_eq!(
        urls(&page(&connection, "a_b", None, 10).unwrap()),
        ["https://other.example/a_b"]
    );
    assert_eq!(
        urls(&page(&connection, "%", None, 10).unwrap()),
        ["https://news.example/100%25"]
    );
    assert!(
        page(&connection, "absent", None, 10)
            .unwrap()
            .visits
            .is_empty()
    );
}

#[test]
fn remove_deletes_only_the_named_visits() {
    let mut connection = memory();
    seed(
        &mut connection,
        &[
            ("https://a.example/", None, NOW - 2),
            ("https://a.example/", None, NOW - 1),
        ],
    );
    let before = page(&connection, "", None, 10).unwrap();
    assert_eq!(
        remove(&connection, &[before.visits[0].id, 9_999]).unwrap(),
        1
    );
    let after = page(&connection, "", None, 10).unwrap();
    assert_eq!(after.visits, before.visits[1..]);
    assert_eq!(remove(&connection, &[]).unwrap(), 0);
}

#[test]
fn a_time_range_removal_keeps_visits_outside_it() {
    let mut connection = memory();
    seed(
        &mut connection,
        &[
            ("https://old.example/", None, NOW - 3 * DAY),
            ("https://edge.example/", None, NOW - DAY),
            ("https://new.example/", None, NOW - 1),
        ],
    );
    assert_eq!(remove_range(&connection, NOW - DAY, NOW).unwrap(), 2);
    assert_eq!(
        urls(&page(&connection, "", None, 10).unwrap()),
        ["https://old.example/"]
    );
    assert_eq!(remove_range(&connection, i64::MIN, i64::MAX).unwrap(), 1);
    assert_eq!(count(&connection), 0);
}

#[test]
fn prune_removes_visits_past_the_age_and_count_bounds() {
    let mut connection = memory();
    let kept = (0..MAX_VISITS)
        .map(|index| format!("https://{index}.example/"))
        .collect::<Vec<_>>();
    let mut rows = vec![
        ("https://expired.example/", None, NOW - RETENTION_MS - 1),
        ("https://oldest.example/", None, NOW - RETENTION_MS),
    ];
    rows.extend(
        kept.iter()
            .enumerate()
            .map(|(index, url)| (url.as_str(), None, NOW + index as i64)),
    );
    seed(&mut connection, &rows);
    let transaction = connection.transaction().unwrap();
    prune(&transaction, NOW).unwrap();
    transaction.commit().unwrap();
    assert_eq!(count(&connection), MAX_VISITS);
    assert!(
        page(&connection, "oldest", None, 10)
            .unwrap()
            .visits
            .is_empty()
    );
    assert_eq!(
        urls(&page(&connection, "", None, 1).unwrap()),
        [format!("https://{}.example/", MAX_VISITS - 1)]
    );
}

#[test]
fn values_are_bounded_and_the_schema_refuses_the_rest() {
    let mut connection = memory();
    assert!(!admissible("", NOW));
    assert!(!admissible(&"a".repeat(MAX_URL_BYTES + 1), NOW));
    assert!(!admissible("https://a.example/", -1));
    assert!(admissible("https://a.example/", NOW));
    let long_title = "t".repeat(MAX_TITLE_CHARS + 50);
    seed(
        &mut connection,
        &[("https://ok.example/", Some(long_title.as_str()), NOW)],
    );
    let page = page(&connection, "", None, 10).unwrap();
    assert_eq!(
        page.visits[0]
            .title
            .as_ref()
            .map(|title| title.chars().count()),
        Some(MAX_TITLE_CHARS)
    );
    for statement in [
        "INSERT INTO browser_visits(url, visited_at) VALUES ('', 1)",
        "INSERT INTO browser_visits(url, title, visited_at) VALUES ('https://x/', '', 1)",
        "INSERT INTO browser_visits(url, visited_at) VALUES ('https://x/', -1)",
    ] {
        assert!(connection.execute(statement, []).is_err(), "{statement}");
    }
}

#[test]
fn the_history_file_persists_across_reopening() {
    let directory =
        std::env::temp_dir().join(format!("sage-browsing-history-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("browsing-history.db");
    let path = path.to_str().unwrap();
    seed(
        &mut open(path).unwrap(),
        &[("https://kept.example/", None, NOW)],
    );
    assert_eq!(
        urls(&page(&open(path).unwrap(), "", None, 10).unwrap()),
        ["https://kept.example/"]
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_file_from_before_autoincrement_keeps_its_visits_and_never_reuses_an_id() {
    let directory =
        std::env::temp_dir().join(format!("sage-browsing-history-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("browsing-history.db");
    let path = path.to_str().unwrap();
    Connection::open(path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE browser_visits (
               id INTEGER PRIMARY KEY,
               url TEXT NOT NULL CHECK (length(url) BETWEEN 1 AND 2048),
               title TEXT CHECK (title IS NULL OR length(title) BETWEEN 1 AND 256),
               visited_at INTEGER NOT NULL CHECK (visited_at >= 0)
             ) STRICT;
             CREATE INDEX browser_visits_by_time ON browser_visits(visited_at);
             INSERT INTO browser_visits (id, url, title, visited_at) VALUES
               (1, 'https://a.example/', 'A', 1), (2, 'https://b.example/', NULL, 2);",
        )
        .unwrap();
    let mut connection = open(path).unwrap();
    assert_eq!(
        urls(&page(&connection, "", None, 10).unwrap()),
        ["https://b.example/", "https://a.example/"]
    );
    assert_eq!(remove(&connection, &[2]).unwrap(), 1);
    let transaction = connection.transaction().unwrap();
    assert_eq!(
        insert(&transaction, "https://c.example/", None, 3).unwrap(),
        3
    );
    transaction.commit().unwrap();
    drop(connection);
    let reopened = open(path).unwrap();
    assert_eq!(count(&reopened), 2);
    drop(reopened);
    std::fs::remove_dir_all(directory).unwrap();
}
