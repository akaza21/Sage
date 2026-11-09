//! Commits a binding's facts through the store's own adapter batch apply
//! against a migrated in-memory database and reads the transcript back
//! through the chat page projection, so every rule the store enforces
//! (terminal items, lifecycle and tool-state transitions, items or turn
//! changes after the turn is terminal, file pair shape) refuses the sequence
//! exactly as it would in the app.

use serde_json::{Map, Value, json};

use super::Fact;
use crate::acp::{
    contract::{
        ClientIntentId, EntityRevision, RequiredText, RuntimeEpoch, SageAgentSessionId, SageChatId,
        SageProjectId, SageTurnId, SchemaVersionV1,
    },
    intent::{CapabilitySnapshotV1, ExternalId},
    store::{
        AdapterInboundBatchCommand, AdapterInboundEvent, AdapterStoreOwner, BeginCancelCommand,
        FilePairCommand, ItemIngressCommand, StoreError, TurnMutationCommand,
        apply_adapter_inbound_batch, begin_cancel, hex_digest, load_chat_page,
    },
};

const SETUP: &str = "
INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project-one','Parity',1,1,1);
INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal) VALUES('root-1','project-one','C:/repo','c:/repo',0);
INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES('chat-one','project-one',0,'Parity',0,1,1);
INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at) VALUES('session-one','project-one','chat-one',0,'binding','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','parity-session','{capabilities}','active','hot',0,0,0,1,1);
INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at) VALUES('project-one','chat-one','session-one',0,1);
INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) VALUES('project-one','chat-one','session-one',1,'running','dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',1);
INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at) VALUES('turn-one','project-one','chat-one','session-one',1,0,'intent-one','running','correlated',0,1,1);
INSERT INTO acp_project_usage(project_id,chat_count,session_count,turn_count,item_count,fact_count,payload_bytes,revision) VALUES('project-one',1,1,1,0,0,0,0);
INSERT INTO acp_session_usage(project_id,chat_id,session_id,turn_count,item_count,item_bytes,fact_count,fact_bytes,payload_bytes,revision) VALUES('project-one','chat-one','session-one',1,0,0,0,0,0,0);
UPDATE acp_store_usage SET chat_count=1,session_count=1,turn_count=1 WHERE singleton=1;";

struct Store {
    connection: rusqlite::Connection,
    owner: AdapterStoreOwner,
    now: i64,
}

impl Store {
    fn new() -> Self {
        let (connection, _) =
            crate::acp::migration::open_or_migrate(std::path::Path::new(":memory:"))
                .expect("migrated store")
                .into_parts();
        let capabilities = serde_json::to_string(&CapabilitySnapshotV1 {
            schema_version: SchemaVersionV1,
            protocol_version: RequiredText::new("protocol", "1").unwrap(),
            new_session: true,
            resume_session: false,
            load_session: false,
            close_session: true,
            prompt: true,
            cancel: true,
            native_session_fork: false,
            native_session_compact: false,
            native_session_import: false,
            native_review: false,
            active_turn_steer: false,
            child_conversation_projection: false,
            child_transcript_read: false,
            collaboration_event_projection: false,
            multi_root_extension_schema_id: None,
            model_count: 0,
            prompt_resources: false,
            prompt_images: false,
            config_count: 0,
        })
        .unwrap();
        connection
            .execute_batch(&SETUP.replace("{capabilities}", &capabilities))
            .expect("parity owner");
        Self {
            connection,
            owner: AdapterStoreOwner {
                project_id: SageProjectId::parse("project-one").unwrap(),
                chat_id: SageChatId::parse("chat-one").unwrap(),
                session_id: SageAgentSessionId::parse("session-one").unwrap(),
                runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                turn_id: SageTurnId::parse("turn-one").unwrap(),
                binding_digest: "a".repeat(64),
                external_session_id: ExternalId::new("external", "parity-session").unwrap(),
            },
            now: 10,
        }
    }

    fn tick(&mut self) -> i64 {
        self.now += 1;
        self.now
    }

