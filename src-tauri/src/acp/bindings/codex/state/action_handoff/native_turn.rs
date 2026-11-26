//! A Turn Codex starts on its own while Sage runs none on the bound thread,
//! such as an active goal's continuation after `thread/resume` or after a
//! Turn ends. It is announced to the runtime, which opens it as a
//! harness-started Turn and adopts it here under that Turn's client message
//! id, so its items, end, Stop and errors take the prompted Turn's path.
//! Codex has no decline: while a Turn is announced no prompt is admitted, so
//! a Turn Sage refuses ends with the runtime, never bound to a prompt.
//!
//! Correlation is left to Codex. A `turn/started` while a Sage `turn/start`
//! is unanswered stays with that request: Codex answers a `turn/start` sent
//! during its own Turn with that Turn's id (it takes the prompt into it), and
//! a reply naming another Turn stays an identity conflict.
//!
//! Codex keeps the policy a Turn started with: a `turn/start` it takes into
//! a running Turn changes only later Turns (recorded with codex-cli 0.167.1:
//! a read-only prompt joined to a goal continuation started under
//! workspace-write still wrote to the workspace). So the rule the
//! coordinator applies to an announced Turn applies to a joined prompt too:
//! it may run only read-only or at exactly its own access. Otherwise the
//! `turn/started` that arrives before Codex answers is refused, and since
//! Codex cannot decline it, the runtime stops and the prompt's Turn fails.
//! Sage's own Turn has always been answered before its `turn/started`
//! arrives; were that order ever reversed, this fails closed. The check
//! assumes the reverse for a raced Turn: Codex sends its own Turn's
//! `turn/started` before it answers the `turn/start` it took in (as
//! recorded). A Codex that answered first would bind the prompt with no
//! check, so the live access-race probe must be re-run on Codex upgrades.
//!
//! A session action races the same way (recorded with codex-cli 0.167.1):
//! `thread/compact/start` or `review/start` sent as Codex starts its own
//! Turn gets that Turn's `turn/started` first, then the answer, then Codex
//! interrupts its Turn and runs the action in a Turn of its own. Until the
//! answer, that `turn/started` would bind to the action, so the same rule
//! holds against the access the chat had when the action was sent. Once
//! answered, a compact asks nothing: it answers `{}` before its own
//! `turn/started`, which must bind. Neither action sets the policy.
//!
//! An answered review runs under the thread's access (recorded with
//! codex-cli 0.167.1): `review/start` takes no policy, and on a thread last
//! prompted under workspace-write the reviewer ran under workspace-write
//! even when the request carried a read-only policy. So a review that rule
//! would refuse is refused before its Turn is prepared, and `review/start`
//! is never sent. The outcome is already known then: no `turn/start` is
//! unanswered, so the thread's access cannot change before the review is
//! answered. For the same reason a raced `turn/started` is never refused
//! against a sent review; that refusal still guards prompts and compacts.
//!
//! Two limits remain. A goal `turn/started` that arrives after a compact's
//! `{}` answer binds to the compact unchecked: the compaction's own
//! `turn/started` and a goal continuation's carry the same fields (a new id
//! that is its own root, no items), and only the later `contextCompaction`
//! item tells them apart. With a goal set right after that answer, Codex
//! ran the compaction first and the goal only after it ended. And a review
//! raced with Codex's own Turn is answered with a new Turn, not the raced
//! one, so when the raced `turn/started` is not refused it binds to the
//! review and the answer stays an identity conflict.
use sage_acp_protocol::{
    access_mode::SessionAccessMode, contract::RequiredText, intent::ExternalId,
};

use crate::acp::{
    runtime::REVIEW_ACCESS_REFUSED, session_action::SessionAction, transcript_quote::Prompt,
};

use super::super::{AdapterError, CodexAdapter, ExternalKind, RequestKind, ScopedExternalId};

#[derive(Clone, Debug, PartialEq)]
pub(in crate::acp::bindings::codex) struct NativeTurn {
    announced: Option<ExternalId>,
    /// `thread/start` and `thread/resume` leave the thread read-only; each
    /// prompt whose `turn/start` Codex accepts sets the policy the thread's
    /// later Turns keep.
    thread_access: SessionAccessMode,
    /// A sent `turn/start` or session action and the chat's access when it
    /// was sent, until Codex answers.
    pending_access: Option<(u64, SessionAccessMode)>,
}

