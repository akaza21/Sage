//! The models each harness listed last, so a new chat shows them at once on
//! the next start while the harness is asked again. A listing here is never
//! sent with: a session starts only on a model the live listing confirms.

use rusqlite::{Connection, OptionalExtension, params};
use sage_core::error::AppResult;
use sage_db::error::SqliteResultExt;
use sage_domain::harness::ModelOption;

use super::SettingsStore;

/// One row per harness; the oldest listings beyond this are dropped.
pub const MAX_REMEMBERED_LISTINGS: i64 = 16;
const MAX_LISTING_BYTES: usize = 64 * 1024;
const MAX_HARNESS_ID_BYTES: usize = 256;

pub(super) fn migrate(connection: &Connection) -> AppResult<()> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS harness_model_listings (
           harness_id TEXT PRIMARY KEY
             CHECK (length(CAST(harness_id AS BLOB)) BETWEEN 1 AND 256),
           models_json TEXT NOT NULL
             CHECK (json_valid(models_json) AND length(CAST(models_json AS BLOB)) <= 65536),
           saved_at INTEGER NOT NULL
         ) STRICT;",
        )
        .db()?;
    Ok(())
}

impl SettingsStore {
    /// The listing remembered for `harness_id`; empty when none is, or when
    /// it no longer reads.
    pub fn remembered_harness_models(&self, harness_id: &str) -> AppResult<Vec<ModelOption>> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let stored = connection
            .query_row(
                "SELECT models_json FROM harness_model_listings WHERE harness_id = ?1",
                [harness_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .db()?;
        Ok(stored
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default())
    }

    /// Keeps a live, non-empty listing; one too large to keep is skipped.
    pub fn remember_harness_models(
        &self,
        harness_id: &str,
        models: &[ModelOption],
        now_millis: i64,
    ) -> AppResult<()> {
        let Ok(json) = serde_json::to_string(models) else {
            return Ok(());
        };
        if models.is_empty()
            || harness_id.is_empty()
            || harness_id.len() > MAX_HARNESS_ID_BYTES
            || json.len() > MAX_LISTING_BYTES
        {
            return Ok(());
        }
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        connection
            .execute(
                "INSERT INTO harness_model_listings(harness_id, models_json, saved_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(harness_id) DO UPDATE SET
                   models_json = excluded.models_json, saved_at = excluded.saved_at",
                params![harness_id, json, now_millis],
            )
            .db()?;
        connection
            .execute(
                "DELETE FROM harness_model_listings WHERE harness_id NOT IN (
                   SELECT harness_id FROM harness_model_listings
                   ORDER BY saved_at DESC, harness_id LIMIT ?1)",
                [MAX_REMEMBERED_LISTINGS],
            )
            .db()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_REMEMBERED_LISTINGS, SettingsStore};
    use sage_domain::harness::ModelOption;

    fn model(id: &str) -> ModelOption {
        ModelOption {
            id: id.into(),
            name: id.into(),
            description: None,
            is_default: Some(true),
            context_window: None,
            effort_levels: Some(vec!["high".into()]),
        }
    }

    #[test]
    fn a_listing_survives_reopening_and_is_replaced_by_the_next() {
        let directory =
            std::env::temp_dir().join(format!("sage-model-listings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.db");
        let store = SettingsStore::open(&path).unwrap();
        assert!(store.remembered_harness_models("pi").unwrap().is_empty());
        store
            .remember_harness_models("pi", &[model("old")], 1)
            .unwrap();
        store
            .remember_harness_models("pi", &[model("sonnet"), model("opus")], 2)
            .unwrap();
        drop(store);
        let reopened = SettingsStore::open(&path).unwrap();
        let ids = reopened
            .remembered_harness_models("pi")
            .unwrap()
            .into_iter()
            .map(|model| model.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, ["sonnet", "opus"]);
        drop(reopened);
        std::fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn an_empty_listing_never_replaces_a_remembered_one() {
        let store = SettingsStore::in_memory().unwrap();
        store
            .remember_harness_models("codex", &[model("a")], 1)
            .unwrap();
        store.remember_harness_models("codex", &[], 2).unwrap();
        assert_eq!(store.remembered_harness_models("codex").unwrap().len(), 1);
    }

    #[test]
    fn only_the_most_recent_listings_are_kept() {
        let store = SettingsStore::in_memory().unwrap();
        for index in 0..=MAX_REMEMBERED_LISTINGS {
            store
                .remember_harness_models(&format!("custom-{index}"), &[model("m")], index)
                .unwrap();
        }
        assert!(
            store
                .remembered_harness_models("custom-0")
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            store
                .remembered_harness_models(&format!("custom-{MAX_REMEMBERED_LISTINGS}"))
                .unwrap()
                .len(),
            1
        );
    }
}
