// Derived from pingdotgg/t3code (commit 08a463e), Copyright (c) 2026 T3 Tools Inc., MIT; see THIRD-PARTY-NOTICES.md.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SelectionChange {
    pub unchanged: bool,
    pub apply_next_turn: bool,
    pub requires_restart: bool,
    pub fresh: bool,
    pub classified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransitionDecision {
    Reuse,
    ApplySelection,
    Resume,
    Fresh,
    Reject,
    RecoveryRequired,
}

pub(crate) struct TransitionInput {
    pub available: bool,
    pub valid_scope: bool,
    pub stale: bool,
    pub busy: bool,
    pub has_current: bool,
    pub compatible: bool,
    pub terminal: bool,
    pub selection: SelectionChange,
    pub cold: bool,
    pub resume_identity: bool,
    pub resume_capability: bool,
    pub resume_refused: bool,
    pub ambiguous: bool,
}

impl TransitionInput {
    #[cfg(test)]
    pub(crate) fn live() -> Self {
        Self {
            available: true,
            valid_scope: true,
            stale: false,
            busy: false,
            has_current: true,
            compatible: true,
            terminal: false,
            selection: SelectionChange {
                unchanged: true,
                classified: true,
                ..Default::default()
            },
            cold: false,
            resume_identity: true,
            resume_capability: true,
            resume_refused: false,
            ambiguous: false,
        }
    }
}

pub(crate) fn decide_transition(input: &TransitionInput) -> TransitionDecision {
    if !input.available || !input.valid_scope || input.stale || input.busy {
        return TransitionDecision::Reject;
    }
    if input.ambiguous {
        return TransitionDecision::RecoveryRequired;
    }
    if !input.has_current || !input.compatible || input.terminal || input.selection.fresh {
        return TransitionDecision::Fresh;
    }
    if !input.selection.classified {
        return TransitionDecision::Reject;
    }
    if input.cold || input.selection.requires_restart {
        return if input.resume_identity && input.resume_capability && !input.resume_refused {
            TransitionDecision::Resume
        } else {
            TransitionDecision::Fresh
        };
    }
    if input.selection.unchanged {
        TransitionDecision::Reuse
    } else if input.selection.apply_next_turn {
        TransitionDecision::ApplySelection
    } else {
        TransitionDecision::Reject
    }
}
