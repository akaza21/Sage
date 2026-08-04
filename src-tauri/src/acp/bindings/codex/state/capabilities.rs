use serde_json::{Map, Value};

use crate::acp::{
    contract::{RequiredText, SchemaVersionV1},
    intent::CapabilitySnapshotV1,
};

use super::{AdapterError, CodexAdapter, NegotiatedMethods};

impl CodexAdapter {
    pub(crate) fn accept_initialize(
        &mut self,
        result: &Map<String, Value>,
    ) -> Result<CapabilitySnapshotV1, AdapterError> {
        let version = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .filter(|value| *value == "codex-app-server-c42ea41")
            .ok_or(AdapterError::CapabilityUnavailable)?;
        let advertised = result
            .get("capabilities")
            .and_then(Value::as_object)
            .ok_or(AdapterError::CapabilityUnavailable)?;
        let flag = |name: &str| {
            advertised
                .get(name)
                .and_then(Value::as_bool)
                .ok_or(AdapterError::CapabilityUnavailable)
        };
        let snapshot = CapabilitySnapshotV1 {
            schema_version: SchemaVersionV1,
            protocol_version: RequiredText::new("protocolVersion", version)?,
            new_session: flag("threadStart")?,
            resume_session: flag("threadResume")?,
            load_session: flag("threadRead")?,
            close_session: flag("threadUnsubscribe")?,
            prompt: flag("turnStart")?,
            cancel: flag("turnInterrupt")?,
            native_session_fork: false,
            active_turn_steer: false,
            child_conversation_projection: flag("childConversationProjection")?,
            child_transcript_read: false,
            collaboration_event_projection: flag("collaborationEventProjection")?,
            multi_root_extension_schema_id: None,
            model_count: 0,
            config_count: 0,
        };
        self.negotiated = Some(NegotiatedMethods {
            snapshot: snapshot.clone(),
            models: flag("modelList")?,
        });
        Ok(snapshot)
    }
}
