use super::{AcpStore, DatabaseAcpStore, StoreCommit, StoreError, StoreMutation, commit_batch};

impl super::DatabaseAcpStore {
    pub(crate) fn new(database: crate::persistence::Database) -> Self {
        Self { database }
    }
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
