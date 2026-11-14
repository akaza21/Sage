use std::time::Instant;

use rusqlite::Connection;
use sage_core::error::AppResult;
use sage_db::error::SqliteResultExt;

use sage_db::database::Database;

pub(super) fn with_snapshot<ResultValue>(
    database: &Database,
    deadline: Instant,
    search: impl FnOnce(&Connection) -> AppResult<ResultValue>,
) -> AppResult<ResultValue> {
    let mut reader = database.chat_search_reader()?;
    let mut locked;
    let connection: &mut Connection = if let Some(reader) = reader.as_mut() {
        reader
    } else {
        locked = database.connection();
        &mut locked
    };
    connection
        .progress_handler(1_000, Some(move || Instant::now() >= deadline))
        .db()?;
    let reset = ResetProgress(connection);
    let _snapshot = reset.0.unchecked_transaction().db()?;
    search(reset.0)
}

/// Clears the deadline handler when the search ends, so the connection
/// goes back to its pool without one.
struct ResetProgress<'connection>(&'connection mut Connection);

impl Drop for ResetProgress<'_> {
    fn drop(&mut self) {
        let _ = self.0.progress_handler(0, None::<fn() -> bool>);
    }
}
