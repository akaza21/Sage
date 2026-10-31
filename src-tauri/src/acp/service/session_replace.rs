use super::{AcpService, session_transition::*};
use crate::acp::{
    bindings::{
        continuation::{ContinuationCompatibility, classify},
        registry::{AdmittedBindingRecord, BindingProjectRoot},
    },
    lifecycle::{Residency, SessionState},
    scheduler::RuntimeOwner,
    store::{
        StoreError,
        session_replace::{self, ReplaceSessionCommand},
    },
};
use rusqlite::{OptionalExtension, params};
use std::time::{Duration, Instant};

#[cfg(all(test, windows))]
#[path = "session_replace_tests.rs"]
mod tests;

impl AcpService {
    pub(crate) fn replace_refused_resume(
        &self,
        activation: &crate::acp::store::ActivateSessionCommand,
    ) -> Result<Option<crate::acp::store::ActivateSessionCommand>, StoreError> {
        let refused: bool = self.database.connection().query_row(
            "SELECT EXISTS(SELECT 1 FROM session_runtime_epochs e JOIN chat_session_heads h ON h.session_id=e.session_id
             WHERE h.project_id=?1 AND h.chat_id=?2 AND e.session_id=?3 AND e.runtime_epoch=?4
             AND e.state='aborted' AND e.exit_kind='resumeRefused'
             AND e.runtime_epoch=(SELECT MAX(runtime_epoch) FROM session_runtime_epochs WHERE session_id=e.session_id))",
            params![activation.project_id.as_str(), activation.chat_id.as_str(), activation.session_id.as_str(), activation.runtime_epoch.get() as i64], |row| row.get(0),
        )?;
        if !refused {
            return Ok(None);
        }
        let page = self.load_chat_page(
            activation.project_id.clone(),
            activation.chat_id.clone(),
            None,
        )?;
        let session = page
            .sessions
            .as_slice()
            .iter()
            .find(|session| session.id == activation.session_id)
            .ok_or(StoreError::Corrupt("refused resume session"))?;
        let (binding_digest, harness_version): (String, Option<String>) =
            self.database.connection().query_row(
                "SELECT binding_digest,harness_version FROM agent_sessions WHERE id=?1",
                [activation.session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
        let intent = crate::acp::contract::ClientIntentId::parse(uuid::Uuid::new_v4().to_string())
            .map_err(|_| StoreError::Corrupt("resume handoff intent"))?;
        let now_millis = crate::persistence::now_millis();
        let command = ReplaceSessionCommand {
            expected_head: Some(activation.session_id.clone()),
            confirm_unknown_delivery: false,
            start: crate::acp::store::StartSessionCommand {
                project_id: activation.project_id.clone(),
                chat_id: activation.chat_id.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: intent.clone(),
                binding_id: session.binding_id.clone(),
                binding_digest,
                harness_version: harness_version
                    .map(|value| crate::acp::contract::SafeText::new("harnessVersion", value))
                    .transpose()
                    .map_err(|_| StoreError::Corrupt("harness version"))?,
                model_id: session.model_id.clone(),
                now_millis,
            },
        };
        self.transition_admitted_session(command.clone())?;
        let result = session_replace::lookup(&self.database.connection(), &command)?
            .ok_or(StoreError::Corrupt("resume handoff receipt"))?;
        Ok(Some(crate::acp::store::ActivateSessionCommand {
            project_id: activation.project_id.clone(),
            chat_id: activation.chat_id.clone(),
            expected_projection_revision: result.projection_revision,
            client_intent_id: intent,
            session_id: result.session_id,
            runtime_epoch: crate::acp::contract::RuntimeEpoch::new(1)
                .map_err(|_| StoreError::CounterExhausted)?,
            artifact_digest: String::new(),
            now_millis,
        }))
    }

    pub(crate) fn transition_session(
        &self,
        admitted: AdmittedBindingRecord<'_>,
        root: &BindingProjectRoot,
        mut command: ReplaceSessionCommand,
    ) -> Result<(), StoreError> {
        if !root.project_matches(&command.start.project_id) {
            return Err(StoreError::Conflict {
                resource: "projectRoot",
            });
        }
        self.admit_session_start(&admitted, root, &mut command.start)
            .map_err(|_| StoreError::Conflict {
                resource: "targetAdmission",
            })?;
        self.transition_admitted_session(command)
    }

    pub(crate) fn transition_admitted_session(
        &self,
        command: ReplaceSessionCommand,
    ) -> Result<(), StoreError> {
        if session_replace::lookup(&self.database.connection(), &command)?.is_some() {
            return Ok(());
        }
        let page = self.load_chat_page(
            command.start.project_id.clone(),
            command.start.chat_id.clone(),
            None,
        )?;
        if page.current_session_id != command.expected_head
            || page.projection_revision != command.start.expected_projection_revision
        {
            return Err(StoreError::Conflict {
                resource: "transitionHeadRevision",
            });
        }
        let current = page
            .sessions
            .as_slice()
            .iter()
            .find(|session| Some(&session.id) == page.current_session_id.as_ref());
        let old_identity: Option<(String,String,Option<String>,Option<String>)> = command.expected_head.as_ref().map(|head| self.database.connection().query_row(
            "SELECT binding_id,binding_digest,harness_version,external_session_id FROM agent_sessions WHERE id=?1 AND project_id=?2 AND chat_id=?3",
            params![head.as_str(),command.start.project_id.as_str(),command.start.chat_id.as_str()], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional()).transpose()?.flatten();
        let compatible = old_identity.as_ref().is_some_and(|identity| {
            classify(
                (&identity.0, &identity.1, identity.2.as_deref()),
                (
                    command.start.binding_id.as_str(),
                    &command.start.binding_digest,
                    command
                        .start
                        .harness_version
                        .as_ref()
                        .map_or("", |version| version.as_str()),
                ),
            ) == ContinuationCompatibility::Compatible
        });
        let selection = crate::acp::bindings::continuation::model_selection(
            current.and_then(|session| session.model_id.as_ref().map(|model| model.as_str())),
            command.start.model_id.as_ref().map(|model| model.as_str()),
        );
        let input = TransitionInput {
            available: true,
            valid_scope: true,
            stale: false,
            busy: page
                .turns
                .as_slice()
                .iter()
                .any(|turn| !turn.state.is_terminal())
                || current.is_some_and(|session| {
                    matches!(
                        session.state,
                        SessionState::Starting | SessionState::Recovering | SessionState::Closing
                    )
                }),
            has_current: current.is_some(),
            compatible,
            terminal: current.is_some_and(|session| session.state.is_terminal()),
            selection,
            cold: current.is_some_and(|session| session.residency == Residency::Cold),
            resume_identity: old_identity
                .as_ref()
                .is_some_and(|identity| identity.3.is_some()),
            resume_capability: current.is_some_and(|session| {
                session.capabilities.resume_session || session.capabilities.load_session
            }),
            resume_refused: command.expected_head.as_ref().map(|head| {
                self.database.connection().query_row(
                    "SELECT exit_kind='resumeRefused' FROM session_runtime_epochs WHERE session_id=?1 ORDER BY runtime_epoch DESC LIMIT 1",
                    [head.as_str()], |row| row.get::<_, Option<bool>>(0),
                ).optional().map(|value| value.flatten().unwrap_or(false))
            }).transpose()?.unwrap_or(false),
            ambiguous: crate::acp::store::context_handoff::latest_delivery_ambiguous(
                &self.database.connection(),
                command.start.project_id.as_str(),
                command.start.chat_id.as_str(),
            )?,
        };
        let decision = decide_transition(&input);
        if current.is_some_and(|session| session.state == SessionState::Created)
            && compatible
            && selection.unchanged
            && !input.ambiguous
        {
            return session_replace::retain_current(
                &mut *self
                    .database
                    .write_connection()
                    .map_err(|_| StoreError::Corrupt("storage recovery required"))?,
                &command,
            );
        }
        match decision {
            TransitionDecision::Reuse | TransitionDecision::Resume => {
                return session_replace::retain_current(
                    &mut *self
                        .database
                        .write_connection()
                        .map_err(|_| StoreError::Corrupt("storage recovery required"))?,
                    &command,
                );
            }
            TransitionDecision::ApplySelection | TransitionDecision::Reject => {
                return Err(StoreError::Conflict {
                    resource: "transitionSelection",
                });
            }
            TransitionDecision::RecoveryRequired if !command.confirm_unknown_delivery => {
                return Err(StoreError::Conflict {
                    resource: "handoffDeliveryUnknown",
                });
            }
            TransitionDecision::RecoveryRequired | TransitionDecision::Fresh => {}
        }
        {
            let mut connection = self
                .database
                .write_connection()
                .map_err(|_| StoreError::Corrupt("storage recovery required"))?;
            let transaction = connection.transaction()?;
            crate::acp::store::sessions::validate_chat_revision(
                &transaction,
                &command.start.project_id,
                &command.start.chat_id,
                command.start.expected_projection_revision,
            )?;
            session_replace::preflight(&transaction, &command)?;
            crate::acp::store::sessions::ensure_session_capacity(
                &transaction,
                &command.start.project_id,
                &command.start.chat_id,
            )?;
        }
        let mut quiesced_revision = None;
        if let Some(session) = current
            && let Some(epoch) = session
                .runtime_epoch
                .filter(|_| session.residency == Residency::Hot)
        {
            let owner = RuntimeOwner {
                project_id: command.start.project_id.clone(),
                chat_id: command.start.chat_id.clone(),
                session_id: session.id.clone(),
                runtime_epoch: epoch,
            };
            self.quiesce_transition_owner(&owner)?;
            quiesced_revision = Some(
                self.intent_snapshot(
                    command.start.project_id.clone(),
                    crate::acp::contract::ProjectGeneration::ZERO,
                    command.start.chat_id.clone(),
                )?
                .projection_revision,
            );
        }
        let mut connection = self
            .database
            .write_connection()
            .map_err(|_| StoreError::Corrupt("storage recovery required"))?;
        let project_id = command.start.project_id.clone();
        let chat_id = command.start.chat_id.clone();
        let mutation = if quiesced_revision.is_some() {
            session_replace::replace_session_at(&mut connection, command, quiesced_revision)?
        } else {
            session_replace::replace_session(&mut connection, command)?
        };
        drop(connection);
        match self
            .publisher
            .publish_committed(crate::acp::intent::ProjectionChangedV1 {
                project_id,
                chat_id,
                session_id: mutation.session_id,
                projection_revision: mutation.projection_revision,
                child_projection_epoch: 0,
            }) {
            Ok(
                crate::acp::runtime::ProjectionPublishStatus::Published
                | crate::acp::runtime::ProjectionPublishStatus::Coalesced,
            ) => Ok(()),
            _ => Err(StoreError::Corrupt(
                "transition projection recovery required",
            )),
        }
    }

    fn quiesce_transition_owner(&self, owner: &RuntimeOwner) -> Result<(), StoreError> {
        let lane = self.lane_for(owner).ok_or(StoreError::Conflict {
            resource: "transitionRuntime",
        })?;
        let host = lane
            .runtime
            .for_owner(owner)
            .map_err(|_| StoreError::Conflict {
                resource: "transitionRuntime",
            })?;
        if !host.has_shutdown_recovery() {
            return Err(StoreError::Conflict {
                resource: "busyRuntime",
            });
        }
        lane.scheduler
            .begin_idle_drain(owner)
            .map_err(|_| StoreError::Conflict {
                resource: "busyRuntime",
            })?;
        if lane.runtime.mark_recovery(owner).is_err() {
            let _ = lane.scheduler.cancel_idle_drain(owner);
            return Err(StoreError::Conflict {
                resource: "transitionRuntime",
            });
        }
        self.revoke_browser_chat(&owner.project_id, &owner.chat_id);
        let deadline = Instant::now()
            + Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close);
        if host.shutdown(deadline).is_err()
            || lane.scheduler.active().is_some()
            || lane.runtime.release_recovered(owner, &host).is_err()
        {
            self.retain_config_effect_recovery(owner.clone(), host);
            return Err(StoreError::Corrupt("transition runtime requires recovery"));
        }
        Ok(())
    }
}