impl Default for NativeTurn {
    fn default() -> Self {
        Self {
            announced: None,
            thread_access: SessionAccessMode::ReadOnly,
            pending_access: None,
        }
    }
}

impl CodexAdapter {
    /// The session identity that announces `turn`, when no Sage request
    /// started, awaits or ended it and no Turn runs. `None` leaves the
    /// `turn/started` to correlation, which rejects what it cannot place.
    pub(crate) fn announce_native_turn(
        &mut self,
        turn: &str,
    ) -> Result<Option<ScopedExternalId>, AdapterError> {
        let id = ExternalId::new("externalTurnId", turn.to_owned())?;
        if self.active_turn_id.is_some()
            || self.provisional_turn.is_some()
            || self.action_handoff.is_some()
            || self.turn_start_correlations.has_pending()
            || self.turn_start_correlations.knows(&id)
            || self
                .previous_turn
                .as_ref()
                .is_some_and(|previous| previous.id == id)
        {
            return Ok(None);
        }
        let thread = self
            .external_thread_id
            .clone()
            .ok_or(AdapterError::SessionUnavailable)?;
        if self.native_turn.announced.replace(id).is_some() {
            return Err(AdapterError::IdentityConflict);
        }
        Ok(Some(self.scoped(ExternalKind::Session, thread)))
    }

    pub(crate) fn has_announced_native_turn(&self) -> bool {
        self.native_turn.announced.is_some()
    }

    /// The access the announced Turn runs under.
    pub(crate) fn native_turn_access(&self) -> Option<SessionAccessMode> {
        self.native_turn
            .announced
            .as_ref()
            .map(|_| self.native_turn.thread_access)
    }

    pub(in crate::acp::bindings::codex::state) fn ask_thread_access(
        &mut self,
        request_id: u64,
        access: SessionAccessMode,
    ) {
        self.native_turn.pending_access = Some((request_id, access));
    }

    /// Whether a Turn Codex starts now, binding to the unanswered prompt or
    /// action, would run it under access other than it asked for: the
    /// Turn keeps the thread's access unless that is read-only.
    pub(in crate::acp::bindings::codex::state) fn joined_turn_overrides_access(&self) -> bool {
        self.native_turn
            .pending_access
            .is_some_and(|(request_id, asked)| {
                self.pending_kind(request_id).is_some()
                    && self.turn_start_correlations.is_pending(request_id)
                    && self.thread_access_overrides(asked)
            })
    }

    /// Whether a Turn under the thread's access runs wider than `asked`.
    fn thread_access_overrides(&self, asked: SessionAccessMode) -> bool {
        self.native_turn.thread_access != SessionAccessMode::ReadOnly
            && self.native_turn.thread_access != asked
    }

    /// The refusal of `prompt` asked under `access`, before its Turn is
    /// prepared: a review whose reviewer would run under other access.
    pub(crate) fn prompt_refusal(
        &self,
        prompt: &Prompt,
        access: SessionAccessMode,
    ) -> Option<&'static str> {
        (matches!(prompt.action, Some(SessionAction::Review { .. }))
            && self.thread_access_overrides(access))
        .then_some(REVIEW_ACCESS_REFUSED)
    }

    /// Codex answered `request_id`. A `turn/start` it accepted sets the
    /// policy later Turns keep; an action leaves the policy as it was.
    pub(crate) fn accept_thread_access(&mut self, request_id: u64) {
        if let Some((pending, access)) = self.native_turn.pending_access
            && pending == request_id
        {
            if self.pending_kind(request_id) == Some(RequestKind::TurnStart) {
                self.native_turn.thread_access = access;
            }
            self.native_turn.pending_access = None;
        }
    }

    /// Makes the announced Turn the active one, owned by the Turn Sage
    /// opened for it.
    pub(crate) fn adopt_native_turn(
        &mut self,
        client_message_id: RequiredText<512>,
    ) -> Result<ScopedExternalId, AdapterError> {
        let id = self
            .native_turn
            .announced
            .take()
            .ok_or(AdapterError::TurnIntentConflict)?;
        if self.active_turn_id.is_some() || self.provisional_turn.is_some() {
            return Err(AdapterError::TurnIntentConflict);
        }
        self.active_turn_client_message_id = Some(client_message_id);
        self.active_item_terminals.clear();
        self.active_unretried_error_fingerprints.clear();
        self.active_terminal_id_bytes = 0;
        self.active_turn_id = Some(id.clone());
        self.scoped_turn(id.as_str())
    }
}
