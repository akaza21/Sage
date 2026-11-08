use std::time::{Duration, Instant};

use crate::acp::{
    bindings::codex::{
        CodexAuthError, CodexAuthNotification, CodexAuthProtocol, NativeApiKey, NativeAuthUrl,
        NativeLoginId,
        discovery::{CodexDiscoveryRunError, DiscoveryZeroProcess},
    },
    limits::LIMITS,
    transport::{Frame, FrameWriteOutcome, OutboundFrame},
};

use super::{
    CodexAuthAccount,
    router::{AuthFrameRouter, RoutedFrame},
};

const AUTH_OPERATION_MS: u64 = 10 * 60 * 1_000;
const AUTH_POLL_MS: u64 = 100;

#[derive(Debug, Clone, Copy)]
pub(crate) struct CodexAuthDeadline {
    operation_end: Instant,
    cleanup_end: Instant,
}

impl CodexAuthDeadline {
    pub(crate) fn new() -> Result<Self, CodexDiscoveryRunError> {
        Self::from_budgets(
            Instant::now(),
            Duration::from_millis(AUTH_OPERATION_MS),
            Duration::from_millis(
                LIMITS
                    .deadlines_ms
                    .tree_termination
                    .saturating_add(LIMITS.deadlines_ms.explicit_close),
            ),
        )
    }

    fn from_budgets(
        admitted: Instant,
        operation: Duration,
        cleanup: Duration,
    ) -> Result<Self, CodexDiscoveryRunError> {
        let operation_end = admitted
            .checked_add(operation)
            .ok_or(CodexDiscoveryRunError::Timeout)?;
        let cleanup_end = operation_end
            .checked_add(cleanup)
            .ok_or(CodexDiscoveryRunError::Timeout)?;
        Ok(Self {
            operation_end,
            cleanup_end,
        })
    }

    pub(crate) fn operation_end(self) -> Instant {
        self.operation_end
    }

    pub(crate) fn cleanup_end(self) -> Instant {
        self.cleanup_end
    }

    fn remaining(self) -> Result<Duration, CodexDiscoveryRunError> {
        let remaining = self.operation_end.saturating_duration_since(Instant::now());
        (!remaining.is_zero())
            .then_some(remaining.min(Duration::from_millis(AUTH_POLL_MS)))
            .ok_or(CodexDiscoveryRunError::Timeout)
    }

