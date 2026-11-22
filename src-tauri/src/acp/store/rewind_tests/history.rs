//! Real prompt admission, accepted steering, and native turn settlement.
use super::super::*;
use crate::acp::transcript_quote::Prompt;
use sage_acp_protocol::{
    contract::SafeText,
    intent::{ExternalId, NormalizedPayload},
    lifecycle::{ItemLifecycle, TurnState},
    session_control::ConversationTarget,
};

pub(crate) struct History {
    pub(crate) fixture: turn_undo::fixture::Fixture,
    pub(crate) turns: Vec<SageTurnId>,
}

impl History {
    pub(crate) fn new() -> Self {
        let fixture = turn_undo::fixture::Fixture::new();
        fixture.run();
        Self {
            fixture,
            turns: Vec::new(),
        }
    }

    pub(crate) fn turn(&mut self, prompt: Prompt, steer: Option<&str>) {
        self.turn_with_native(prompt, steer, None);
    }

    pub(crate) fn turn_with_native(
        &mut self,
        prompt: Prompt,
        steer: Option<&str>,
        native: Option<&str>,
    ) {
        let fixture = &self.fixture;
        let session_id = SageAgentSessionId::parse("session-a").unwrap();
        let runtime_epoch = RuntimeEpoch::new(fixture.epoch() as u64).unwrap();
        let expected_projection_revision = fixture.revision();
        let mut connection = fixture.database.connection();
        let prepared = prepare_turn(
            &mut connection,
            PrepareTurnCommand {
                project_id: fixture.project.clone(),
                chat_id: fixture.chat.clone(),
                session_id: session_id.clone(),
                runtime_epoch,
                project_generation: ProjectGeneration::new(1).unwrap(),
                expected_projection_revision,
                client_intent_id: ClientIntentId::new(),
                prompt,
                now_millis: 10,
            },
        )
        .unwrap();
        begin_send(
            &mut connection,
            TurnMutationCommand {
                project_id: fixture.project.clone(),
                chat_id: fixture.chat.clone(),
                session_id: session_id.clone(),
                runtime_epoch,
                turn_id: prepared.turn_id.clone(),
                expected_revision: prepared.turn_revision,
                now_millis: 11,
            },
        )
        .unwrap();
        let binding_digest = connection
            .query_row(
                "SELECT binding_digest FROM agent_sessions WHERE id='session-a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let owner = AdapterStoreOwner {
            project_id: fixture.project.clone(),
            chat_id: fixture.chat.clone(),
            session_id,
            runtime_epoch,
            turn_id: prepared.turn_id.clone(),
            binding_digest,
            external_session_id: ExternalId::new("externalSessionId", "thread-a").unwrap(),
        };
        let mut events = vec![AdapterInboundEvent::Turn {
            state: TurnState::Running,
            native_turn_id: native.map(|id| ExternalId::new("nativeTurnId", id).unwrap()),
            now_millis: 12,
        }];
        if let Some(steer) = steer {
            let payload = NormalizedPayload::UserPrompt {
                text: SafeText::new("text", steer).unwrap(),
            };
            let payload_digest = facts::hex_digest(&serde_json::to_vec(&payload).unwrap());
            events.push(AdapterInboundEvent::Item(ItemIngressCommand {
                project_id: owner.project_id.clone(),
                chat_id: owner.chat_id.clone(),
                session_id: owner.session_id.clone(),
                runtime_epoch,
                turn_id: owner.turn_id.clone(),
                external_item_id: ExternalId::new(
                    "steer",
                    format!("accepted-steer-{}", owner.turn_id.as_str()),
                )
                .unwrap(),
                activity_id: None,
                payload,
                payload_digest,
                lifecycle: ItemLifecycle::Completed,
                tool_state: None,
                now_millis: 13,
            }));
        }
        events.push(AdapterInboundEvent::Turn {
            state: TurnState::Completed,
            native_turn_id: None,
            now_millis: 14,
        });
        apply_adapter_inbound_batch(
            &mut connection,
            AdapterInboundBatchCommand::new(owner, 1024, 4096, events).unwrap(),
        )
        .unwrap();
        self.turns.push(prepared.turn_id);
    }

    pub(crate) fn target(&self, index: usize) -> Result<ConversationTarget, StoreError> {
        rewind::target(
            &self.fixture.database.connection(),
            self.fixture.project.as_str(),
            self.fixture.chat.as_str(),
            "session-a",
            self.turns[index].as_str(),
        )
    }

    /// Seed the old immutable representation for compatibility checks only.
    /// Current prompts and all accepted steers use real admission above.
    pub(crate) fn historical_turn(&mut self, prompt: Prompt, native: Option<&str>) {
        let turn = SageTurnId::new();
        self.fixture.turn(turn.as_str(), self.turns.len() as i64);
        let payload =
            serde_json::json!({"kind":"text","payload":prompt.user_payload()}).to_string();
        let epoch = RuntimeEpoch::new(self.fixture.epoch() as u64).unwrap();
        let mut connection = self.fixture.database.connection();
        connection.execute(
            "INSERT INTO items(id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,
             origin,kind,lifecycle,payload_json,payload_bytes,payload_digest,revision,created_at,updated_at,terminal_at)
             VALUES(?1,?2,?3,'session-a',?1,?4,0,'primary','text','completed',?5,?6,?7,0,1,1,1)",
            rusqlite::params![turn.as_str(), self.fixture.project.as_str(), self.fixture.chat.as_str(),
                epoch.get() as i64, payload, payload.len() as i64, facts::hex_digest(payload.as_bytes())]).unwrap();
        if let Some(native) = native {
            let owner = AdapterStoreOwner {
                project_id: self.fixture.project.clone(),
                chat_id: self.fixture.chat.clone(),
                session_id: SageAgentSessionId::parse("session-a").unwrap(),
                runtime_epoch: epoch,
                turn_id: turn.clone(),
                binding_digest: String::new(),
                external_session_id: ExternalId::new("externalSessionId", "thread-a").unwrap(),
            };
            let tx = connection.transaction().unwrap();
            rewind::record_native_turn(
                &tx,
                &owner,
                &ExternalId::new("nativeTurnId", native).unwrap(),
                15,
            )
            .unwrap();
            tx.commit().unwrap();
        }
        self.turns.push(turn);
    }
}

pub(crate) fn quoted_prompt() -> Prompt {
    let contexts = serde_json::from_value(serde_json::json!([{
        "kind":"transcriptQuote","ordinal":1,"sourceTurnId":"prior-turn",
        "sourceItemId":"answer","text":"quoted answer","comment":"Why?",
        "startOffset":0,"endOffset":13
    }]))
    .unwrap();
    crate::acp::attached_context::effective_prompt(
        &Prompt::new("prompt", "Explain").unwrap(),
        &contexts,
    )
    .unwrap()
}

pub(crate) fn file_prompt() -> Prompt {
    use crate::acp::attached_context::files::{Resource, Resources};
    let mut prompt = quoted_prompt();
    prompt.resources = Resources(std::sync::Arc::new(vec![Resource {
        id: "attachment".into(),
        name: "context.txt".into(),
        bytes: b"snapshot-only\n".to_vec(),
    }]));
    prompt
}
