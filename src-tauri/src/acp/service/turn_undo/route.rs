//! Which path an undo or a rewind takes. A session that previews file
//! rewinds restores files only through the checked seam: a dry run, the
//! conflict refusal, then stage, restore and cut. Other sessions undo as
//! their binding offers.
use super::{AcpService, SessionControlError, TurnUndo, file_rewind};
use crate::acp::{goal::NativeRpcReply, store::TurnUndoneCommand};
use sage_acp_protocol::{
    contract::{SafeText, SageTurnId},
    scheduler::RuntimeOwner,
    session_control::SessionControl,
};

pub(crate) const FILES_STAY: &str = "Last turn removed; files stay as they are.";
pub(crate) const FILES_NOT_RESTORED: &str =
    "This turn can't be undone without restoring its files, which Sage can't check. Files were not restored.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rewind {
    /// The latest turn; with `keep_files` only the conversation is cut.
    Undo { keep_files: bool },
    /// Everything from a turn on; with `files` the files go back too.
    Conversation { files: bool },
}

type Staged = (TurnUndoneCommand, Option<SafeText<512>>, bool);

impl AcpService {
    /// Stages and runs `mode`; the third value says whether files were
    /// restored. An undo whose turn the seam cannot check (older, never
    /// correlated, or compacted), or that keeps its files, cuts the
    /// conversation only: it never restores files unchecked.
    pub(super) async fn rewind_turn<E: std::fmt::Display>(
        &self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
        undo: TurnUndo,
        mode: Rewind,
        control: file_rewind::Control<'_>,
        resolve_cut: impl FnOnce() -> Result<Option<String>, E>,
    ) -> Result<Staged, SessionControlError> {
        let checked = file_rewind::offers_file_rewind(self.session_rewind(owner)?);
        let conversation_only = match mode {
            Rewind::Conversation { files: true } | Rewind::Undo { keep_files: true }
                if !checked =>
            {
                return Err(SessionControlError::Unavailable);
            }
            Rewind::Conversation { files: false } => true,
            Rewind::Undo { .. } if !checked => false,
            Rewind::Conversation { files: true } => {
                let target = self.rewind_target(owner, turn_id)?;
                return self
                    .checked(owner, turn_id, undo, true, &target, control, resolve_cut)
                    .await;
            }
            Rewind::Undo { keep_files } => {
                return match self.rewind_target(owner, turn_id) {
                    Ok(target) if !keep_files => {
                        self.checked(owner, turn_id, undo, false, &target, control, resolve_cut)
                            .await
                    }
                    _ => self.undo_keeping_files(owner, turn_id, undo, resolve_cut),
                };
            }
        };
        let (command, native) =
            self.prepare_rewind(owner, turn_id.clone(), undo, conversation_only, resolve_cut)?;
        match control(native) {
            Ok(NativeRpcReply::Controlled(summary)) => Ok((command, summary, false)),
            Ok(_) => Err(self.abandon_staged(owner, &command, SessionControlError::RecoveryRequired)),
            Err(error) => Err(self.abandon_staged(owner, &command, error)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn checked<E: std::fmt::Display>(
        &self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
        undo: TurnUndo,
        conversation_only: bool,
        target: &sage_acp_protocol::session_control::ConversationTarget,
        control: file_rewind::Control<'_>,
        resolve_cut: impl FnOnce() -> Result<Option<String>, E>,
    ) -> Result<Staged, SessionControlError> {
        let cut = SessionControl::RewindConversation {
            before: target.clone(),
        };
        self.rewind_files_then_cut(owner, target, control, || {
            self.prepare_rewind(owner, turn_id.clone(), undo, conversation_only, resolve_cut)
                .map(|(command, _)| (command, cut))
        })
        .await
    }

    /// The undo's cut, staged for the harness to apply when it resumes; no
    /// native control runs, so no file changes. A harness that must cut the
    /// thread itself has only its file-restoring undo, so it refuses.
    fn undo_keeping_files<E: std::fmt::Display>(
        &self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
        undo: TurnUndo,
        resolve_cut: impl FnOnce() -> Result<Option<String>, E>,
    ) -> Result<Staged, SessionControlError> {
        if undo.reverts_thread {
            return Err(file_rewind::refusal(FILES_NOT_RESTORED));
        }
        let (command, _) = self.prepare_rewind(owner, turn_id.clone(), undo, false, resolve_cut)?;
        Ok((command, SafeText::new("controlSummary", FILES_STAY).ok(), false))
    }
}
