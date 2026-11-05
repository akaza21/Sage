//! Grant admission, single-session reuse and binding share this native-free seam.
use super::*;

pub(in crate::browser) trait TabSession: Send + Sync + 'static {
    fn reserve(&self) -> Result<(), BrowserFailure>;
    fn close(self: &Arc<Self>, force: bool);
}

pub(in crate::browser) struct Slot<T = Session>(Arc<SharedSlot<T>>);
pub(super) type SharedSlot<T = Session> = Mutex<Option<Arc<T>>>;

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self(Arc::default())
    }
}

impl<T> Clone for Slot<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: TabSession> Slot<T> {
    pub(super) fn acquire(
        &self,
        label: &str,
        grant: &DeveloperScope,
        open: impl FnOnce(Weak<SharedSlot<T>>) -> T,
    ) -> Result<(Arc<T>, bool), BrowserFailure> {
        let (session, opening) = grant.with_live(label, || {
            let mut current = self.0.lock().map_err(|_| BrowserFailure::Unavailable)?;
            if let Some(session) = current.as_ref() {
                session.reserve()?;
                return Ok((session.clone(), false));
            }
            let session = Arc::new(open(Arc::downgrade(&self.0)));
            *current = Some(session.clone());
            Ok((session, true))
        })?;
        if opening {
            let weak = Arc::downgrade(&session);
            if let Err(error) = grant.bind(Arc::new(move |force| {
                if let Some(session) = weak.upgrade() {
                    session.close(force);
                }
            })) {
                if let Ok(mut current) = self.0.lock()
                    && current
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, &session))
                {
                    current.take();
                }
                return Err(error);
            }
        }
        Ok((session, opening))
    }
}
