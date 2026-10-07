use super::super::teardown::UNCERTAIN_CLOSE_PREFIX;

pub(super) fn uncertain_close(error: impl std::fmt::Display) -> String {
    format!("{UNCERTAIN_CLOSE_PREFIX}: {error}")
}

pub(super) async fn wait_for_native_close(
    receiver: tokio::sync::oneshot::Receiver<Result<(), String>>,
    timeout: std::time::Duration,
) -> Result<(), String> {
    match tokio::time::timeout(timeout, receiver).await {
        Ok(Ok(result)) => result.map_err(uncertain_close),
        Ok(Err(error)) => Err(uncertain_close(format!(
            "native close acknowledgment disconnected: {error}"
        ))),
        Err(_) => Err(uncertain_close("native close timed out")),
    }
}
