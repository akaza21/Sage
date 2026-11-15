use super::*;
use sage_acp_protocol::{
    file_diff::{DiffRef, SnapshotData, SnapshotReadError},
    page_admission::PageReadLease,
};

impl AcpService {
    /// Never exposes a BLOB through the generic page payload.
    pub(crate) async fn read_file_snapshot(
        &self,
        project_id: &SageProjectId,
        expected_revision: sage_acp_protocol::contract::ProjectionRevision,
        diff_ref: DiffRef,
        lease: PageReadLease,
    ) -> Result<SnapshotData, SnapshotReadError> {
        if diff_ref.project_id != project_id.as_str() {
            return Err(SnapshotReadError::Forbidden);
        }
        diff_ref.validate()?;
        let database = self.database.clone();
        let project_id = project_id.clone();
        read_off_thread(lease, move || {
            read_snapshot(&database, &project_id, expected_revision, &diff_ref)
        })
        .await
    }
}

async fn read_off_thread(
    lease: PageReadLease,
    read: impl FnOnce() -> Result<SnapshotData, SnapshotReadError> + Send + 'static,
) -> Result<SnapshotData, SnapshotReadError> {
    tauri::async_runtime::spawn_blocking(move || {
        // The caller may disappear while a queued/running blocking task is
        // still alive. Only actual task settlement releases its shared slot.
        let _lease = lease;
        read()
    })
    .await
    .map_err(|_| SnapshotReadError::Corrupt)?
}

fn read_snapshot(
    database: &Database,
    project_id: &SageProjectId,
    expected_revision: sage_acp_protocol::contract::ProjectionRevision,
    diff_ref: &DiffRef,
) -> Result<SnapshotData, SnapshotReadError> {
    let connection = database.connection();
    verify_current_owner(&connection, expected_revision, diff_ref)?;
    read_owned(&connection, project_id, diff_ref)
}

pub(crate) fn verify_current_owner(
    connection: &rusqlite::Connection,
    expected_revision: sage_acp_protocol::contract::ProjectionRevision,
    diff_ref: &DiffRef,
) -> Result<(), SnapshotReadError> {
    // Head ownership and the projection selected by the caller are checked
    // under the same DB lock as the immutable row. Historical Turns in the
    // current session may retain older runtime epochs after a resume.
    let current: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM chats c JOIN chat_session_heads h
             ON h.project_id=c.project_id AND h.chat_id=c.id
             WHERE c.project_id=?1 AND c.id=?2 AND h.session_id=?3 AND c.projection_revision=?4)",
            rusqlite::params![
                diff_ref.project_id,
                diff_ref.chat_id,
                diff_ref.session_id,
                expected_revision.get() as i64
            ],
            |row| row.get(0),
        )
        .map_err(|_| SnapshotReadError::Corrupt)?;
    if !current {
        return Err(SnapshotReadError::Stale);
    }
    Ok(())
}

