use super::*;
use crate::browser::browsing_history::Write;

const DAY: i64 = 24 * 60 * 60 * 1000;
const NOW: i64 = 1_800_000_000_000;

fn visit(url: &str, visited_at: i64) -> Write {
    Write::Visit(NewVisit {
        url: url.into(),
        visited_at,
    })
}

fn title(url: &str, title: &str) -> Write {
    Write::Title {
        url: url.into(),
        title: title.into(),
    }
}

fn urls(page: &VisitPage) -> Vec<&str> {
    page.visits.iter().map(|visit| visit.url.as_str()).collect()
}

fn memory() -> Connection {
    open(":memory:").unwrap()
}

#[test]
fn visits_read_newest_first_and_a_title_lands_on_the_latest_visit_to_its_url() {
    let mut connection = memory();
    write(
        &mut connection,
        &[
            visit("https://a.example/", NOW - 3),
            visit("https://b.example/", NOW - 2),
            visit("https://a.example/", NOW - 1),
            title("https://a.example/", "Alpha"),
        ],
        NOW,
    )
    .unwrap();
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
fn pages_continue_strictly_after_the_cursor_even_within_one_millisecond() {
    let mut connection = memory();
    let writes = (0..5)
        .map(|index| visit(&format!("https://{index}.example/"), NOW))
        .collect::<Vec<_>>();
    write(&mut connection, &writes, NOW).unwrap();
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
    write(
        &mut connection,
        &[
            visit("https://docs.example/guide", NOW - 4),
            title("https://docs.example/guide", "Rust Guide"),
            visit("https://news.example/100%25", NOW - 3),
            visit("https://other.example/a_b", NOW - 2),
            visit("https://other.example/axb", NOW - 1),
        ],
        NOW,
    )
    .unwrap();
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
    write(
        &mut connection,
        &[
            visit("https://a.example/", NOW - 2),
            visit("https://a.example/", NOW - 1),
        ],
        NOW,
    )
    .unwrap();
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
    write(
        &mut connection,
        &[
            visit("https://old.example/", NOW - 3 * DAY),
            visit("https://edge.example/", NOW - DAY),
            visit("https://new.example/", NOW - 1),
        ],
        NOW,
    )
    .unwrap();
    assert_eq!(remove_range(&connection, NOW - DAY, NOW).unwrap(), 2);
    assert_eq!(
        urls(&page(&connection, "", None, 10).unwrap()),
        ["https://old.example/"]
    );
    assert_eq!(remove_range(&connection, i64::MIN, i64::MAX).unwrap(), 1);
    assert!(page(&connection, "", None, 10).unwrap().visits.is_empty());
}

#[test]
fn writes_prune_by_age_and_by_count() {
    let mut connection = memory();
    write(
        &mut connection,
        &[
            visit("https://expired.example/", NOW - RETENTION_MS - 1),
            visit("https://kept.example/", NOW - RETENTION_MS),
        ],
        NOW,
    )
    .unwrap();
    assert_eq!(
        urls(&page(&connection, "", None, 10).unwrap()),
        ["https://kept.example/"]
    );
    let writes = (0..MAX_VISITS)
        .map(|index| visit(&format!("https://{index}.example/"), NOW + index))
        .collect::<Vec<_>>();
    write(&mut connection, &writes, NOW).unwrap();
    let count: i64 = connection
        .query_row("SELECT count(*) FROM browser_visits", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, MAX_VISITS);
    let newest = page(&connection, "", None, 1).unwrap();
    assert_eq!(
        urls(&newest),
        [format!("https://{}.example/", MAX_VISITS - 1)]
    );
    assert!(
        page(&connection, "kept", None, 10)
            .unwrap()
            .visits
            .is_empty()
    );
}

#[test]
fn invalid_values_are_skipped_or_refused_by_the_schema() {
    let mut connection = memory();
    let long_title = "t".repeat(MAX_TITLE_CHARS + 50);
    write(
        &mut connection,
        &[
            visit("", NOW),
            visit(
                &format!("https://{}.example/", "a".repeat(MAX_URL_BYTES)),
                NOW,
            ),
            visit("https://a.example/", -1),
            visit("https://ok.example/", NOW),
            title("https://ok.example/", &long_title),
        ],
        NOW,
    )
    .unwrap();
    let page = page(&connection, "", None, 10).unwrap();
    assert_eq!(urls(&page), ["https://ok.example/"]);
    assert_eq!(
        page.visits[0]
            .title
            .as_ref()
            .map(|title| title.chars().count()),
        Some(MAX_TITLE_CHARS)
    );
    assert!(
        connection
            .execute(
                "INSERT INTO browser_visits(url, visited_at) VALUES ('', 1)",
                []
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO browser_visits(url, title, visited_at) VALUES ('https://x/', '', 1)",
                []
            )
            .is_err()
    );
}

#[test]
fn the_history_file_persists_across_reopening() {
    let directory =
        std::env::temp_dir().join(format!("sage-browsing-history-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("browsing-history.db");
    let path = path.to_str().unwrap();
    write(
        &mut open(path).unwrap(),
        &[visit("https://kept.example/", NOW)],
        NOW,
    )
    .unwrap();
    assert_eq!(
        urls(&page(&open(path).unwrap(), "", None, 10).unwrap()),
        ["https://kept.example/"]
    );
    std::fs::remove_dir_all(directory).unwrap();
}
