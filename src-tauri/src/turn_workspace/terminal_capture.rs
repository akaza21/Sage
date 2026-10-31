use rusqlite::{Connection, params};
use std::time::Duration;

use super::{TurnWorkspaceChanges, WorkspaceMark, capture_after, diff_marks};
use crate::{
    acp::store::{SendObservation, StoreError, StoreMutation},
    persistence::Database,
};

struct CaptureJob {
    session: String,
    key: String,
    root: String,
    before: Option<WorkspaceMark>,
    ambiguous: bool,
    baseline_failed: bool,
}

type TerminalScope<'a> = (&'a str, &'a str, &'a str, u64, Option<&'a str>);

pub(crate) fn needs_terminal_capture(mutations: &[StoreMutation]) -> bool {
    mutations
        .iter()
        .any(|mutation| terminal_scope(mutation).is_some())
}

fn terminal_scope(mutation: &StoreMutation) -> Option<TerminalScope<'_>> {
    match mutation {
        StoreMutation::TurnTerminal {
            command, terminal, ..
        } if terminal.is_terminal() => Some((
            command.project_id.as_str(),
            command.chat_id.as_str(),
            command.session_id.as_str(),
            command.runtime_epoch.get(),
            Some(command.turn_id.as_str()),
        )),
        StoreMutation::ObserveSend {
            command,
            observation: SendObservation::PossibleBytes | SendObservation::ProcessLost,
            ..
        } => Some((
            command.project_id.as_str(),
            command.chat_id.as_str(),
            command.session_id.as_str(),
            command.runtime_epoch.get(),
            Some(command.turn_id.as_str()),
        )),
        StoreMutation::AdapterInboundBatch(command) if command.ends_turn() => {
            let owner = command.owner();
            Some((
                owner.project_id.as_str(),
                owner.chat_id.as_str(),
                owner.session_id.as_str(),
                owner.runtime_epoch.get(),
                Some(owner.turn_id.as_str()),
            ))
        }
        StoreMutation::FinalizeClose(command) => Some((
            command.request.project_id.as_str(),
            command.request.chat_id.as_str(),
            command.request.session_id.as_str(),
            command.request.runtime_epoch.get(),
            None,
        )),
        StoreMutation::RecoverRuntimeAfterZero(command) => Some((
            command.project_id.as_str(),
            command.chat_id.as_str(),
            command.session_id.as_str(),
            command.runtime_epoch.get(),
            None,
        )),
        _ => None,
    }
}

pub(crate) fn prepare_terminal(
    database: &Database,
    mutations: &[StoreMutation],
    budget: Duration,
) -> Result<(), StoreError> {
    let deadline = std::time::Instant::now() + budget;
    for mutation in mutations {
        let scope = terminal_scope(mutation);
        let Some((project, chat, session, epoch, turn)) = scope else {
            continue;
        };
        let jobs = pending_scoped(&database.connection(), project, chat, session, epoch, turn)?;
        for job in jobs {
            let json = snapshot(
                &job,
                deadline.saturating_duration_since(std::time::Instant::now()),
            )?;
            let connection = database
                .write_connection()
                .map_err(|_| StoreError::Corrupt("workspace capture storage"))?;
            save(&connection, &job, &json)?;
        }
    }
    Ok(())
}

fn pending_scoped(
    connection: &Connection,
    project: &str,
    chat: &str,
    session: &str,
    epoch: u64,
    turn: Option<&str>,
) -> Result<Vec<CaptureJob>, StoreError> {
    let active: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM turns WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND runtime_epoch=?4 AND (?5 IS NULL OR id=?5) AND terminal_at IS NULL)",params![project,chat,session,epoch as i64,turn],|row| row.get(0))?;
    if !active {
        return Ok(Vec::new());
    }
    pending(connection, session)
}

