use super::{AdapterRuntimeError, CommandState};

impl CommandState {
    pub(crate) fn refresh_after_turn(&self) {
        if let Some(list) = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
        {
            list.refresh_after_turn = true;
        }
    }
    pub(crate) fn turn_completed(&self) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(list) = state.as_mut()
            && std::mem::take(&mut list.refresh_after_turn)
        {
            list.refresh_requested = true;
        }
    }
    /// One wanted bit and one outstanding read per session; repeated intents coalesce.
    pub(crate) fn request_refresh(&self) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(list) = state.as_mut().filter(|list| !list.pending) {
            list.refresh_requested = true;
        }
    }

    pub(crate) fn refresh_pending(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_some_and(|list| list.pending)
    }

    pub(crate) fn refresh_requested(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_some_and(|list| list.refresh_requested && !list.pending)
    }

    pub(crate) fn begin_refresh(&self) -> Result<bool, AdapterRuntimeError> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let list = state.as_mut().ok_or(AdapterRuntimeError::Operation)?;
        if list.pending {
            return Ok(false);
        }
        list.next_revision()?;
        list.refresh_requested = false;
        list.pending = true;
        Ok(true)
    }

    pub(crate) fn finish_refresh(&self) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(list) = state
            .as_mut()
            .filter(|list| list.pending || list.refresh_requested)
        {
            list.pending = false;
            list.refresh_requested = false;
            let _ = list.next_revision();
        }
    }
}
