use rusqlite::{OptionalExtension, Transaction, params};

/// Who set a chat's current title. Stored in `chats.title_source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatTitleSource {
    /// Sage's placeholder for a new chat.
    Default,
    /// Derived from the first prompt.
    Prompt,
    /// Reported by the harness for the chat's agent session.
    Harness,
    /// Typed by the user.
    User,
    /// Derived from the parent chat when the chat was forked.
    Fork,
}

impl ChatTitleSource {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Prompt => "prompt",
            Self::Harness => "harness",
            Self::User => "user",
            Self::Fork => "fork",
        }
    }
}

/// Shows a harness-reported title as the chat title unless the user named
/// the chat or it is a fork. A title that is empty once sanitized is ignored,
/// so a harness can never blank a chat.
pub(crate) fn apply_harness_title_in(
    transaction: &Transaction<'_>,
    project_id: &str,
    chat_id: &str,
    harness_title: &str,
    now: i64,
) -> rusqlite::Result<()> {
    let Some(title) = super::auto_title::first_line_title(harness_title) else {
        return Ok(());
    };
    let changed = transaction.execute(
        "UPDATE chats SET title=?3,updated_at=?4,title_source=?5
         WHERE project_id=?1 AND id=?2 AND title_source IN (?6,?7,?5)
           AND NOT (title=?3 AND title_source=?5)",
        params![
            project_id,
            chat_id,
            title,
            now,
            ChatTitleSource::Harness.as_str(),
            ChatTitleSource::Default.as_str(),
            ChatTitleSource::Prompt.as_str(),
        ],
    )?;
    if changed == 0 {
        return Ok(());
    }
    retitle_attached_tab_in(transaction, project_id, chat_id, &title, now)
}

/// Id prefix of a temporary side chat.
pub(crate) const SIDE_CHAT_ID_PREFIX: &str = "side-";

/// The title Sage gives a new chat before anything names it.
pub(crate) fn placeholder_chat_title(chat_id: &str) -> &'static str {
    if chat_id.starts_with(SIDE_CHAT_ID_PREFIX) {
        "Chat: Side chat"
    } else {
        "Chat: Chat"
    }
}

/// Puts back the title Sage would have given the chat once its harness
/// clears the title it reported: the first prompt's title, or the
/// placeholder when there is no usable first prompt. Only a harness-titled
/// chat changes; user and fork titles are never touched.
pub(crate) fn clear_harness_title_in(
    transaction: &Transaction<'_>,
    project_id: &str,
    chat_id: &str,
    now: i64,
) -> rusqlite::Result<()> {
    let (title, source) = match first_prompt_title(transaction, project_id, chat_id)? {
        Some(title) => (title, ChatTitleSource::Prompt),
        None => (
            placeholder_chat_title(chat_id).to_owned(),
            ChatTitleSource::Default,
        ),
    };
    let changed = transaction.execute(
        "UPDATE chats SET title=?3,updated_at=?4,title_source=?5
         WHERE project_id=?1 AND id=?2 AND title_source=?6",
        params![
            project_id,
            chat_id,
            title,
            now,
            source.as_str(),
            ChatTitleSource::Harness.as_str(),
        ],
    )?;
    if changed == 0 {
        return Ok(());
    }
    retitle_attached_tab_in(transaction, project_id, chat_id, &title, now)
}

