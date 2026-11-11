use crate::acp::{
    contract::{RequiredText, SchemaVersionV1},
    intent::CapabilitySnapshotV1,
};

pub(super) fn capabilities(model_count: u16, config_count: u16) -> CapabilitySnapshotV1 {
    CapabilitySnapshotV1 {
        prompt_resources: true,
        prompt_images: false,
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new(
            "protocolVersion",
            format!("pi-rpc-{}", crate::acp::bindings::pi::PI_VERSION),
        )
        .expect("fixed protocol version"),
        new_session: true,
        resume_session: true,
        load_session: false,
        close_session: false,
        prompt: true,
        cancel: true,
        native_session_fork: false,
        native_session_compact: true,
        native_review: false,
        active_turn_steer: true,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count,
        config_count,
    }
}
