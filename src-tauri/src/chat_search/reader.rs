use std::time::Instant;

use rusqlite::Connection;

use crate::{error::AppResult, persistence::Database};

pub(super) fn with_snapshot<ResultValue>(
    database: &Database,
    deadline: Instant,
    search: impl FnOnce(&Connection) -> AppResult<ResultValue>,
) -> AppResult<ResultValue> {
    let reader = database.chat_search_reader()?;
    let locked;
    let connection = if let Some(reader) = reader.as_ref() {
        &**reader
    } else {
        locked = database.connection();
        &locked
    };
    connection.progress_handler(1_000, Some(move || Instant::now() >= deadline))?;
    let _reset = ResetProgress(connection);
    let _snapshot = connection.unchecked_transaction()?;
    search(connection)
}

struct ResetProgress<'connection>(&'connection Connection);

impl Drop for ResetProgress<'_> {
    fn drop(&mut self) {
        let _ = self.0.progress_handler(0, None::<fn() -> bool>);
    }
}
