use rusqlite::{Connection, OptionalExtension, params, types::ValueRef};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::acp::{
    contract::{SageChatId, SageProjectId},
    store::StoreError,
};

const MAX_TABLE_ROWS: u64 = 200_000;
const TABLES: [&str; 14] = [
    "chats",
    "chat_tabs",
    "agent_sessions",
    "chat_session_heads",
    "session_runtime_epochs",
    "turns",
    "session_activities",
    "items",
    "chat_lineage",
    "server_requests",
    "session_facts",
    "acp_project_usage",
    "acp_session_usage",
    "acp_store_usage",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RendererRecreationNativeSnapshot {
    schema_version: u8,
    project_id: SageProjectId,
    chat_id: SageChatId,
    tables: Vec<TableSnapshot>,
    chat_projection_revision: i64,
    runtime_slot_active: u8,
    live_process_tree_reservations: u32,
    live_descendant_reservations: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct TableSnapshot {
    name: &'static str,
    rows: u64,
    content_sha256: String,
}

impl RendererRecreationNativeSnapshot {
    pub(super) fn capture(
        connection: &Connection,
        project_id: &SageProjectId,
        chat_id: &SageChatId,
        runtime_slot_active: u8,
        live_process_tree_reservations: u32,
        live_descendant_reservations: u32,
    ) -> Result<Self, StoreError> {
        let chat_projection_revision = connection
            .query_row(
                "SELECT projection_revision FROM chats WHERE project_id=?1 AND id=?2",
                params![project_id.as_str(), chat_id.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or(StoreError::Conflict { resource: "chat" })?;
        if chat_projection_revision < 0 {
            return Err(StoreError::Corrupt("negative Chat projection revision"));
        }
        let tables = TABLES
            .iter()
            .map(|table| snapshot_table(connection, table))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            schema_version: 1,
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            tables,
            chat_projection_revision,
            runtime_slot_active,
            live_process_tree_reservations,
            live_descendant_reservations,
        })
    }

    #[cfg(test)]
    pub(crate) fn table_names(&self) -> [&'static str; 14] {
        TABLES
    }

    #[cfg(test)]
    pub(crate) fn maximum_rows_per_table(&self) -> u64 {
        MAX_TABLE_ROWS
    }

    #[cfg(test)]
    pub(crate) fn table_rows(&self, name: &str) -> Option<u64> {
        self.tables
            .iter()
            .find(|table| table.name == name)
            .map(|table| table.rows)
    }

    pub(crate) fn project_id(&self) -> &SageProjectId {
        &self.project_id
    }

    pub(crate) fn chat_id(&self) -> &SageChatId {
        &self.chat_id
    }

    pub(crate) fn chat_projection_revision(&self) -> i64 {
        self.chat_projection_revision
    }

    #[cfg(test)]
    pub(crate) fn runtime_slot_active(&self) -> u8 {
        self.runtime_slot_active
    }

    #[cfg(test)]
    pub(crate) fn live_process_tree_reservations(&self) -> u32 {
        self.live_process_tree_reservations
    }
}

fn snapshot_table(
    connection: &Connection,
    table: &'static str,
) -> Result<TableSnapshot, StoreError> {
    let sql = format!(
        "SELECT * FROM \"{table}\" ORDER BY rowid LIMIT {}",
        MAX_TABLE_ROWS + 1
    );
    let mut statement = connection.prepare(&sql)?;
    let column_count = statement.column_count();
    let mut query = statement.query([])?;
    let mut rows = 0_u64;
    let mut digest = Sha256::new();
    digest.update(table.as_bytes());
    digest.update([0]);
    digest.update(
        u64::try_from(column_count)
            .unwrap_or(u64::MAX)
            .to_le_bytes(),
    );
    while let Some(row) = query.next()? {
        rows = rows.saturating_add(1);
        if rows > MAX_TABLE_ROWS {
            return Err(StoreError::Capacity {
                resource: "rendererRecreationTableRows",
                limit: MAX_TABLE_ROWS,
            });
        }
        digest.update(rows.to_le_bytes());
        for column in 0..column_count {
            hash_value(&mut digest, row.get_ref(column)?);
        }
    }
    Ok(TableSnapshot {
        name: table,
        rows,
        content_sha256: format!("{:x}", digest.finalize()),
    })
}

fn hash_value(digest: &mut Sha256, value: ValueRef<'_>) {
    match value {
        ValueRef::Null => digest.update([0]),
        ValueRef::Integer(value) => {
            digest.update([1]);
            digest.update(value.to_le_bytes());
        }
        ValueRef::Real(value) => {
            digest.update([2]);
            digest.update(value.to_bits().to_le_bytes());
        }
        ValueRef::Text(value) => hash_bytes(digest, 3, value),
        ValueRef::Blob(value) => hash_bytes(digest, 4, value),
    }
}

fn hash_bytes(digest: &mut Sha256, kind: u8, value: &[u8]) {
    digest.update([kind]);
    digest.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_le_bytes());
    digest.update(value);
}
