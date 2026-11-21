use super::DriverMailbox;

impl DriverMailbox {
    /// Coalesced independently from protocol traffic; never a waiting queue.
    pub(crate) fn bind_title_admission(
        &self,
        admission: std::sync::Weak<crate::acp::service::chat_title::TitleAdmission>,
    ) {
        self.lock().title_admission = admission;
    }

    pub(crate) fn title_wake_available(&self) -> bool {
        !self.lock().closed
    }

    pub(crate) fn wake_title(&self) -> bool {
        let mut state = self.lock();
        if !state.closed {
            state.title_wake = true;
            self.ready.notify_one();
            return true;
        }
        false
    }
}
