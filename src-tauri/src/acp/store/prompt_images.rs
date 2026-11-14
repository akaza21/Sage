//! The user's own images as durable transcript items. They sit right after the
//! prompt text, before any harness output, so a reloaded Chat shows the images
//! the user sent in the same place every other user attachment renders.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rusqlite::{Transaction, params};

use sage_acp_protocol::{
    contract::{SafeText, SageItemId, SageTurnId},
    intent::{ExternalId, ImageSource, NormalizedPayload},
    lifecycle::ItemLifecycle,
};
use sage_core::limits::LIMITS;

use super::{ItemIngressCommand, PrepareTurnCommand, StoreError, facts::hex_digest};

pub(super) fn insert_user_images(
    transaction: &Transaction<'_>,
    command: &PrepareTurnCommand,
    turn_id: &SageTurnId,
) -> Result<(), StoreError> {
    for image in command.prompt.images.0.iter() {
        let Some((media_type, bytes)) = image.preview() else {
            continue;
        };
        let payload = NormalizedPayload::Image {
            media_type,
            data: SafeText::new("imageData", STANDARD.encode(bytes))
                .map_err(|_| StoreError::PayloadTooLarge)?,
            source: ImageSource::User,
        };
        let json = serde_json::to_string(&payload)
            .map_err(|_| StoreError::Corrupt("user image serialization"))?;
        let bytes = i64::try_from(json.len()).map_err(|_| StoreError::PayloadTooLarge)?;
        if bytes > LIMITS.wire.normalized_item_bytes as i64 {
            return Err(StoreError::PayloadTooLarge);
        }
        let digest = hex_digest(json.as_bytes());
        // Usage and capacity reuse item ingress accounting; only the owner
        // fields of this command are read.
        let usage = ItemIngressCommand {
            project_id: command.project_id.clone(),
            chat_id: command.chat_id.clone(),
            session_id: command.session_id.clone(),
            turn_id: turn_id.clone(),
            runtime_epoch: command.runtime_epoch,
            external_item_id: ExternalId::new("externalItemId", "sage-user-image")
                .map_err(|_| StoreError::Corrupt("user image identity"))?,
            activity_id: None,
            payload_digest: digest.clone(),
            payload,
            lifecycle: ItemLifecycle::Completed,
            tool_state: None,
            now_millis: command.now_millis,
        };
        let ordinal = super::item_usage::ensure_capacity(transaction, &usage, 1, bytes)?;
        transaction.execute(
            "INSERT INTO items (id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,
             origin,activity_id,kind,lifecycle,tool_state,external_item_id,payload_json,payload_bytes,
             payload_digest,revision,created_at,updated_at,terminal_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,'primary',NULL,'image','completed',NULL,NULL,?8,?9,?10,0,?11,?11,?11)",
            params![
                SageItemId::new().as_str(),
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
                turn_id.as_str(),
                command.runtime_epoch.get() as i64,
                ordinal,
                json,
                bytes,
                digest,
                command.now_millis
            ],
        )?;
        super::item_usage::update(transaction, &usage, 1, bytes)?;
    }
    Ok(())
}