    fn item(&mut self, key: &str, fact: Fact) -> AdapterInboundEvent {
        let Fact::Item {
            lifecycle,
            tool_state,
            payload,
            ..
        } = fact
        else {
            unreachable!()
        };
        let json = serde_json::to_string(&payload).expect("payload json");
        let owner = &self.owner;
        AdapterInboundEvent::Item(ItemIngressCommand {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            turn_id: owner.turn_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            external_item_id: ExternalId::new("external", key).expect("item key"),
            activity_id: None,
            payload,
            payload_digest: hex_digest(json.as_bytes()),
            lifecycle,
            tool_state,
            now_millis: self.tick(),
        })
    }

    fn pair(&mut self, key: String, old_text: String, new_text: String) -> AdapterInboundEvent {
        let owner = &self.owner;
        AdapterInboundEvent::FilePair(FilePairCommand {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            turn_id: owner.turn_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            external_item_id: ExternalId::new("external", key).expect("pair key"),
            old_text,
            new_text,
            now_millis: self.now,
        })
    }

    fn commit(&mut self, events: Vec<AdapterInboundEvent>) -> Result<(), StoreError> {
        let command = AdapterInboundBatchCommand::new(self.owner.clone(), 1024, 4096, events)?;
        apply_adapter_inbound_batch(&mut self.connection, command).map(drop)
    }

    /// The durable cancel intent Sage records before it writes the native
    /// cancel, which is what lets the terminal Turn carry the user's stop.
    fn cancel(&mut self) -> Result<(), StoreError> {
        let revision: i64 = self.connection.query_row(
            "SELECT revision FROM turns WHERE id='turn-one'",
            [],
            |row| row.get(0),
        )?;
        let owner = self.owner.clone();
        let now_millis = self.tick();
        begin_cancel(
            &mut self.connection,
            BeginCancelCommand {
                turn: TurnMutationCommand {
                    project_id: owner.project_id,
                    chat_id: owner.chat_id,
                    session_id: owner.session_id,
                    runtime_epoch: owner.runtime_epoch,
                    turn_id: owner.turn_id,
                    expected_revision: EntityRevision::new(revision as u64).expect("revision"),
                    now_millis,
                },
                client_intent_id: ClientIntentId::parse("render-parity-cancel").unwrap(),
            },
        )
        .map(drop)
    }
}

/// Commits each fact as one adapter batch (a file pair travels in the same
/// batch as its item, as the store requires) and projects the chat page.
pub(super) fn project(facts: Vec<Fact>) -> Result<Value, StoreError> {
    let mut store = Store::new();
    let mut facts = facts.into_iter().peekable();
    while let Some(fact) = facts.next() {
        match fact {
            Fact::Item { ref key, .. } => {
                let key = key.clone();
                let mut events = vec![store.item(&key, fact)];
                if let Some(Fact::Pair { key: paired, .. }) = facts.peek()
                    && *paired == key
                {
                    let Some(Fact::Pair {
                        key,
                        old_text,
                        new_text,
                    }) = facts.next()
                    else {
                        unreachable!()
                    };
                    events.push(store.pair(key, old_text, new_text));
                }
                store.commit(events)?;
            }
            Fact::Pair {
                key,
                old_text,
                new_text,
            } => {
                let pair = store.pair(key, old_text, new_text);
                store.commit(vec![pair])?;
            }
            Fact::Turn(state) => {
                let now_millis = store.tick();
                store.commit(vec![AdapterInboundEvent::Turn { state, now_millis }])?;
            }
            Fact::UserCancel => store.cancel()?,
        }
    }
    let page = load_chat_page(
        &store.connection,
        store.owner.project_id.clone(),
        store.owner.chat_id.clone(),
        None,
    )?;
    let [turn] = page.turns.as_slice() else {
        panic!("one parity turn")
    };
    let items = turn
        .items
        .as_slice()
        .iter()
        .map(|item| {
            let mut row = Map::new();
            row.insert("lifecycle".into(), json!(item.lifecycle));
            row.insert("toolState".into(), json!(item.tool_state));
            let Value::Object(content) = serde_json::to_value(&item.content).unwrap() else {
                unreachable!()
            };
            row.extend(content);
            super::strip_volatile(Value::Object(row))
        })
        .collect::<Vec<_>>();
    Ok(json!({ "turnState": turn.state, "items": items }))
}
