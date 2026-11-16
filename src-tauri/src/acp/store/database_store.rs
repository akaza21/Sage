impl super::DatabaseAcpStore {
    pub(crate) fn new(database: sage_db::database::Database) -> Self {
        Self { database }
    }
}
