use super::{AdapterRuntimeError, NativeRuntime};

impl NativeRuntime {
    pub(super) fn publish_config(&self) -> Result<(), AdapterRuntimeError> {
        if let Some((owner, channel)) = &self.1 {
            match self.0.session_config() {
                Some(config) => channel.update(owner, config),
                None => channel.remove(owner),
            }
            .map_err(|_| AdapterRuntimeError::Frame)?;
            channel.set_hydrating(owner, self.0.config_hydrating());
        }
        Ok(())
    }

    pub(super) fn publish_goal(&self) {
        if let Some((owner, channel)) = &self.2
            && let Some(goal) = self.0.goal_state()
        {
            channel.update_observed(owner, goal);
        }
    }
}

impl Drop for NativeRuntime {
    fn drop(&mut self) {
        if let Some((owner, channel)) = &self.3 {
            channel.clear(owner);
        }
        if let Some((owner, channel)) = &self.4 {
            channel.clear(owner);
        }
    }
}