    #[cfg(test)]
    fn test() -> Self {
        Self::from_budgets(
            Instant::now(),
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .unwrap()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodexAuthOutcome {
    SignedIn,
    SignedOut,
    Cancelled,
}

pub(crate) trait AuthBrowser {
    fn open(&self, url: &NativeAuthUrl) -> Result<(), CodexDiscoveryRunError>;
}

pub(crate) trait AuthTransport {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome;
    fn read_frame(&mut self, timeout: Duration) -> Result<Option<Frame>, CodexDiscoveryRunError>;
    fn user_cancel_requested(&self) -> bool;
    fn close_stdin(&mut self);
    fn terminate_zero(
        &mut self,
        failed: bool,
        deadline: CodexAuthDeadline,
    ) -> Result<DiscoveryZeroProcess, CodexDiscoveryRunError>;
}

pub(crate) fn run_login_protocol(
    protocol: &mut CodexAuthProtocol,
    transport: &mut dyn AuthTransport,
    browser: &dyn AuthBrowser,
    runtime_epoch: u64,
    deadline: CodexAuthDeadline,
) -> Result<CodexAuthOutcome, CodexDiscoveryRunError> {
    let result = login(protocol, transport, browser, deadline);
    settle(transport, runtime_epoch, deadline, result)
}

pub(crate) fn run_logout_protocol(
    protocol: &mut CodexAuthProtocol,
    transport: &mut dyn AuthTransport,
    runtime_epoch: u64,
    deadline: CodexAuthDeadline,
) -> Result<CodexAuthOutcome, CodexDiscoveryRunError> {
    let result = logout(protocol, transport, deadline);
    settle(transport, runtime_epoch, deadline, result)
}

pub(crate) fn run_api_key_protocol(
    protocol: &mut CodexAuthProtocol,
    transport: &mut dyn AuthTransport,
    key: &NativeApiKey,
    runtime_epoch: u64,
    deadline: CodexAuthDeadline,
) -> Result<CodexAuthOutcome, CodexDiscoveryRunError> {
    let result = api_key_login(protocol, transport, key, deadline);
    settle(transport, runtime_epoch, deadline, result)
}

pub(crate) fn run_account_protocol(
    protocol: &mut CodexAuthProtocol,
    transport: &mut dyn AuthTransport,
    runtime_epoch: u64,
    deadline: CodexAuthDeadline,
) -> Result<CodexAuthOutcome, CodexDiscoveryRunError> {
    let result = (|| {
        initialize(protocol, transport, deadline)?;
        Ok(match read_account(protocol, transport, deadline)? {
            CodexAuthAccount::SignedIn => CodexAuthOutcome::SignedIn,
            CodexAuthAccount::SignedOut => CodexAuthOutcome::SignedOut,
        })
    })();
    settle(transport, runtime_epoch, deadline, result)
}

fn login(
    protocol: &mut CodexAuthProtocol,
    transport: &mut dyn AuthTransport,
    browser: &dyn AuthBrowser,
    deadline: CodexAuthDeadline,
) -> Result<CodexAuthOutcome, CodexDiscoveryRunError> {
    initialize(protocol, transport, deadline)?;
    if read_account(protocol, transport, deadline)? != CodexAuthAccount::SignedOut {
        return Ok(CodexAuthOutcome::SignedIn);
    }
    let request = protocol.login_request()?;
    let expected = request.id();
    write(transport, request.into_frame())?;
    let mut router = AuthFrameRouter::new();
    let (login_id, url) = loop {
        let frame = read(transport, deadline)?;
        match router.route(protocol, frame)? {
            RoutedFrame::Response { id, frame } if id == expected => {
                break protocol.accept_login_started(&frame, expected)?;
            }
            RoutedFrame::Response { .. } => return Err(CodexDiscoveryRunError::Protocol),
            RoutedFrame::Notification => {}
        }
    };
    browser.open(&url)?;
    let mut completed = None;
    let mut updated = false;
    let mut cancel_id = None;
    let mut cancel_ack = false;
    loop {
        if transport.user_cancel_requested() && cancel_id.is_none() {
            let request = protocol.cancel_request(&login_id)?;
            cancel_id = Some(request.id());
            write(transport, request.into_frame())?;
        }
        while let Some(notification) = router.pop_notification() {
            accept_login_notification(notification, &login_id, &mut completed, &mut updated)?;
        }
        if completed == Some(false) && (cancel_id.is_none() || cancel_ack) {
            return Ok(CodexAuthOutcome::Cancelled);
        }
        if completed == Some(true) && updated {
            return Ok(CodexAuthOutcome::SignedIn);
        }
        let frame = read(transport, deadline)?;
        match router.route(protocol, frame)? {
            RoutedFrame::Response { id, frame } if Some(id) == cancel_id => {
                protocol.accept_cancel_response(&frame, id)?;
                cancel_ack = true;
            }
            RoutedFrame::Response { .. } => return Err(CodexDiscoveryRunError::Protocol),
            RoutedFrame::Notification => {}
        }
    }
}

/// Codex stores the key and answers only once it has; account notifications
/// that race the answer are accepted, and discovery re-reads the account.
fn api_key_login(
    protocol: &mut CodexAuthProtocol,
    transport: &mut dyn AuthTransport,
    key: &NativeApiKey,
    deadline: CodexAuthDeadline,
) -> Result<CodexAuthOutcome, CodexDiscoveryRunError> {
    initialize(protocol, transport, deadline)?;
    let request = protocol.api_key_login_request(key)?;
    let expected = request.id();
    write(transport, request.into_frame())?;
    let mut router = AuthFrameRouter::new();
    loop {
        let frame = read(transport, deadline)?;
        let answered = match router.route(protocol, frame)? {
            RoutedFrame::Response { id, frame } if id == expected => {
                protocol.accept_api_key_login(&frame, id)?;
                true
            }
            RoutedFrame::Response { .. } => return Err(CodexDiscoveryRunError::Protocol),
            RoutedFrame::Notification => false,
        };
        while let Some(notification) = router.pop_notification() {
            if !matches!(
                notification,
                CodexAuthNotification::AccountUpdated { signed_in: true }
                    | CodexAuthNotification::LoginCompleted {
                        login_id: None,
                        success: true,
                    }
            ) {
                return Err(CodexDiscoveryRunError::Protocol);
            }
        }
        if answered {
            return Ok(CodexAuthOutcome::SignedIn);
        }
    }
}

fn read_account(
    protocol: &mut CodexAuthProtocol,
    transport: &mut dyn AuthTransport,
    deadline: CodexAuthDeadline,
) -> Result<CodexAuthAccount, CodexDiscoveryRunError> {
    let request = protocol.account_request()?;
    let expected = request.id();
    write(transport, request.into_frame())?;
    let mut router = AuthFrameRouter::new();
    let mut early_update = None;
    loop {
        let frame = read(transport, deadline)?;
        match router.route(protocol, frame)? {
            RoutedFrame::Response { id, frame } if id == expected => {
                while let Some(notification) = router.pop_notification() {
                    match notification {
                        CodexAuthNotification::AccountUpdated { signed_in }
                            if early_update.is_none() =>
                        {
                            early_update = Some(signed_in)
                        }
                        _ => return Err(CodexDiscoveryRunError::Protocol),
                    }
                }
                let account = protocol.accept_account(&frame, expected)?;
                let observed_signed_in = account == CodexAuthAccount::SignedIn;
                if early_update.is_some_and(|signed_in| signed_in != observed_signed_in) {
                    return Err(CodexDiscoveryRunError::Protocol);
                }
                return Ok(account);
            }
            RoutedFrame::Response { .. } => return Err(CodexDiscoveryRunError::Protocol),
            RoutedFrame::Notification => {}
        }
    }
}

fn logout(
    protocol: &mut CodexAuthProtocol,
    transport: &mut dyn AuthTransport,
    deadline: CodexAuthDeadline,
) -> Result<CodexAuthOutcome, CodexDiscoveryRunError> {
    initialize(protocol, transport, deadline)?;
    let request = protocol.logout_request()?;
    let expected = request.id();
    write(transport, request.into_frame())?;
    let mut router = AuthFrameRouter::new();
    let mut acknowledged = false;
    let mut signed_out = false;
    while !acknowledged || !signed_out {
        let frame = read(transport, deadline)?;
        match router.route(protocol, frame)? {
            RoutedFrame::Response { id, frame } if id == expected => {
                protocol.accept_empty_response(&frame, id)?;
                acknowledged = true;
            }
            RoutedFrame::Response { .. } => return Err(CodexDiscoveryRunError::Protocol),
            RoutedFrame::Notification => {
                while let Some(notification) = router.pop_notification() {
                    match notification {
                        CodexAuthNotification::AccountUpdated { signed_in: false } => {
                            signed_out = true
                        }
                        CodexAuthNotification::LoginCompleted { success: false, .. } => {}
                        _ => return Err(CodexDiscoveryRunError::Protocol),
                    }
                }
            }
        }
    }
    Ok(CodexAuthOutcome::SignedOut)
}

fn initialize(
    protocol: &mut CodexAuthProtocol,
    transport: &mut dyn AuthTransport,
    deadline: CodexAuthDeadline,
) -> Result<(), CodexDiscoveryRunError> {
    let request = protocol.initialize_request()?;
    let expected = request.id();
    write(transport, request.into_frame())?;
    let frame = read(transport, deadline)?;
    let initialized = protocol.accept_initialize(&frame, expected)?;
    write(transport, initialized)
}

fn accept_login_notification(
    notification: CodexAuthNotification,
    login_id: &NativeLoginId,
    completed: &mut Option<bool>,
    updated: &mut bool,
) -> Result<(), CodexDiscoveryRunError> {
    match notification {
        CodexAuthNotification::LoginCompleted {
            login_id: Some(observed),
            success,
        } if observed == *login_id && completed.is_none() => *completed = Some(success),
        CodexAuthNotification::AccountUpdated { signed_in: true } if !*updated => *updated = true,
        _ => return Err(CodexDiscoveryRunError::Protocol),
    }
    Ok(())
}

fn read(
    transport: &mut dyn AuthTransport,
    deadline: CodexAuthDeadline,
) -> Result<Frame, CodexDiscoveryRunError> {
    transport
        .read_frame(deadline.remaining()?)?
        .ok_or(CodexDiscoveryRunError::Eof)
}

fn write(
    transport: &mut dyn AuthTransport,
    frame: OutboundFrame,
) -> Result<(), CodexDiscoveryRunError> {
    match transport.write(&frame) {
        FrameWriteOutcome::Complete => Ok(()),
        _ => Err(CodexDiscoveryRunError::Transport),
    }
}

fn settle<T>(
    transport: &mut dyn AuthTransport,
    runtime_epoch: u64,
    deadline: CodexAuthDeadline,
    result: Result<T, CodexDiscoveryRunError>,
) -> Result<T, CodexDiscoveryRunError> {
    transport.close_stdin();
    let zero = transport.terminate_zero(result.is_err(), deadline);
    match (result, zero) {
        (Ok(value), Ok(zero)) if zero.runtime_epoch() == runtime_epoch => Ok(value),
        (Err(error), Ok(zero)) if zero.runtime_epoch() == runtime_epoch => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(_), Err(CodexDiscoveryRunError::Cleanup)) => Err(CodexDiscoveryRunError::Cleanup),
        (Err(error), Err(_)) => Err(error),
        _ => Err(CodexDiscoveryRunError::Cleanup),
    }
}

impl From<CodexAuthError> for CodexDiscoveryRunError {
    fn from(_: CodexAuthError) -> Self {
        Self::Protocol
    }
}

#[cfg(test)]
mod tests;
