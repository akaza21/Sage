use super::super::request::{NativeProjectRoot, initialized_notification};
use super::{
    AdapterError, CodexAdapter, ExternalId, ExternalKind, OutgoingRequest, RequestKind,
    ScopedExternalId,
};
use crate::acp::{contract::RequiredText, intent::ModelId};

impl CodexAdapter {
    /// The session identity for a notification about the bound thread itself
    /// rather than one of its Turns.
    pub(crate) fn thread_session_id(&self, value: &str) -> Result<ScopedExternalId, AdapterError> {
        self.expect_thread(value)?;
        let thread = self
            .external_thread_id
            .clone()
            .ok_or(AdapterError::ForeignIdentity)?;
        Ok(self.scoped(ExternalKind::Session, thread))
    }

    pub(crate) fn project_root(&self, value: String) -> Result<NativeProjectRoot, AdapterError> {
        NativeProjectRoot::new(self.platform, value).map_err(AdapterError::Request)
    }

    pub(crate) fn initialize(&mut self, id: u64) -> Result<OutgoingRequest, AdapterError> {
        if self.negotiated.is_some()
            || self
                .pending
                .iter()
                .any(|(_, kind)| *kind == RequestKind::Initialize)
        {
            return Err(AdapterError::AlreadyInitialized);
        }
        self.reserve(OutgoingRequest::initialize(id)?)
    }

    pub(crate) fn initialized(&self) -> Result<Vec<u8>, AdapterError> {
        self.require_method(|_| true)?;
        initialized_notification().map_err(AdapterError::Request)
    }

    pub(crate) fn start(
        &mut self,
        id: u64,
        root: &NativeProjectRoot,
        model: Option<&ModelId>,
    ) -> Result<OutgoingRequest, AdapterError> {
        self.require_method(|methods| methods.snapshot.new_session)?;
        let request = self.reserve(OutgoingRequest::start(id, root, model)?)?;
        self.native_root = Some(root.clone());
        Ok(request)
    }

    pub(crate) fn resume(
        &mut self,
        id: u64,
        external_thread_id: &ExternalId,
        root: &NativeProjectRoot,
        model: Option<&ModelId>,
    ) -> Result<OutgoingRequest, AdapterError> {
        self.require_method(|methods| methods.snapshot.resume_session)?;
        let request = self.reserve(OutgoingRequest::resume(
            id,
            external_thread_id,
            root,
            model,
        )?)?;
        self.native_root = Some(root.clone());
        Ok(request)
    }

    pub(crate) fn read(
        &mut self,
        id: u64,
        external_thread_id: &ExternalId,
    ) -> Result<OutgoingRequest, AdapterError> {
        self.require_method(|methods| methods.snapshot.load_session)?;
        self.reserve(OutgoingRequest::read(id, external_thread_id)?)
    }

    pub(crate) fn models(
        &mut self,
        id: u64,
        cursor: Option<&RequiredText<512>>,
    ) -> Result<OutgoingRequest, AdapterError> {
        self.require_method(|methods| methods.models)?;
        self.reserve(OutgoingRequest::models(id, cursor)?)
    }

    pub(crate) fn cancel(&mut self, id: u64) -> Result<OutgoingRequest, AdapterError> {
        self.require_method(|methods| methods.snapshot.cancel)?;
        let thread = self
            .external_thread_id
            .clone()
            .ok_or(AdapterError::SessionUnavailable)?;
        let turn = self
            .active_turn_id
            .clone()
            .ok_or(AdapterError::TurnUnavailable)?;
        self.reserve(OutgoingRequest::interrupt(id, &thread, &turn)?)
    }

    pub(crate) fn close(&mut self, id: u64) -> Result<OutgoingRequest, AdapterError> {
        self.require_method(|methods| methods.snapshot.close_session)?;
        let thread = self
            .external_thread_id
            .clone()
            .ok_or(AdapterError::SessionUnavailable)?;
        self.reserve(OutgoingRequest::unsubscribe(id, &thread)?)
    }
}
