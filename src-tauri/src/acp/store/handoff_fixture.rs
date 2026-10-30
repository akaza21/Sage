pub(crate) fn create(connection: &rusqlite::Connection) {
    connection.execute_batch("CREATE TABLE acp_context_handoffs(id TEXT PRIMARY KEY,project_id TEXT,chat_id TEXT,source_session TEXT,target_session TEXT,source_watermark INTEGER,plan_json TEXT,digest TEXT,state TEXT,native_id TEXT,runtime_epoch INTEGER,turn_id TEXT,projection_revision INTEGER,created_at INTEGER,updated_at INTEGER);").unwrap();
}