fn read_owned(
    connection: &rusqlite::Connection,
    project_id: &SageProjectId,
    diff_ref: &DiffRef,
) -> Result<SnapshotData, SnapshotReadError> {
    let mut statement = connection
        .prepare("SELECT id FROM project_roots WHERE project_id=?1 ORDER BY ordinal,id LIMIT 9")
        .map_err(|_| SnapshotReadError::Corrupt)?;
    let roots = statement
        .query_map([project_id.as_str()], |row| row.get::<_, String>(0))
        .map_err(|_| SnapshotReadError::Corrupt)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SnapshotReadError::Corrupt)?;
    if roots.is_empty() || roots.len() > 8 {
        return Err(SnapshotReadError::Corrupt);
    }
    let roots = roots
        .into_iter()
        .map(|root| {
            sage_acp_protocol::contract::SageRootId::parse(root)
                .map_err(|_| SnapshotReadError::Corrupt)
        })
        .collect::<Result<Vec<_>, _>>()?;
    crate::acp::store::read_snapshot(connection, diff_ref, &roots)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use sage_acp_protocol::page_admission::PageReadAdmission;

    #[test]
    fn snapshot_database_work_does_not_run_on_the_async_executor_thread() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let admission = PageReadAdmission::default();
                let project = SageProjectId::parse("project").unwrap();
                let chat = SageChatId::parse("chat").unwrap();
                let lease = admission.try_acquire(&project, &chat).unwrap();
                let executor = std::thread::current().id();
                let result = read_off_thread(lease, move || {
                    assert_ne!(std::thread::current().id(), executor);
                    Err(SnapshotReadError::Unavailable)
                })
                .await;
                assert_eq!(result, Err(SnapshotReadError::Unavailable));
                assert!(admission.try_acquire(&project, &chat).is_ok());
            });
    }

    #[test]
    fn cancelled_caller_does_not_release_a_running_snapshot_read_slot() {
        struct ReleaseOnDrop(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for ReleaseOnDrop {
            fn drop(&mut self) {
                if let Some(sender) = self.0.take() {
                    let _ = sender.send(());
                }
            }
        }
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let admission = PageReadAdmission::default();
                let project = SageProjectId::parse("project").unwrap();
                let chat = SageChatId::parse("chat").unwrap();
                let _other_reads: Vec<_> = (0..3)
                    .map(|_| admission.try_acquire(&project, &chat).unwrap())
                    .collect();
                let lease = admission.try_acquire(&project, &chat).unwrap();
                let (started, ready) = tokio::sync::oneshot::channel();
                let (release, blocked) = tokio::sync::oneshot::channel();
                let release = ReleaseOnDrop(Some(release));
                let caller = tokio::spawn(read_off_thread(lease, move || {
                    started.send(()).unwrap();
                    blocked.blocking_recv().unwrap();
                    Err(SnapshotReadError::Unavailable)
                }));
                tokio::time::timeout(Duration::from_secs(2), ready)
                    .await
                    .unwrap()
                    .unwrap();
                caller.abort();
                assert!(caller.await.unwrap_err().is_cancelled());
                assert_eq!(
                    admission.try_acquire(&project, &chat).unwrap_err(),
                    sage_acp_protocol::page_admission::PageAdmissionError::Owner
                );
                drop(release);
                tokio::time::timeout(Duration::from_secs(2), async {
                    loop {
                        if admission.try_acquire(&project, &chat).is_ok() {
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
            });
    }

    #[test]
    fn snapshot_owner_check_requires_current_head_and_exact_projection() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE chats(id TEXT,project_id TEXT,projection_revision INTEGER);
            CREATE TABLE chat_session_heads(project_id TEXT,chat_id TEXT,session_id TEXT);
            INSERT INTO chats VALUES('chat','project',7);
            INSERT INTO chat_session_heads VALUES('project','chat','session');",
            )
            .unwrap();
        let reference = DiffRef {
            project_id: "project".into(),
            chat_id: "chat".into(),
            session_id: "session".into(),
            runtime_epoch: 1,
            turn_id: "turn".into(),
            item_id: "item".into(),
            diff_ordinal: 0,
            snapshot_revision: 1,
            root_id: "root".into(),
            relative_path: "src/a.ts".into(),
            content_digest: "a".repeat(64),
        };
        let revision = sage_acp_protocol::contract::ProjectionRevision::new(7).unwrap();
        assert!(verify_current_owner(&connection, revision, &reference).is_ok());
        for field in ["project", "chat", "session"] {
            let mut forged = reference.clone();
            match field {
                "project" => forged.project_id = "foreign".into(),
                "chat" => forged.chat_id = "foreign".into(),
                _ => forged.session_id = "foreign".into(),
            }
            assert_eq!(
                verify_current_owner(&connection, revision, &forged),
                Err(SnapshotReadError::Stale)
            );
        }
        assert_eq!(
            verify_current_owner(
                &connection,
                sage_acp_protocol::contract::ProjectionRevision::new(6).unwrap(),
                &reference
            ),
            Err(SnapshotReadError::Stale)
        );
        connection
            .execute("UPDATE chat_session_heads SET session_id='replacement'", [])
            .unwrap();
        assert_eq!(
            verify_current_owner(&connection, revision, &reference),
            Err(SnapshotReadError::Stale)
        );
    }
}
