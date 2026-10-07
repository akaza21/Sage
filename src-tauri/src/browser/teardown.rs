use super::*;

pub(super) const UNCERTAIN_CLOSE_PREFIX: &str =
    "Browser is still closing; restart Sage before browsing again";

#[derive(Clone, Default)]
pub(super) struct TeardownQuarantine {
    pending: std::sync::Arc<Mutex<BTreeMap<String, bool>>>,
}

impl TeardownQuarantine {
    pub(super) fn begin(&self, label: &str) -> Result<(), String> {
        let mut pending = self.pending.lock().map_err(|error| error.to_string())?;
        if pending.len() > MAX_BROWSER_TABS && !pending.contains_key(label) {
            return Err(UNCERTAIN_CLOSE_PREFIX.into());
        }
        pending.entry(label.to_owned()).or_insert(false);
        Ok(())
    }

    pub(super) fn acknowledge(&self, label: &str, result: &Result<(), String>) {
        if let Ok(mut pending) = self.pending.lock() {
            if result.is_ok() {
                pending.remove(label);
            } else if let Some(failed) = pending.get_mut(label) {
                *failed = true;
            }
        }
    }

    pub(super) fn mark_failed(&self, label: &str) {
        if let Ok(mut pending) = self.pending.lock()
            && let Some(failed) = pending.get_mut(label)
        {
            *failed = true;
        }
    }

    pub(super) fn blocked_message(&self) -> Option<&'static str> {
        let Ok(pending) = self.pending.lock() else {
            return Some(UNCERTAIN_CLOSE_PREFIX);
        };
        if pending.values().any(|failed| *failed) {
            Some(UNCERTAIN_CLOSE_PREFIX)
        } else if pending.is_empty() {
            None
        } else {
            Some("Browser is still closing, try again")
        }
    }

    pub(super) fn clear_after_window_destroy(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.clear();
        }
    }
}
