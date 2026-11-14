//! Bounded post-classification source selection.

use crate::acp::runtime::read_text_file::read_disk_text_file;

use super::*;

impl ReadTextFileService {
    pub(super) fn read_acknowledged(
        &self,
        pending: &PendingRead,
        ack: &ReadTextFlushAck,
    ) -> ReadTextResult<ReadTextCompletion> {
        // Reconstruct authority from the current complete root set before
        // either disk or persisted-buffer selection. This rejects target/root
        // replacement and newly ambiguous nested roots after the renderer
        // barrier, before an editor buffer can be consulted.
        if let Some(admission) = &pending.admission {
            self.mapper_for_owner(&pending.owner)?
                .revalidate_admission(admission)
                .map_err(|_| {
                    Box::new(ReadTextFailure::for_resource(
                        ReadTextFailureKind::Forbidden,
                        &pending.resource,
                        pending.range,
                    ))
                })?;
        }
        // The root identity is rechecked after the renderer barrier, before
        // both disk and durable-buffer sources. A replaced root loses authority.
        let current_workspace = self.resolve_workspace(&pending.owner, &pending.resource)?;
        if current_workspace.root() != pending.workspace.root()
            || current_workspace.root_identity() != pending.workspace.root_identity()
        {
            return Err(Box::new(ReadTextFailure::for_resource(
                ReadTextFailureKind::Forbidden,
                &pending.resource,
                pending.range,
            )));
        }
        pending.workspace.ensure_root_identity().map_err(|_| {
            Box::new(ReadTextFailure::for_resource(
                ReadTextFailureKind::Forbidden,
                &pending.resource,
                pending.range,
            ))
        })?;
        let durable_revision = self
            .database
            .editor_buffer_revision(pending.workspace.project_id(), &pending.resource)
            .map_err(|_| {
                Box::new(ReadTextFailure::for_resource(
                    ReadTextFailureKind::Corrupt,
                    &pending.resource,
                    pending.range,
                ))
            })?;
        let (content, source) = match ack.state {
            ReadTextFlushState::DiskAllowed {
                state: DiskAllowedState::NotOpen,
                editor_revision: None,
            } if pending.expected_editor_revision.is_none() && durable_revision.is_none() => {
                let result = read_disk_text_file(
                    &pending.workspace,
                    &pending.resource,
                    pending.range,
                    pending.admission.as_ref(),
                )
                .map_err(|_| {
                    Box::new(ReadTextFailure::for_resource(
                        ReadTextFailureKind::Forbidden,
                        &pending.resource,
                        pending.range,
                    ))
                })?;
                if self
                    .database
                    .editor_buffer_revision(pending.workspace.project_id(), &pending.resource)
                    .map_err(|_| {
                        Box::new(ReadTextFailure::for_resource(
                            ReadTextFailureKind::Corrupt,
                            &pending.resource,
                            pending.range,
                        ))
                    })?
                    .is_some()
                {
                    return Err(Box::new(ReadTextFailure::for_resource(
                        ReadTextFailureKind::Corrupt,
                        &pending.resource,
                        pending.range,
                    )));
                }
                (result.content, "disk")
            }
            // OpenClean is a bounded identity reservation: it is accepted only
            // for an active clean editor (nonzero identity) while no durable
            // dirty identity existed at begin or acknowledgement.
            ReadTextFlushState::DiskAllowed {
                state: DiskAllowedState::OpenClean,
                editor_revision: Some(revision),
            } if revision != 0
                && pending.expected_editor_revision.is_none()
                && durable_revision.is_none() =>
            {
                let result = read_disk_text_file(
                    &pending.workspace,
                    &pending.resource,
                    pending.range,
                    pending.admission.as_ref(),
                )
                .map_err(|_| {
                    Box::new(ReadTextFailure::for_resource(
                        ReadTextFailureKind::Forbidden,
                        &pending.resource,
                        pending.range,
                    ))
                })?;
                if self
                    .database
                    .editor_buffer_revision(pending.workspace.project_id(), &pending.resource)
                    .map_err(|_| {
                        Box::new(ReadTextFailure::for_resource(
                            ReadTextFailureKind::Corrupt,
                            &pending.resource,
                            pending.range,
                        ))
                    })?
                    .is_some()
                {
                    return Err(Box::new(ReadTextFailure::for_resource(
                        ReadTextFailureKind::Corrupt,
                        &pending.resource,
                        pending.range,
                    )));
                }
                (result.content, "disk")
            }
            ReadTextFlushState::Persisted {
                editor_revision,
                durable_revision: acknowledged_revision,
            } if editor_revision != 0
                && editor_revision == acknowledged_revision
                && durable_revision == Some(editor_revision) =>
            {
                let buffer = self
                    .database
                    .read_editor_buffer_bounded(
                        pending.workspace.project_id(),
                        &pending.resource,
                        editor_revision,
                        MAX_EDITABLE_BUFFER_BYTES,
                    )
                    .map_err(|_| {
                        Box::new(ReadTextFailure::for_resource(
                            ReadTextFailureKind::Corrupt,
                            &pending.resource,
                            pending.range,
                        ))
                    })?;
                let content = select_text_lines(&buffer, pending.range.line, pending.range.limit)
                    .map_err(|_| {
                    Box::new(ReadTextFailure::for_resource(
                        ReadTextFailureKind::Forbidden,
                        &pending.resource,
                        pending.range,
                    ))
                })?;
                (content, "buffer")
            }
            _ => {
                return Err(Box::new(ReadTextFailure::for_resource(
                    ReadTextFailureKind::Corrupt,
                    &pending.resource,
                    pending.range,
                )));
            }
        };
        Ok(ReadTextCompletion {
            receipt: terminal::receipt(
                Some(&pending.resource),
                pending.range,
                source,
                "success",
                content.len(),
            ),
            content,
        })
    }
}
