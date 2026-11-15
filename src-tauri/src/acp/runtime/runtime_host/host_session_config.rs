use super::*;

/// A live runtime serves one config change or native RPC at a time, and an
/// open Subagent view keeps issuing bounded background child reads (at most
/// one in flight, at least a second apart). A user command that finds the
/// slot busy waits for that read instead of failing: it retries a `State`
/// refusal a few times, briefly, which a background read never does.
///
/// The waits (at most 150 ms) stay on the calling thread. The config and
/// Goal commands already block that thread on `RuntimeHost::submit` while
/// they hold their Project-transition guard, for up to the host deadline, so
/// moving only the retry off it would change nothing; the fork's native RPC
/// already runs on a blocking thread outside the transition lock.
const USER_COMMAND_ATTEMPTS: u32 = 4;
const USER_COMMAND_RETRY: std::time::Duration = std::time::Duration::from_millis(50);

pub(super) fn yield_to_background_read<T>(
    mut attempt: impl FnMut() -> Result<T, HostError>,
    mut wait: impl FnMut(),
) -> Result<T, HostError> {
    let mut remaining = USER_COMMAND_ATTEMPTS;
    loop {
        match attempt() {
            Err(HostError::State) if remaining > 1 => {
                remaining -= 1;
                wait();
            }
            result => return result,
        }
    }
}

/// The callers that predate `NotDispatched` keep seeing `Deadline`.
fn dispatched_or_deadline(error: HostError) -> HostError {
    match error {
        HostError::NotDispatched => HostError::Deadline,
        error => error,
    }
}

impl RuntimeHost {
    pub(crate) fn native_rpc(
        &self,
        intent: crate::acp::goal::NativeRpcIntent,
    ) -> Result<crate::acp::goal::NativeRpcReply, HostError> {
        let background = matches!(intent.request, crate::acp::goal::NativeRpcRequest::Read(_));
        let send = || {
            let _hold = self.hold_control()?;
            match self
                .submit(HostCommand::NativeRpc(intent.clone()))
                .map_err(dispatched_or_deadline)?
            {
                HostReply::NativeRpc(reply) => Ok(reply),
                _ => Err(HostError::State),
            }
        };
        if background {
            return send();
        }
        yield_to_background_read(send, || std::thread::sleep(USER_COMMAND_RETRY))
    }
    pub(crate) fn configure_session(
        &self,
        intent: sage_acp_protocol::session_config::ConfigIntent,
    ) -> Result<HostOutcome, HostError> {
        yield_to_background_read(
            || {
                let _hold = self.hold_control()?;
                self.outcome(HostCommand::ConfigureSession(intent.clone()))
                    .map_err(dispatched_or_deadline)
            },
            || std::thread::sleep(USER_COMMAND_RETRY),
        )
    }
    /// The same change, answered by `deadline` at the latest. Unlike
    /// `configure_session`, a deadline that passes before the change is sent
    /// is reported as `NotDispatched`.
    pub(crate) fn configure_session_until(
        &self,
        intent: sage_acp_protocol::session_config::ConfigIntent,
        deadline: std::time::Instant,
    ) -> Result<HostOutcome, HostError> {
        yield_to_background_read(
            || {
                let _hold = self.hold_control()?;
                self.outcome_until(HostCommand::ConfigureSession(intent.clone()), deadline)
            },
            || std::thread::sleep(USER_COMMAND_RETRY),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_user_command_waits_out_a_busy_slot_but_not_forever() {
        let mut calls = 0;
        let mut waits = 0;
        let result = yield_to_background_read(
            || {
                calls += 1;
                if calls < 3 {
                    Err(HostError::State)
                } else {
                    Ok(calls)
                }
            },
            || waits += 1,
        );
        assert_eq!(result, Ok(3));
        assert_eq!(waits, 2);

        let mut calls = 0;
        let refused: Result<(), _> = yield_to_background_read(
            || {
                calls += 1;
                Err(HostError::State)
            },
            || {},
        );
        assert_eq!(refused, Err(HostError::State));
        assert_eq!(calls, USER_COMMAND_ATTEMPTS);

        // Any other refusal is final at once.
        let mut calls = 0;
        let closed: Result<(), _> = yield_to_background_read(
            || {
                calls += 1;
                Err(HostError::Closed)
            },
            || {},
        );
        assert_eq!(closed, Err(HostError::Closed));
        assert_eq!(calls, 1);
    }
}
