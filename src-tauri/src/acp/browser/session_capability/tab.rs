use super::*;

pub(crate) struct AttachedTabPort<'a> {
    pub(super) lanes: &'a mut AttachmentLanes<NativeDevtools>,
    pub(super) browser: &'a BrowserState,
    pub(super) lease: AttachmentLease,
}

impl attached::AttachedTabFactory<AppHandle> for AttachmentLanes<NativeDevtools> {
    type Port<'a> = AttachedTabPort<'a>;

    fn port<'a>(&'a mut self, app: &'a AppHandle, lease: AttachmentLease) -> Self::Port<'a> {
        AttachedTabPort {
            lanes: self,
            browser: app.state::<BrowserState>().inner(),
            lease,
        }
    }
}

impl BrowserCapabilityDispatch for AttachedTabPort<'_> {
    fn dispatch(
        &mut self,
        request: &BrowserRequest,
        permit: BrowserDispatchPermit,
    ) -> Result<PendingBrowserResult, BrowserFailure> {
        if !permit.is_live() {
            return Err(permit.revoked_failure());
        }
        let generation = self.lease.resource_generation();
        let mut native = self
            .lanes
            .get_or_try_insert(&request.scope, &self.lease, || {
                NativeDevtools::for_attachment(self.browser, request.scope.clone(), &self.lease, generation, &permit)
            })?;
        native.wake_diagnostics(&self.lease, &permit)?;
        native.dispatch(request, permit)
    }
}