fn pending(connection: &Connection, session: &str) -> Result<Vec<CaptureJob>, StoreError> {
    let mut statement = connection.prepare("SELECT w.client_intent_id,w.root_path,w.baseline_json,w.ambiguous FROM acp_turn_workspace_changes w JOIN turns t ON t.session_id=w.session_id AND t.client_intent_id=w.client_intent_id WHERE w.session_id=?1 AND w.result_json IS NULL AND t.terminal_at IS NULL LIMIT 2")?;
    let rows = statement
        .query_map([session], |row| {
            let baseline: Option<String> = row.get(2)?;
            Ok(CaptureJob {
                session: session.to_owned(),
                key: row.get(0)?,
                root: row.get(1)?,
                baseline_failed: baseline.as_deref() == Some(super::FAILED_BASELINE_JSON),
                before: baseline.and_then(|json| serde_json::from_str(&json).ok()),
                ambiguous: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() > 1 {
        return Err(StoreError::Corrupt("multiple active workspace captures"));
    }
    Ok(rows)
}

fn snapshot(job: &CaptureJob, budget: Duration) -> Result<String, StoreError> {
    let changes = if job.baseline_failed {
        TurnWorkspaceChanges::incomplete()
    } else if job.ambiguous {
        TurnWorkspaceChanges::untracked()
    } else if let Some(before) = &job.before {
        let root = job.root.clone();
        let baseline = before.clone();
        let now = bounded_snapshot(budget, move || {
            tauri::async_runtime::block_on(async {
                tokio::time::timeout(
                    budget,
                    capture_after(std::path::Path::new(&root), &baseline),
                )
                .await
                .ok()
                .flatten()
            })
        });
        now.map_or_else(TurnWorkspaceChanges::incomplete, |now| {
            diff_marks(before, &now)
        })
    } else {
        TurnWorkspaceChanges::untracked()
    };
    serde_json::to_string(&changes).map_err(|_| StoreError::Corrupt("terminal workspace encoding"))
}

fn bounded_snapshot(
    budget: Duration,
    work: impl FnOnce() -> Option<WorkspaceMark> + Send + 'static,
) -> Option<WorkspaceMark> {
    static WORKERS: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
        std::sync::OnceLock::new();
    bounded_snapshot_with(
        WORKERS
            .get_or_init(|| {
                std::sync::Arc::new(tokio::sync::Semaphore::new(super::MAX_CONCURRENT_CAPTURES))
            })
            .clone(),
        budget,
        work,
    )
}

fn bounded_snapshot_with(
    workers: std::sync::Arc<tokio::sync::Semaphore>,
    budget: Duration,
    work: impl FnOnce() -> Option<WorkspaceMark> + Send + 'static,
) -> Option<WorkspaceMark> {
    if budget.is_zero() {
        return None;
    }
    let permit = workers.try_acquire_owned().ok()?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let _worker = std::thread::Builder::new()
        .name("sage-terminal-snapshot".to_owned())
        .spawn(move || {
            let result = work();
            drop(permit);
            let _ = sender.send(result);
        })
        .ok()?;
    receiver.recv_timeout(budget).ok().flatten()
}

fn save(connection: &Connection, job: &CaptureJob, json: &str) -> Result<(), StoreError> {
    let untracked = serde_json::to_string(&TurnWorkspaceChanges::untracked())
        .map_err(|_| StoreError::Corrupt("terminal workspace encoding"))?;
    connection.execute("UPDATE acp_turn_workspace_changes SET terminal_json=CASE WHEN length(CAST(?3 AS BLOB))+length(CAST(root_path AS BLOB))+length(CAST(root_id AS BLOB))+coalesce(length(CAST(baseline_json AS BLOB)),0)<=payload_bytes THEN ?3 WHEN length(CAST(?4 AS BLOB))+length(CAST(root_path AS BLOB))+length(CAST(root_id AS BLOB))+coalesce(length(CAST(baseline_json AS BLOB)),0)<=payload_bytes THEN ?4 ELSE NULL END WHERE session_id=?1 AND client_intent_id=?2 AND result_json IS NULL", params![job.session,job.key,json,untracked])?;
    Ok(())
}

pub(crate) fn discard_rejected_staging(
    database: Option<&Database>,
    mutations: &[StoreMutation],
    result: &Result<crate::acp::store::StoreCommit, StoreError>,
) {
    if matches!(result,Err(error) if !matches!(error,StoreError::CommitUnknown))
        && let Some(database) = database
        && let Ok(connection) = database.write_connection()
    {
        for mutation in mutations {
            if let Some((project, chat, session, epoch, turn)) = terminal_scope(mutation) {
                let _ = connection.execute("UPDATE acp_turn_workspace_changes SET terminal_json=NULL WHERE session_id=?3 AND result_json IS NULL AND terminal_json IS NOT NULL AND EXISTS(SELECT 1 FROM turns t WHERE t.session_id=acp_turn_workspace_changes.session_id AND t.client_intent_id=acp_turn_workspace_changes.client_intent_id AND t.project_id=?1 AND t.chat_id=?2 AND t.runtime_epoch=?4 AND (?5 IS NULL OR t.id=?5) AND t.terminal_at IS NULL)",params![project,chat,session,epoch as i64,turn]);
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn freeze_session_for_test(
    connection: &Connection,
    session: &str,
) -> Result<(), StoreError> {
    for job in pending(connection, session)? {
        save(connection, &job, &snapshot(&job, Duration::from_secs(5))?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stalled_optional_snapshots_keep_their_slots_until_actual_worker_exit() {
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
            mpsc::sync_channel,
        };
        let workers = Arc::new(tokio::sync::Semaphore::new(4));
        let (release, blocked) = sync_channel(4);
        let blocked = Arc::new(Mutex::new(blocked));
        let (started_sender, started_receiver) = sync_channel(4);
        let (exited_sender, exited_receiver) = sync_channel(4);
        let started = std::time::Instant::now();
        for _index in 0..4 {
            let blocked = blocked.clone();
            let entered = started_sender.clone();
            let exited = exited_sender.clone();
            assert!(
                bounded_snapshot_with(workers.clone(), Duration::from_millis(20), move || {
                    entered.send(()).unwrap();
                    blocked
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap();
                    exited.send(()).unwrap();
                    None
                })
                .is_none()
            );
            started_receiver
                .recv_timeout(Duration::from_secs(1))
                .unwrap();
        }
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(workers.available_permits(), 0);
        let called = Arc::new(AtomicBool::new(false));
        let fifth = called.clone();
        assert!(
            bounded_snapshot_with(workers.clone(), Duration::from_millis(20), move || {
                fifth.store(true, Ordering::Release);
                None
            })
            .is_none()
        );
        assert!(!called.load(Ordering::Acquire));
        for _index in 0..4 {
            release.send(()).unwrap();
        }
        for _index in 0..4 {
            exited_receiver
                .recv_timeout(Duration::from_secs(1))
                .unwrap();
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while workers.available_permits() != 4 && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert_eq!(workers.available_permits(), 4);
        let restored = called.clone();
        assert!(
            bounded_snapshot_with(workers, Duration::from_secs(1), move || {
                restored.store(true, Ordering::Release);
                None
            })
            .is_none()
        );
        assert!(called.load(Ordering::Acquire));
    }

    #[test]
    fn staging_never_charges_more_than_the_reserved_payload_even_for_fallback() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE acp_turn_workspace_changes(session_id TEXT,client_intent_id TEXT,root_path TEXT,root_id TEXT,baseline_json TEXT,terminal_json TEXT,result_json TEXT,payload_bytes INTEGER);").unwrap();
        connection.execute("INSERT INTO acp_turn_workspace_changes VALUES('session','intent','root','root',?1,NULL,NULL,100)",["x".repeat(92)]).unwrap();
        let job = CaptureJob {
            session: "session".to_owned(),
            key: "intent".to_owned(),
            root: "root".to_owned(),
            before: None,
            ambiguous: false,
            baseline_failed: false,
        };
        save(&connection, &job, "{}").unwrap();
        let result: (bool,i64) = connection.query_row("SELECT terminal_json IS NULL,length(baseline_json)+length(root_path)+length(root_id)+coalesce(length(terminal_json),0) FROM acp_turn_workspace_changes",[],|row| Ok((row.get(0)?,row.get(1)?))).unwrap();
        assert_eq!(result, (true, 100));
    }
}
