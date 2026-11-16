use sage_core::error::{AppError, AppResult, DatabaseError};

/// The `AppError` for a failed rusqlite call. `sage-core` does not link
/// SQLite, so the driver's error is wrapped here; its message, `Debug` form
/// and source read as they did when `AppError` held it directly.
pub fn app_error(error: rusqlite::Error) -> AppError {
    AppError::Database(DatabaseError::new(error))
}

/// `.db()?` turns a rusqlite result into an `AppResult`, as `?` alone did
/// while `AppError` held the rusqlite error itself.
pub trait SqliteResultExt<T> {
    fn db(self) -> AppResult<T>;
}

impl<T> SqliteResultExt<T> for rusqlite::Result<T> {
    fn db(self) -> AppResult<T> {
        self.map_err(app_error)
    }
}
