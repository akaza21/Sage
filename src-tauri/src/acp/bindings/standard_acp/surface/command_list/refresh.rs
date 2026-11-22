use super::CommandState;

impl CommandState {
    /// Whether this session has published a catalog (revision 0 means never).
    pub(crate) fn advertised(&self) -> bool {
        self.revision().is_some_and(|revision| revision > 0)
    }
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
    /// Coalesce intents until the idle discovery pump consumes them. Outstanding
    /// wire requests are accounted for by each binding's existing request table.
    pub(crate) fn request_refresh(&self) {
        if let Some(list) = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
        {
            list.refresh_requested = true;
        }
    }

    pub(crate) fn take_refresh_request(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
            .is_some_and(|list| std::mem::take(&mut list.refresh_requested))
    }
}
