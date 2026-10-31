use super::bootstrap_support::WAIT;
use crate::persistence::Database;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use std::sync::mpsc;

/// Observe real owner reads without replacing their result or blocking SQLite.
/// The idle two-turn fixture has no other owner reader at this point. The
/// attached call reads its owner at entry and at the attached attempt, then
/// after UserControl and on each iteration of the actual pause loop. Reads
/// four/five therefore witness two pause-loop checks while Chat one is held.
pub(super) struct PauseObservation {
    database: Database,
    ticks: mpsc::Receiver<usize>,
}

impl PauseObservation {
    pub(super) fn arm(database: &Database) -> Self {
        let (sender, ticks) = mpsc::sync_channel(2);
        let mut reads = 0;
        database
            .connection()
            .authorizer(Some(move |context: AuthContext<'_>| {
                if matches!(
                    context.action,
                    AuthAction::Read {
                        table_name: "agent_sessions",
                        column_name: "binding_digest"
                    }
                ) && context.accessor.is_none()
                {
                    reads += 1;
                    if matches!(reads, 4 | 5) {
                        let _ = sender.try_send(reads);
                    }
                }
                Authorization::Allow
            }))
            .unwrap();
        Self {
            database: database.clone(),
            ticks,
        }
    }

    pub(super) fn next(&self, expected: usize) {
        assert_eq!(
            self.ticks
                .recv_timeout(WAIT)
                .expect("actual paused-call owner check"),
            expected
        );
    }
}

impl Drop for PauseObservation {
    fn drop(&mut self) {
        self.database
            .connection()
            .authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
            .unwrap();
    }
}
