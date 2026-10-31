use rusqlite::Connection;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tauri::Emitter;

pub(crate) const EVENT: &str = "chat-attention-changed-v1";

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum Notice {
    Changed {
        #[serde(rename = "projectId")]
        project_id: String,
        #[serde(rename = "chatId")]
        chat_id: String,
        revision: String,
    },
    Resync {
        #[serde(rename = "resyncRequired")]
        resync_required: bool,
    },
}

#[derive(Debug, Default)]
struct Pending {
    dirty: BTreeMap<String, Notice>,
    resync: bool,
    scheduled: bool,
    app: Option<tauri::AppHandle>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AttentionPublisher(Arc<Mutex<Pending>>);

impl AttentionPublisher {
    pub(crate) fn attach(&self, app: tauri::AppHandle) {
        self.lock().app = Some(app);
        self.resync();
    }

    pub(crate) fn collect_committed(&self, connection: &Connection) {
        if !connection.is_autocommit() {
            self.resync();
            return;
        }
        match drain(connection) {
            Ok((resync, notices)) => {
                let mut state = self.lock();
                state.resync |= resync;
                for notice in notices {
                    if let Notice::Changed { chat_id, .. } = &notice {
                        if state.dirty.len() >= 64 && !state.dirty.contains_key(chat_id) {
                            state.resync = true;
                        }
                        if !state.resync {
                            state.dirty.insert(chat_id.clone(), notice);
                        }
                    }
                }
                if state.resync {
                    state.dirty.clear();
                }
            }
            Err(error) => {
                tracing::warn!(?error, "attention publication requires resync");
                self.lock().resync = true;
            }
        }
        self.schedule();
    }

    fn resync(&self) {
        self.lock().resync = true;
        self.schedule();
    }

    fn schedule(&self) {
        let mut state = self.lock();
        if state.scheduled || state.app.is_none() || (!state.resync && state.dirty.is_empty()) {
            return;
        }
        state.scheduled = true;
        drop(state);
        let publisher = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let (app, notices) = {
                let mut state = publisher.lock();
                let notices = if state.resync {
                    state.resync = false;
                    state.dirty.clear();
                    vec![Notice::Resync {
                        resync_required: true,
                    }]
                } else {
                    std::mem::take(&mut state.dirty).into_values().collect()
                };
                state.scheduled = false;
                (state.app.clone(), notices)
            };
            if let Some(app) = app {
                for notice in notices {
                    if app.emit(EVENT, &notice).is_err() {
                        publisher.resync();
                        break;
                    }
                }
            }
        });
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Pending> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

pub(crate) fn install(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch("CREATE TEMP TABLE attention_dirty(chat_id TEXT PRIMARY KEY,project_id TEXT NOT NULL,revision INTEGER NOT NULL) STRICT;
        CREATE TEMP TABLE attention_overflow(singleton INTEGER PRIMARY KEY CHECK(singleton=1),resync INTEGER NOT NULL) STRICT;
        INSERT INTO attention_overflow VALUES(1,0);")?;
    for (name, event) in [
        ("attention_insert", "INSERT"),
        ("attention_update", "UPDATE"),
    ] {
        connection.execute_batch(&format!("CREATE TEMP TRIGGER {name} AFTER {event} ON main.chat_attention BEGIN
          UPDATE attention_overflow SET resync=1 WHERE resync=0 AND (SELECT COUNT(*) FROM attention_dirty)>=64
            AND NOT EXISTS(SELECT 1 FROM attention_dirty WHERE chat_id=NEW.chat_id);
          DELETE FROM attention_dirty WHERE (SELECT resync FROM attention_overflow)=1;
          INSERT INTO attention_dirty(chat_id,project_id,revision)
            SELECT NEW.chat_id,c.project_id,NEW.revision FROM main.chats c WHERE c.id=NEW.chat_id
              AND (SELECT resync FROM attention_overflow)=0
            ON CONFLICT(chat_id) DO UPDATE SET project_id=excluded.project_id,revision=excluded.revision;
        END;"))?;
    }
    Ok(())
}

fn drain(connection: &Connection) -> rusqlite::Result<(bool, Vec<Notice>)> {
    let resync = connection.query_row("SELECT resync FROM attention_overflow", [], |row| {
        row.get(0)
    })?;
    let notices = connection
        .prepare("SELECT project_id,chat_id,revision FROM attention_dirty LIMIT 64")?
        .query_map([], |row| {
            Ok(Notice::Changed {
                project_id: row.get(0)?,
                chat_id: row.get(1)?,
                revision: row.get::<_, i64>(2)?.to_string(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    connection
        .execute_batch("DELETE FROM attention_dirty; UPDATE attention_overflow SET resync=0;")?;
    Ok((resync, notices))
}

#[cfg(test)]
pub(crate) fn assert_overflow_resync_for_test() {
    let mut connection = Connection::open_in_memory().unwrap();
    connection.execute_batch("CREATE TABLE chats(id TEXT PRIMARY KEY, project_id TEXT); CREATE TABLE chat_attention(chat_id TEXT,revision INTEGER);").unwrap();
    install(&connection).unwrap();
    {
        let transaction = connection.transaction().unwrap();
        transaction
            .execute("INSERT INTO chats VALUES('rolled-back','project')", [])
            .unwrap();
        transaction
            .execute("INSERT INTO chat_attention VALUES('rolled-back',1)", [])
            .unwrap();
    }
    let (resync, notices) = drain(&connection).unwrap();
    assert!(!resync);
    assert!(notices.is_empty());
    {
        let transaction = connection.transaction().unwrap();
        transaction
            .execute("INSERT INTO chats VALUES('committed','project')", [])
            .unwrap();
        transaction
            .execute("INSERT INTO chat_attention VALUES('committed',2)", [])
            .unwrap();
        transaction.commit().unwrap();
    }
    let (resync, notices) = drain(&connection).unwrap();
    assert!(!resync);
    assert_eq!(notices.len(), 1);
    assert!(
        matches!(&notices[0], Notice::Changed { chat_id, revision, .. } if chat_id == "committed" && revision == "2")
    );
    for index in 0..65 {
        connection
            .execute(
                "INSERT INTO chats VALUES(?1,'project')",
                [index.to_string()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO chat_attention VALUES(?1,1)",
                [index.to_string()],
            )
            .unwrap();
    }
    let (resync, notices) = drain(&connection).unwrap();
    assert!(resync);
    assert!(notices.is_empty());
    assert!(!drain(&connection).unwrap().0);
}
