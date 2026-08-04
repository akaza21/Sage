use crate::acp::{contract::SchemaVersionV1, intent::CapabilitySnapshotV1, limits::LIMITS};

use super::ActivationPumpError;

pub(super) fn intersect(
    curated: &CapabilitySnapshotV1,
    advertised: &CapabilitySnapshotV1,
    policy: &CapabilitySnapshotV1,
) -> Result<CapabilitySnapshotV1, ActivationPumpError> {
    if curated.schema_version != SchemaVersionV1
        || advertised.schema_version != SchemaVersionV1
        || policy.schema_version != SchemaVersionV1
        || curated.protocol_version != advertised.protocol_version
        || advertised.protocol_version != policy.protocol_version
        || advertised.protocol_version.as_str() == "pending"
    {
        return Err(ActivationPumpError::Intent);
    }
    let extension = match (
        &curated.multi_root_extension_schema_id,
        &advertised.multi_root_extension_schema_id,
        &policy.multi_root_extension_schema_id,
    ) {
        (Some(a), Some(b), Some(c)) if a == b && b == c => Some(a.clone()),
        _ => None,
    };
    let value = CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: advertised.protocol_version.clone(),
        new_session: curated.new_session && advertised.new_session && policy.new_session,
        resume_session: curated.resume_session
            && advertised.resume_session
            && policy.resume_session,
        load_session: curated.load_session && advertised.load_session && policy.load_session,
        close_session: curated.close_session && advertised.close_session && policy.close_session,
        prompt: curated.prompt && advertised.prompt && policy.prompt,
        cancel: curated.cancel && advertised.cancel && policy.cancel,
        native_session_fork: curated.native_session_fork
            && advertised.native_session_fork
            && policy.native_session_fork,
        active_turn_steer: curated.active_turn_steer
            && advertised.active_turn_steer
            && policy.active_turn_steer,
        child_conversation_projection: curated.child_conversation_projection
            && advertised.child_conversation_projection
            && policy.child_conversation_projection,
        child_transcript_read: curated.child_transcript_read
            && advertised.child_transcript_read
            && policy.child_transcript_read,
        collaboration_event_projection: curated.collaboration_event_projection
            && advertised.collaboration_event_projection
            && policy.collaboration_event_projection,
        multi_root_extension_schema_id: extension,
        model_count: advertised
            .model_count
            .min(curated.model_count)
            .min(policy.model_count),
        config_count: advertised
            .config_count
            .min(curated.config_count)
            .min(policy.config_count),
    };
    let bytes = serde_json::to_vec(&value).map_err(|_| ActivationPumpError::Capacity)?;
    if bytes.len() > LIMITS.wire.capability_metadata_bytes as usize {
        return Err(ActivationPumpError::Capacity);
    }
    Ok(value)
}
