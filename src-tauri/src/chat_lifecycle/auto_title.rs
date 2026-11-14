use rusqlite::{Transaction, params};

use super::ChatTitleSource;

/// Format characters that change how surrounding text displays without
/// showing themselves: bidirectional controls and zero-width characters. A
/// title from a prompt or a harness must not be able to reorder or hide text
/// in the rail.
fn is_hidden_format_character(value: char) -> bool {
    matches!(
        value,
        '\u{061c}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}'
    )
}

/// Characters that end a line of text: the ones `str::lines` honours plus a
/// lone carriage return, vertical tab, form feed and the Unicode line and
/// paragraph separators, so no line break is ever read as part of a title.
fn is_line_break(value: char) -> bool {
    matches!(
        value,
        '\n' | '\r' | '\u{000b}' | '\u{000c}' | '\u{0085}' | '\u{2028}' | '\u{2029}'
    )
}

/// Whitespace controls become one space and other controls and hidden format
/// characters are dropped; `None` when nothing printable is left.
pub(crate) fn title_line(text: &str, max_bytes: usize) -> Option<String> {
    let mut visible = String::new();
    for value in text.split(is_line_break).next()?.chars() {
        if value.is_control() && value.is_whitespace() {
            if !visible.ends_with(' ') {
                visible.push(' ');
            }
        } else if !value.is_control() && !is_hidden_format_character(value) {
            visible.push(value);
        }
    }
    let mut title = String::new();
    for character in visible
        .trim_matches(super::is_chat_title_trim_character)
        .chars()
    {
        if title.len() + character.len_utf8() > max_bytes {
            break;
        }
        title.push(character);
    }
    let title = title.trim_end_matches(super::is_chat_title_trim_character);
    (!title.is_empty()).then(|| title.to_owned())
}

pub(super) fn first_line_title(text: &str) -> Option<String> {
    super::canonical_chat_title(&title_line(text, super::MAX_CHAT_TITLE_BYTES)?).ok()
}

pub(crate) fn auto_title_first_prompt_in(
    transaction: &Transaction<'_>,
    project_id: &str,
    chat_id: &str,
    prompt: &str,
    now: i64,
) -> rusqlite::Result<()> {
    let eligible: bool = transaction.query_row(
        concat!(
            "SELECT EXISTS(SELECT 1 FROM chats c
             WHERE c.project_id=?1 AND c.id=?2 AND c.title_source=?3
             AND NOT EXISTS(SELECT 1 FROM turns t WHERE t.project_id=?1 AND t.chat_id=?2 AND ",
            crate::acp::store::turn_undo::visible_turn!("t.session_id", "t.id"),
            " LIMIT 1))"
        ),
        params![project_id, chat_id, ChatTitleSource::Default.as_str()],
        |row| row.get(0),
    )?;
    if !eligible {
        return Ok(());
    }
    let Some(title) = first_line_title(prompt) else {
        return Ok(());
    };
    transaction.execute(
        "UPDATE chats SET title=?3,updated_at=?4,title_source=?5 WHERE project_id=?1 AND id=?2",
        params![
            project_id,
            chat_id,
            title,
            now,
            ChatTitleSource::Prompt.as_str()
        ],
    )?;
    super::title_source::retitle_attached_tab_in(transaction, project_id, chat_id, &title, now)
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
            Some("Chat: hello world".into())
        );
        assert_eq!(
            first_line_title("hello \t\tworld"),
            Some("Chat: hello world".into())
        );
        assert_eq!(
            first_line_title("line one\rHow many lines"),
            Some("Chat: line one".into())
        );
    }

    #[test]
    fn renames_only_an_untitled_first_prompt_and_rolls_back_with_the_turn() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE chats(id TEXT,project_id TEXT,title TEXT,updated_at INTEGER,title_source TEXT);
            CREATE TABLE turns(id TEXT,project_id TEXT,chat_id TEXT,session_id TEXT);
            CREATE TABLE session_facts(session_id TEXT,turn_id TEXT,kind TEXT);
            CREATE TABLE workbench_tabs(id TEXT,project_id TEXT,kind TEXT,title TEXT,updated_at INTEGER,metadata_json TEXT);
            CREATE TABLE chat_tabs(project_id TEXT,chat_id TEXT,tab_id TEXT);
            INSERT INTO chat_tabs VALUES('project','chat','chat');
            INSERT INTO chats VALUES('chat','project','Chat: Chat',0,'default');
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
            .execute(
                "UPDATE chats SET title='Chat: Custom',title_source='user'",
                [],
            )
            .unwrap();
        let transaction = connection.transaction().unwrap();
        auto_title_first_prompt_in(&transaction, "project", "chat", "Do not rename", 2).unwrap();
        transaction.commit().unwrap();
        assert_eq!(read(&connection), "Chat: Custom");
        connection
            .execute_batch(
                "UPDATE chats SET title='Chat: Chat';
                 INSERT INTO turns VALUES('turn','project','chat','session');",
            )
            .unwrap();
        connection
            .execute("UPDATE chats SET title_source='default'", [])
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
