use std::path::Path;

use rusqlite::{Connection, params};
use sage_core::error::AppResult;

use crate::acp::migration;

use super::store::{full_path, validate_root};

/// Safe once sent: the transcript keeps its own copy. Queued images are the queue's job.
pub(crate) fn retire_sent_images(
    connection: &Connection,
    root: &Path,
    project_id: &str,
    chat_id: &str,
    artifact_ids: &[&str],
) -> AppResult<()> {
    validate_root(root)?;
    for id in artifact_ids {
        let retired = connection.execute(
            "UPDATE chat_image_artifacts SET lifecycle='retired',updated_at=?4
             WHERE id=?1 AND project_id=?2 AND chat_id=?3 AND lifecycle='pending'",
            params![id, project_id, chat_id, sage_core::time::now_millis()],
        )?;
        if retired == 1 {
            let _ = migration::remove_image_file(&full_path(root, id));
        }
    }
    Ok(())
}
