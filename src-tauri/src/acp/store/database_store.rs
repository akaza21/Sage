use super::{AcpStore, StoreCommit, StoreError, StoreMutation, commit_batch};

pub(crate) struct DatabaseAcpStore {
    pub(super) database: sage_db::database::Database,
}

impl AcpStore for DatabaseAcpStore {
    fn commit(&mut self, batch: &[StoreMutation]) -> Result<StoreCommit, StoreError> {
        let mut connection = self
            .database
            .write_connection()
            .map_err(|_| StoreError::Corrupt("storage recovery required"))?;
        commit_batch(&mut connection, batch)
    }
}

impl super::DatabaseAcpStore {
    pub(crate) fn new(database: sage_db::database::Database) -> Self {
        Self { database }
    }
}