/// The title the chat's first prompt gives it: the primary user item of its
/// earliest visible turn, by the same rule as auto-titling.
fn first_prompt_title(
    transaction: &Transaction<'_>,
    project_id: &str,
    chat_id: &str,
) -> rusqlite::Result<Option<String>> {
    let payload: Option<String> = transaction
        .query_row(
            concat!(
                "SELECT i.payload_json FROM items i
                 WHERE i.project_id=?1 AND i.chat_id=?2 AND i.ordinal=0 AND i.origin='primary'
                   AND i.turn_id=(SELECT t.id FROM turns t WHERE t.project_id=?1 AND t.chat_id=?2 AND ",
                crate::acp::store::turn_undo::visible_turn!("t.session_id", "t.id"),
                " ORDER BY t.created_at,t.ordinal LIMIT 1)"
            ),
            params![project_id, chat_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(payload) = payload else {
        return Ok(None);
    };
    let Ok(item) = serde_json::from_str::<serde_json::Value>(&payload) else {
        return Ok(None);
    };
    if item["kind"] != "text" {
        return Ok(None);
    }
    Ok(item["payload"]["text"]
        .as_str()
        .and_then(super::auto_title::first_line_title))
}

/// Mirrors a chat's new title onto the workbench tab attached to it.
pub(super) fn retitle_attached_tab_in(
    transaction: &Transaction<'_>,
    project_id: &str,
    chat_id: &str,
    title: &str,
    now: i64,
) -> rusqlite::Result<()> {
    transaction.execute(
        "UPDATE workbench_tabs SET title=?3,updated_at=?4
         WHERE project_id=?1 AND kind='chat'
           AND id=(SELECT tab_id FROM chat_tabs WHERE project_id=?1 AND chat_id=?2)",
        params![project_id, chat_id, title, now],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat_lifecycle::store::ChatLifecycleStore;
    use crate::project_registry::create::ProjectCreateStore;
    use crate::workbench_persistence::store::WorkbenchStore;
    use sage_db::database::Database;

    struct Fixture {
        root: std::path::PathBuf,
        database: Database,
        project: String,
        chat: String,
    }

    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("sage-harness-title-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let database =
                crate::database::open_database(std::path::Path::new(":memory:")).unwrap();
            let project = database.create_project_from_root(&root).unwrap().id;
            let chat = database
                .bootstrap_snapshot()
                .unwrap()
                .active_tab_id
                .unwrap();
            Self {
                root,
                database,
                project,
                chat,
            }
        }

        fn harness(&self, title: &str) {
            let mut connection = self.database.write_connection().unwrap();
            let transaction = connection.transaction().unwrap();
            apply_harness_title_in(&transaction, &self.project, &self.chat, title, 7).unwrap();
            transaction.commit().unwrap();
        }

        fn clear(&self) {
            let mut connection = self.database.write_connection().unwrap();
            let transaction = connection.transaction().unwrap();
            clear_harness_title_in(&transaction, &self.project, &self.chat, 8).unwrap();
            transaction.commit().unwrap();
        }

        /// Switches the fixture to a new side chat of the current chat.
        fn side_chat(mut self) -> Self {
            let mut connection = self.database.write_connection().unwrap();
            let transaction = connection.transaction().unwrap();
            let side =
                super::super::insert_new_chat(&transaction, &self.project, true, Some(&self.chat))
                    .unwrap();
            transaction.commit().unwrap();
            drop(connection);
            self.chat = side;
            self
        }

        fn prompt(&self, text: &str) {
            let mut connection = self.database.write_connection().unwrap();
            let transaction = connection.transaction().unwrap();
            super::super::auto_title_first_prompt_in(
                &transaction,
                &self.project,
                &self.chat,
                text,
                5,
            )
            .unwrap();
            transaction.commit().unwrap();
        }

        /// The chat row's title and source, and the attached tab's title.
        fn shown(&self) -> (String, String, String) {
            let connection = self.database.write_connection().unwrap();
            let (title, source) = connection
                .query_row(
                    "SELECT title,title_source FROM chats WHERE id=?1",
                    [&self.chat],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            let tab = connection
                .query_row(
                    "SELECT title FROM workbench_tabs WHERE id=(SELECT tab_id FROM chat_tabs WHERE chat_id=?1)",
                    [&self.chat],
                    |row| row.get(0),
                )
                .unwrap();
            (title, source, tab)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn shown(title: &str, source: &str) -> (String, String, String) {
        (title.into(), source.into(), title.into())
    }

    #[test]
    fn harness_title_replaces_the_placeholder_and_the_prompt_title() {
        let fixture = Fixture::new();
        assert_eq!(fixture.shown(), shown("Chat: Chat", "default"));
        fixture.prompt("fix the flaky upload test\nwith details");
        assert_eq!(
            fixture.shown(),
            shown("Chat: fix the flaky upload test", "prompt")
        );
        fixture.harness("Fix flaky upload test");
        assert_eq!(
            fixture.shown(),
            shown("Chat: Fix flaky upload test", "harness")
        );
        fixture.harness("Stabilise the upload test");
        assert_eq!(
            fixture.shown(),
            shown("Chat: Stabilise the upload test", "harness")
        );
    }

    #[test]
    fn a_harness_title_before_the_first_prompt_is_kept() {
        let fixture = Fixture::new();
        fixture.harness("Resumed session");
        fixture.prompt("a new first prompt");
        assert_eq!(fixture.shown(), shown("Chat: Resumed session", "harness"));
    }

    #[test]
    fn a_chat_the_user_renamed_keeps_its_title() {
        let fixture = Fixture::new();
        fixture.harness("Harness name");
        let snapshot = fixture.database.bootstrap_snapshot().unwrap();
        fixture
            .database
            .rename_chat(&sage_domain::chat::RenameChatRequest {
                project_id: fixture.project.clone(),
                chat_id: fixture.chat.clone(),
                title: "Release notes".into(),
                current_layout: sage_domain::chat::ProjectLayoutRequest {
                    project_id: fixture.project.clone(),
                    tabs: snapshot.tabs,
                    active_tab_id: snapshot.active_tab_id,
                    selected_root_id: snapshot.selected_root_id,
                    selected_resource: snapshot.selected_resource,
                },
            })
            .unwrap();
        assert_eq!(fixture.shown(), shown("Chat: Release notes", "user"));
        fixture.harness("A later harness name");
        assert_eq!(fixture.shown(), shown("Chat: Release notes", "user"));
    }

    #[test]
    fn harness_titles_are_sanitized_and_bounded() {
        let fixture = Fixture::new();
        fixture.harness(" \u{feff}\n second line");
        assert_eq!(fixture.shown(), shown("Chat: Chat", "default"));
        fixture.harness("\u{7} Ship\u{202e}it\u{200b}\tnow\nignored");
        assert_eq!(fixture.shown(), shown("Chat: Shipit now", "harness"));
        fixture.harness(&"é".repeat(200));
        let (title, _, _) = fixture.shown();
        assert_eq!(
            title.strip_prefix("Chat: ").unwrap().len(),
            super::super::MAX_CHAT_TITLE_BYTES
        );
    }

    #[test]
    fn a_cleared_harness_title_without_turns_restores_the_placeholder() {
        let fixture = Fixture::new();
        fixture.harness("Resumed session");
        fixture.clear();
        assert_eq!(fixture.shown(), shown("Chat: Chat", "default"));
        fixture.harness("Named again");
        assert_eq!(fixture.shown(), shown("Chat: Named again", "harness"));
    }

    #[test]
    fn a_cleared_side_chat_restores_the_side_chat_placeholder() {
        let fixture = Fixture::new().side_chat();
        assert!(fixture.chat.starts_with(SIDE_CHAT_ID_PREFIX));
        assert_eq!(fixture.shown(), shown("Chat: Side chat", "default"));
        fixture.harness("Side question");
        fixture.clear();
        assert_eq!(fixture.shown(), shown("Chat: Side chat", "default"));
    }

    #[test]
    fn a_clear_only_touches_a_harness_title() {
        let fixture = Fixture::new();
        fixture.prompt("fix the flaky upload test");
        fixture.clear();
        assert_eq!(
            fixture.shown(),
            shown("Chat: fix the flaky upload test", "prompt")
        );
    }
}
