//! The invoke guard in `ipc_commands.rs` rejects browser tabs, so no page can
//! read or change its own history.

use serde::Deserialize;
use tauri::State;

use super::{BrowsingHistory, Cursor, MAX_PAGE, MAX_REMOVE, MAX_SEARCH_CHARS, VisitPage};
use crate::browser::BrowserState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HistoryQuery {
    #[serde(default)]
    search: String,
    before: Option<Cursor>,
    limit: u32,
}

fn admit_query(query: HistoryQuery) -> Result<HistoryQuery, String> {
    if query.search.chars().count() > MAX_SEARCH_CHARS {
        return Err("history search is too long".into());
    }
    if !(1..=MAX_PAGE).contains(&query.limit) {
        return Err("history page size is out of range".into());
    }
    Ok(HistoryQuery {
        search: query.search.trim().to_owned(),
        ..query
    })
}

fn admit_ids(ids: Vec<i64>) -> Result<Vec<i64>, String> {
    if ids.is_empty() || ids.len() > MAX_REMOVE {
        return Err("history removal names too many or no visits".into());
    }
    Ok(ids)
}

async fn off_ui<T: Send + 'static>(
    history: BrowsingHistory,
    work: impl FnOnce(&BrowsingHistory) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || work(&history))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn browser_history_page(
    state: State<'_, BrowserState>,
    query: HistoryQuery,
) -> Result<VisitPage, String> {
    let query = admit_query(query)?;
    off_ui(state.browsing_history.clone(), move |history| {
        history.page(query.search, query.before, query.limit)
    })
    .await
}

#[tauri::command]
pub(crate) async fn browser_history_remove(
    state: State<'_, BrowserState>,
    ids: Vec<i64>,
) -> Result<usize, String> {
    let ids = admit_ids(ids)?;
    off_ui(state.browsing_history.clone(), move |history| {
        history.remove(ids)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_are_bounded_and_closed() {
        let query = |value| serde_json::from_value::<HistoryQuery>(value);
        assert!(query(serde_json::json!({ "limit": 50, "url": "https://example.com/" })).is_err());
        assert!(query(serde_json::json!({ "search": "a" })).is_err());
        assert!(
            query(serde_json::json!({
                "limit": 1,
                "before": { "visitedAt": 1, "id": 1, "extra": 0 }
            }))
            .is_err()
        );
        for limit in [0, MAX_PAGE + 1] {
            assert!(admit_query(query(serde_json::json!({ "limit": limit })).unwrap()).is_err());
        }
        let long = "x".repeat(MAX_SEARCH_CHARS + 1);
        assert!(
            admit_query(query(serde_json::json!({ "search": long, "limit": 1 })).unwrap()).is_err()
        );
        let admitted = admit_query(
            query(serde_json::json!({
                "search": "  docs ",
                "before": { "visitedAt": 5, "id": 2 },
                "limit": MAX_PAGE
            }))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(admitted.search, "docs");
        assert_eq!(
            admitted.before,
            Some(Cursor {
                visited_at: 5,
                id: 2
            })
        );
    }

    #[test]
    fn removals_name_between_one_and_the_maximum_visits() {
        assert!(admit_ids(Vec::new()).is_err());
        assert!(admit_ids((0..=MAX_REMOVE as i64).collect()).is_err());
        assert_eq!(admit_ids(vec![3, 4]).unwrap(), vec![3, 4]);
    }
}
