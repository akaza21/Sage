use crate::acp::{
    bindings::adapter::{AdapterBatch, AdapterRuntimeError, HarnessAdapter, ResourceMapper},
    transport::Frame,
};

use super::ActivatedSession;

pub(crate) struct IdleActivationParts<A, J, R> {
    pub(super) adapter: A,
    pub(super) journal: J,
    pub(super) resources: R,
    pub(super) session: ActivatedSession,
}

pub(crate) struct RecoveryActivationParts<A, J, R> {
    pub(crate) adapter: A,
    pub(crate) journal: J,
    pub(crate) resources: R,
}

impl<A, J, R> RecoveryActivationParts<A, J, R> {
    pub(crate) fn into_parts(self) -> (A, J, R) {
        (self.adapter, self.journal, self.resources)
    }
}

pub(crate) struct TeardownActivationParts<A, J, R> {
    pub(crate) adapter: A,
    pub(crate) journal: J,
    pub(crate) resources: R,
}

impl<A, J, R> TeardownActivationParts<A, J, R> {
    pub(crate) fn into_parts(self) -> (A, J, R) {
        (self.adapter, self.journal, self.resources)
    }
}

impl<A, J, R> IdleActivationParts<A, J, R> {
    pub(crate) fn new(adapter: A, journal: J, resources: R, session: ActivatedSession) -> Self {
        Self {
            adapter,
            journal,
            resources,
            session,
        }
    }

    pub(crate) fn session(&self) -> &ActivatedSession {
        &self.session
    }

    pub(crate) fn into_parts(self) -> (A, J, R, ActivatedSession) {
        (self.adapter, self.journal, self.resources, self.session)
    }

    pub(crate) fn journal(&self) -> &J {
        &self.journal
    }
}

impl<A: HarnessAdapter, J, R: ResourceMapper> IdleActivationParts<A, J, R> {
    pub(crate) fn ingest_idle_frame(
        &mut self,
        frame: &Frame,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        self.adapter.ingest(frame, &self.resources)
    }
}
