//! A native completion owns only a weak lease. Invoke and disposal both
//! acknowledge once, so a stalled COM handler cannot pin a WebView or session.
use super::*;

pub(in crate::browser::devtools) trait CompletionTarget:
    Sized
{
    fn complete(self: &Arc<Self>);
}

pub(in crate::browser::devtools) struct Completion<T: CompletionTarget = Session> {
    owner: Option<Weak<T>>,
}

impl<T: CompletionTarget> Completion<T> {
    pub(super) fn new(owner: &Arc<T>) -> Self {
        Self {
            owner: Some(Arc::downgrade(owner)),
        }
    }

    pub(in crate::browser::devtools) fn acknowledge(&mut self) {
        if let Some(owner) = self.owner.take().and_then(|owner| owner.upgrade()) {
            owner.complete();
        }
    }
}

impl<T: CompletionTarget> Drop for Completion<T> {
    fn drop(&mut self) {
        self.acknowledge();
    }
}

#[cfg(test)]
mod tests;
