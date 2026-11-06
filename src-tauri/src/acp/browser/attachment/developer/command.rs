//! A bounded command lease keeps its original grant through attachment handoff.
//! Only a tab with another live grant can hand off; losing the last grant,
//! explicit revocation, navigation or sleep still ends native work.
use super::*;

pub(crate) struct DeveloperCommand {
    lease: AttachmentLease,
    grant: Arc<()>,
    session: String,
    origin: String,
}

impl AttachmentLease {
    pub(crate) fn developer_command(
        &self,
        session: &str,
        origin: &str,
    ) -> Option<DeveloperCommand> {
        let mut tab = self.developer.tab.lock().ok()?;
        let grant = tab.grants.get_mut(&self.developer.key())?;
        if !grant.is_live() || grant.session != session || grant.origin != origin {
            return None;
        }
        grant.commands = grant.commands.checked_add(1)?;
        Some(DeveloperCommand {
            lease: self.clone(),
            grant: grant.identity.clone(),
            session: session.into(),
            origin: origin.into(),
        })
    }

    /// Detach/replacement stops ordinary admission immediately. If this tab
    /// has a successor grant, the admitted developer command keeps its lease
    /// until its owned result has passed the gate's final permission check.
    pub(in crate::acp::browser::attachment) fn stop_or_hand_off(&self) {
        let handing_off = self.authority.lock().ok().is_some_and(|slot| {
            let Ok(mut tab) = self.developer.tab.lock() else {
                return false;
            };
            if !tab
                .grants
                .iter()
                .any(|(key, grant)| *key != self.developer.key() && grant.is_live())
            {
                return false;
            }
            let Some(grant) = tab.grants.get_mut(&self.developer.key()) else {
                return false;
            };
            if grant.commands == 0 {
                return false;
            }
            grant.draining = true;
            self.retiring.store(true, Ordering::Release);
            drop(tab);
            if let Some((_, authority)) = slot.as_ref() {
                authority.hand_off();
            }
            true
        });
        if !handing_off {
            self.stop();
        }
    }
}

impl DeveloperCommand {
    pub(crate) fn is_live(&self, session: &str, origin: &str) -> bool {
        if self.session != session || self.origin != origin {
            return false;
        }
        self.lease.developer.tab.lock().is_ok_and(|tab| {
            tab.grants
                .get(&self.lease.developer.key())
                .is_some_and(|grant| {
                    Arc::ptr_eq(&grant.identity, &self.grant)
                        && !grant.revoked
                        && grant.session == session
                        && grant.origin == origin
                        && (grant.is_live()
                            || (grant.draining && tab.grants.values().any(DeveloperGrant::is_live)))
                })
        })
    }
}

impl Drop for DeveloperCommand {
    fn drop(&mut self) {
        let stop = self.lease.developer.tab.lock().ok().is_some_and(|mut tab| {
            let Some(grant) = tab.grants.get_mut(&self.lease.developer.key()) else {
                return false;
            };
            if !Arc::ptr_eq(&grant.identity, &self.grant) {
                return false;
            }
            grant.commands -= 1;
            grant.draining && grant.commands == 0
        });
        if stop {
            self.lease.stop();
        }
    }
}
