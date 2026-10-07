impl super::DatabaseAcpStore {
    pub(crate) fn new(database: crate::persistence::Database) -> Self {
        Self { database }
    }
}
