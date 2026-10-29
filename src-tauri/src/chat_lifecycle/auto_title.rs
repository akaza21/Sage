use rusqlite::{OptionalExtension, Transaction, params};

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
    let previous: Option<String> = transaction
        .query_row(
            "SELECT title FROM chats WHERE project_id=?1 AND id=?2
         AND NOT EXISTS (SELECT 1 FROM turns WHERE project_id=?1 AND chat_id=?2 LIMIT 1)",
            params![project_id, chat_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(previous) = previous else {
        return Ok(());
    };
    let default_title = matches!(previous.as_str(), "Chat: Chat" | "Chat: New project")
        || previous
            .strip_prefix("Chat: New chat ")
            .is_some_and(|suffix| {
                !suffix.is_empty() && suffix.bytes().all(|character| character.is_ascii_digit())
            });
    if !default_title {
        return Ok(());
    }
    let Some(title) = first_line_title(prompt) else {
        return Ok(());
    };
    transaction.execute(
        "UPDATE chats SET title=?3,updated_at=?4 WHERE project_id=?1 AND id=?2",
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
        connection.execute_batch("CREATE TABLE chats(id TEXT,project_id TEXT,title TEXT,updated_at INTEGER);
            CREATE TABLE turns(project_id TEXT,chat_id TEXT);
            CREATE TABLE workbench_tabs(id TEXT,project_id TEXT,kind TEXT,title TEXT,updated_at INTEGER);
            INSERT INTO chats VALUES('chat','project','Chat: Chat',0);
            INSERT INTO workbench_tabs VALUES('chat','project','chat','Chat: Chat',0);").unwrap();
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
        let transaction = connection.transaction().unwrap();
        auto_title_first_prompt_in(&transaction, "project", "chat", "Second prompt", 3).unwrap();
        transaction.commit().unwrap();
        assert_eq!(read(&connection), "Chat: Chat");
    }
}
