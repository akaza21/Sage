use rusqlite::{Connection, params};

use crate::persistence::Database;

pub(super) const DIGEST_A: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const DIGEST_B: &str =
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

pub(super) struct SchemaFixture {
    pub(super) database: Database,
}

impl SchemaFixture {
    pub(super) fn project_chat() -> Self {
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        {
            let connection = database.connection();
            insert_project(&connection, "project-a");
            connection
                .execute(
                    "INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal)
                     VALUES('root-a','project-a','C:/project','c:/project',0)",
                    [],
                )
                .unwrap();
            insert_chat(&connection, "project-a", "chat-a", 0);
        }
        Self { database }
    }

    pub(super) fn session() -> Self {
        Self::session_with_binding(DIGEST_A)
    }

    pub(super) fn session_with_binding(binding_digest: &str) -> Self {
        let fixture = Self::project_chat();
        insert_session(
            &fixture.database.connection(),
            "project-a",
            "chat-a",
            "session-a",
            0,
            "ready",
            "hot",
            None,
            binding_digest,
        );
        fixture
    }

    pub(super) fn graph() -> Self {
        Self::graph_with_binding(DIGEST_A)
    }

    pub(super) fn graph_with_binding(binding_digest: &str) -> Self {
        let fixture = Self::session_with_binding(binding_digest);
        let connection = fixture.database.connection();
        insert_epoch(&connection, 1, "exited", Some(1));
        insert_epoch(&connection, 2, "running", None);
        insert_turn(
            &connection,
            "turn-old",
            1,
            0,
            "completed",
            "correlated",
            Some(2),
        );
        insert_item(&connection, "item-old", "turn-old", 1, 0, None);
        insert_turn(&connection, "turn-a", 2, 1, "running", "notWritten", None);
        insert_item(&connection, "item-a", "turn-a", 2, 0, None);
        drop(connection);
        fixture
    }
}

pub(super) fn insert_project(connection: &Connection, id: &str) {
    connection
        .execute(
            "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at)
             VALUES (?1,?1,1,1,1)",
            [id],
        )
        .unwrap();
}

pub(super) fn insert_chat(connection: &Connection, project_id: &str, id: &str, ordinal: i64) {
    connection
        .execute(
            "INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at)
             VALUES (?1,?2,?3,?1,1,1)",
            params![id, project_id, ordinal],
        )
        .unwrap();
}

#[allow(clippy::too_many_arguments)]
pub(super) fn insert_session(
    connection: &Connection,
    project_id: &str,
    chat_id: &str,
    id: &str,
    ordinal: i64,
    state: &str,
    residency: &str,
    terminal_at: Option<i64>,
    binding_digest: &str,
) {
    connection
        .execute(
            "INSERT INTO agent_sessions(
               id,project_id,chat_id,ordinal,binding_id,binding_digest,
               capabilities_json,state,residency,legacy,revision,
               projection_revision,created_at,updated_at,terminal_at
             ) VALUES (?1,?2,?3,?4,'binding',?5,'{}',?6,?7,0,0,0,1,1,?8)",
            params![
                id,
                project_id,
                chat_id,
                ordinal,
                binding_digest,
                state,
                residency,
                terminal_at
            ],
        )
        .unwrap();
}

pub(super) fn insert_epoch(
    connection: &Connection,
    runtime_epoch: i64,
    state: &str,
    terminal_at: Option<i64>,
) {
    connection
        .execute(
            "INSERT INTO session_runtime_epochs(
               project_id,chat_id,session_id,runtime_epoch,state,started_at,terminal_at
             ) VALUES ('project-a','chat-a','session-a',?1,?2,1,?3)",
            params![runtime_epoch, state, terminal_at],
        )
        .unwrap();
}

pub(super) fn insert_turn(
    connection: &Connection,
    id: &str,
    runtime_epoch: i64,
    ordinal: i64,
    state: &str,
    delivery_state: &str,
    terminal_at: Option<i64>,
) {
    connection
        .execute(
            "INSERT INTO turns(
               id,project_id,chat_id,session_id,runtime_epoch,ordinal,
               client_intent_id,state,delivery_state,created_at,updated_at,terminal_at
             ) VALUES (?1,'project-a','chat-a','session-a',?2,?3,?1,?4,?5,1,1,?6)",
            params![
                id,
                runtime_epoch,
                ordinal,
                state,
                delivery_state,
                terminal_at
            ],
        )
        .unwrap();
}

pub(super) fn insert_item(
    connection: &Connection,
    id: &str,
    turn_id: &str,
    runtime_epoch: i64,
    ordinal: i64,
    external_id: Option<&str>,
) {
    connection
        .execute(
            "INSERT INTO items(
               id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,
               origin,kind,lifecycle,external_item_id,payload_json,payload_bytes,
               payload_digest,revision,created_at,updated_at,terminal_at
             ) VALUES (?1,'project-a','chat-a','session-a',?2,?3,?4,
                       'primary','text','completed',?5,'{}',2,?6,0,1,1,2)",
            params![id, turn_id, runtime_epoch, ordinal, external_id, DIGEST_A],
        )
        .unwrap();
}

// This test-only helper mirrors the eight independently constrained columns in
// the production activity table so individual fixture vectors stay explicit.
#[allow(clippy::too_many_arguments)]
pub(super) fn insert_activity(
    connection: &Connection,
    id: &str,
    turn_id: &str,
    runtime_epoch: i64,
    parent_item_id: &str,
    external_id: &str,
    lifecycle: &str,
    revision: i64,
    terminal_at: Option<i64>,
) -> rusqlite::Result<usize> {
    connection.execute(
        "INSERT INTO session_activities(
           id,project_id,chat_id,session_id,turn_id,runtime_epoch,
           external_activity_id,activity_kind,parent_item_id,role,lifecycle,
           depth,observed_sequence,provenance_json,revision,
           created_at,updated_at,terminal_at
         ) VALUES (?1,'project-a','chat-a','session-a',?2,?3,?4,'subagent',?5,
                   'child',?6,1,1,'{}',?7,1,1,?8)",
        params![
            id,
            turn_id,
            runtime_epoch,
            external_id,
            parent_item_id,
            lifecycle,
            revision,
            terminal_at
        ],
    )
}

pub(super) fn count(connection: &Connection, table: &str) -> i64 {
    let sql = match table {
        "items" => "SELECT COUNT(*) FROM items",
        "server_requests" => "SELECT COUNT(*) FROM server_requests",
        "session_activities" => "SELECT COUNT(*) FROM session_activities",
        _ => panic!("unapproved fixture table"),
    };
    connection.query_row(sql, [], |row| row.get(0)).unwrap()
}
