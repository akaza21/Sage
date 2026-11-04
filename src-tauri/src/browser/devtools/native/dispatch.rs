//! Worker dispatch and scoped developer-session ownership through the owned result.
use super::*;

impl BrowserCapabilityDispatch for &mut NativeDevtools {
    fn dispatch(
        &mut self,
        request: &BrowserRequest,
        permit: BrowserDispatchPermit,
    ) -> Result<PendingBrowserResult, BrowserFailure> {
        let mut capability = (**self).clone();
        let developer = capability.port_mut().transport.developer.clone();
        let timeout_permit = permit.clone();
        let issued = Arc::new(AtomicBool::new(false));
        capability.port_mut().transport.issued = issued.clone();
        capability.port_mut().transport.permit = Some(permit);
        let request = request.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _ = sender.send(capability.perform(&request));
        });
        Ok(Box::new(move || {
            let deadline = Instant::now() + Duration::from_secs(35);
            loop {
                if !timeout_permit.is_live() && !timeout_permit.was_invalidated_by_navigation() {
                    if let Some(session) = &developer {
                        session.close(true);
                    }
                    return Err(timeout_permit.revoked_failure());
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    if let Some(session) = &developer {
                        session.close(true);
                    } else {
                        timeout_permit.revoke();
                    }
                    return Err(dispatch_timeout_failure(issued.load(Ordering::Acquire)));
                }
                match receiver.recv_timeout(remaining.min(Duration::from_millis(25))) {
                    Ok(result) => return result,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        if let Some(session) = &developer {
                            session.close(true);
                        }
                        return Err(dispatch_disconnected_failure(
                            issued.load(Ordering::Acquire),
                        ));
                    }
                }
            }
        }))
    }
}

impl DevtoolsCapability<NativePort> {
    pub(crate) fn dispatch_developer(
        &mut self,
        request: &BrowserRequest,
        permit: BrowserDispatchPermit,
        scope: crate::acp::browser::attachment::DeveloperScope,
    ) -> Result<PendingBrowserResult, BrowserFailure> {
        let work = super::super::developer_session::Session::acquire(
            &self.port_mut().transport.webview,
            scope,
            &permit,
        )?;
        let mut capability = self.clone();
        capability.port_mut().transport.developer = Some(work.0.clone());
        let pending = (&mut capability).dispatch(request, permit)?;
        Ok(Box::new(move || {
            let _work = work;
            pending()
        }))
    }
}
