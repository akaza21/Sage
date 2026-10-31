use rusqlite::{Transaction, params};

fn first_line_title(prompt: &str) -> Option<String> {
    let line = prompt
        .lines()
        .next()?
        .trim_matches(super::is_chat_title_trim_character);
    let mut title = String::new();
    for character in line.chars().filter(|character| !character.is_control()) {
        if title.len() + character.len_utf8() > super::MAX_CHAT_TITLE_BYTES {
            break;
        }
        title.push(character);
    }
    super::canonical_chat_title(&title).ok()
}

pub(crate) fn auto_title_first_prompt_in(
    transaction: &Transaction<'_>,
    project_id: &str,
    chat_id: &str,
    prompt: &str,
    now: i64,
) -> rusqlite::Result<()> {
    let eligible: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM chats c
         WHERE c.project_id=?1 AND c.id=?2 AND c.title_is_default=1
         AND NOT EXISTS(SELECT 1 FROM turns WHERE project_id=?1 AND chat_id=?2 LIMIT 1))",
        params![project_id, chat_id],
        |row| row.get(0),
    )?;
    if !eligible {
        return Ok(());
    }
    let Some(title) = first_line_title(prompt) else {
        return Ok(());
    };
    transaction.execute(
        "UPDATE chats SET title=?3,updated_at=?4,title_is_default=0 WHERE project_id=?1 AND id=?2",
        params![project_id, chat_id, title, now],
    )?;
    transaction.execute("UPDATE workbench_tabs SET title=?3,updated_at=?4 WHERE project_id=?1 AND id=?2 AND kind='chat'",
        params![project_id, chat_id, title, now])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_is_utf8_bounded_and_does_not_use_later_lines() {
        assert_eq!(
            first_line_title("  Fix the sidebar\nIgnore this  "),
            Some("Chat: Fix the sidebar".into())
        );
        let title = first_line_title(&"🙂".repeat(100)).unwrap();
        assert_eq!(title.strip_prefix("Chat: ").unwrap().len(), 128);
        assert_eq!(first_line_title("\nsecond line"), None);
        assert_eq!(first_line_title("\u{feff}"), None);
        assert_eq!(
            first_line_title("\u{feff}Fix sidebar\u{feff}"),
            Some("Chat: Fix sidebar".into())
        );
        assert_eq!(
            first_line_title("hello\tworld"),
            Some("Chat: helloworld".into())
        );
    }

    #[test]
    fn renames_only_an_untitled_first_prompt_and_rolls_back_with_the_turn() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE chats(id TEXT,project_id TEXT,title TEXT,updated_at INTEGER,title_is_default INTEGER);
            CREATE TABLE turns(project_id TEXT,chat_id TEXT);
            CREATE TABLE workbench_tabs(id TEXT,project_id TEXT,kind TEXT,title TEXT,updated_at INTEGER,metadata_json TEXT);
            INSERT INTO chats VALUES('chat','project','Chat: Chat',0,1);
            INSERT INTO workbench_tabs VALUES('chat','project','chat','Chat: Chat',0,'{}');").unwrap();
        {
            let transaction = connection.transaction().unwrap();
            auto_title_first_prompt_in(&transaction, "project", "chat", "Fix sidebar\nDetails", 1)
                .unwrap();
        }
        let read = |connection: &rusqlite::Connection| -> String {
            connection
                .query_row("SELECT title FROM chats", [], |row| row.get(0))
                .unwrap()
        };
        assert_eq!(read(&connection), "Chat: Chat");
        let transaction = connection.transaction().unwrap();
        auto_title_first_prompt_in(&transaction, "project", "chat", "Fix sidebar", 1).unwrap();
        transaction.commit().unwrap();
        assert_eq!(read(&connection), "Chat: Fix sidebar");
        let attached: String = connection
            .query_row("SELECT title FROM workbench_tabs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(attached, "Chat: Fix sidebar");
        connection
            .execute("UPDATE chats SET title='Chat: Custom'", [])
            .unwrap();
        let transaction = connection.transaction().unwrap();
        auto_title_first_prompt_in(&transaction, "project", "chat", "Do not rename", 2).unwrap();
        transaction.commit().unwrap();
        assert_eq!(read(&connection), "Chat: Custom");
        connection
            .execute_batch(
                "UPDATE chats SET title='Chat: Chat'; INSERT INTO turns VALUES('project','chat');",
            )
            .unwrap();
        connection
            .execute("UPDATE chats SET title_is_default=1", [])
            .unwrap();
        let transaction = connection.transaction().unwrap();
        auto_title_first_prompt_in(&transaction, "project", "chat", "Second prompt", 3).unwrap();
        transaction.commit().unwrap();
        assert_eq!(read(&connection), "Chat: Chat");
    }
    #[test]
    fn manually_named_default_text_is_not_guessed_to_be_untitled() {
        let root = std::env::temp_dir().join(format!("sage-manual-title-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let database =
            crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let chat = snapshot.active_tab_id.clone().unwrap();
        database
            .rename_chat(&crate::domain::RenameChatRequest {
                project_id: project.id.clone(),
                chat_id: chat.clone(),
                title: "Chat".into(),
                current_layout: crate::domain::ProjectLayoutRequest {
                    project_id: project.id.clone(),
                    tabs: snapshot.tabs,
                    active_tab_id: snapshot.active_tab_id,
                    selected_root_id: snapshot.selected_root_id,
                    selected_resource: snapshot.selected_resource,
                },
            })
            .unwrap();
        let mut connection = database.write_connection().unwrap();
        let transaction = connection.transaction().unwrap();
        auto_title_first_prompt_in(
            &transaction,
            &project.id,
            &chat,
            "Do not replace my title",
            1,
        )
        .unwrap();
        let title: String = transaction
            .query_row("SELECT title FROM chats WHERE id=?1", [&chat], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(title, "Chat: Chat");
        drop(transaction);
        drop(connection);
        drop(database);
        std::fs::remove_dir_all(root).unwrap();
    }
}
