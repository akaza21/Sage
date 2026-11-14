//! The migrated connection and the anchor that keeps its file in place,
//! handed to `sage_db::database::Database::open`.

use std::path::Path;

use rusqlite::Connection;
use sage_core::error::AppResult;

use super::platform;

pub(crate) struct OpenedDatabase {
    pub(super) connection: Connection,
    pub(super) live_path_anchor: LivePathAnchor,
}

#[derive(Debug)]
pub(crate) struct LivePathAnchor {
    pub(super) _anchor: Option<platform::SqlitePathAnchor>,
}

impl sage_db::database::PathAnchor for LivePathAnchor {
    fn verify_path(&self, path: &Path) -> AppResult<()> {
        self._anchor
            .as_ref()
            .map_or(Ok(()), |anchor| anchor.verify_path(path))
    }
}

impl OpenedDatabase {
    pub(crate) fn into_parts(self) -> (Connection, LivePathAnchor) {
        (self.connection, self.live_path_anchor)
    }
}

impl std::ops::Deref for OpenedDatabase {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        &self.connection
    }
}

impl std::ops::DerefMut for OpenedDatabase {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.connection
    }
}
