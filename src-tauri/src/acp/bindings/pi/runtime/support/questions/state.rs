use std::collections::VecDeque;

use crate::acp::bindings::pi::translator::PendingUiRequest;

#[derive(Default)]
enum Phase {
    #[default]
    InTurn,
    AwaitingAdoption,
    IdleTurn,
    Cancelling {
        idle: bool,
    },
}

#[derive(Default)]
pub(in crate::acp::bindings::pi::runtime) struct Questions {
    pub(super) queued: VecDeque<PendingUiRequest>,
    phase: Phase,
    pub(in crate::acp::bindings::pi::runtime) reply_pending: bool,
    pub(in crate::acp::bindings::pi::runtime) visible: Option<String>,
    pub(in crate::acp::bindings::pi::runtime) settling: Option<String>,
    pub(super) cancelled: VecDeque<String>,
    pub(super) notices: u8,
}

impl Questions {
    pub(in crate::acp::bindings::pi::runtime) fn starting(&self) -> bool {
        matches!(self.phase, Phase::AwaitingAdoption)
    }
    pub(in crate::acp::bindings::pi::runtime) fn idle_turn(&self) -> bool {
        matches!(
            self.phase,
            Phase::IdleTurn | Phase::Cancelling { idle: true }
        )
    }
    pub(in crate::acp::bindings::pi::runtime) fn cancelling(&self) -> bool {
        matches!(self.phase, Phase::Cancelling { .. })
    }
    pub(super) fn start(&mut self) {
        self.phase = Phase::AwaitingAdoption;
    }
    pub(super) fn adopt(&mut self) {
        self.phase = Phase::IdleTurn;
    }
    pub(super) fn finish(&mut self) {
        self.phase = Phase::InTurn;
    }
    pub(in crate::acp::bindings::pi::runtime) fn cancel(&mut self) {
        self.phase = Phase::Cancelling {
            idle: self.idle_turn(),
        };
    }
}
