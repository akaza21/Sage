use super::super::PiRuntime;
use crate::acp::bindings::adapter::{AdapterEgress, AdapterRuntimeError};

impl PiRuntime {
    pub(in crate::acp::bindings::pi::runtime) fn begin_title_request(
        &mut self,
        input: &str,
    ) -> Result<(), AdapterRuntimeError> {
        self.launch
            .as_ref()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)?
            .revalidate_title()?;
        let model = self
            .current_model
            .as_ref()
            .and_then(|current| {
                self.available_models
                    .iter()
                    .find(|model| format!("{}:{}", model.provider, model.id) == *current)
            })
            .ok_or(AdapterRuntimeError::Operation)?;
        if self.title_request.is_some() || self.active_turn.is_some() || input.len() > 4096 {
            return Err(AdapterRuntimeError::Operation);
        }
        let command = self
            .title_launch
            .as_ref()
            .ok_or(AdapterRuntimeError::LaunchUnavailable)?
            .command(model)?;
        self.title_request =
            Some(crate::acp::bindings::pi::title_request::TitleRequest::start(command, input)?);
        Ok(())
    }
    pub(in crate::acp::bindings::pi::runtime) fn poll_title_request(
        &mut self,
    ) -> Option<AdapterEgress> {
        if let Some(result) = self
            .title_request
            .as_mut()
            .and_then(|request| request.poll())
        {
            self.title_result = match result {
                crate::acp::bindings::pi::title_request::TitleOutcome::Generated(title) => {
                    Some(Some(title))
                }
                crate::acp::bindings::pi::title_request::TitleOutcome::Failed => Some(None),
                crate::acp::bindings::pi::title_request::TitleOutcome::Preempted => None,
            };
            self.title_request = None;
        }
        None
    }
}
