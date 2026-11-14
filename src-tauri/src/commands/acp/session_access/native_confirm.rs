//! The native confirmation of a permissive access mode. The WebView's own
//! dialog explains the mode, but a script in the WebView could answer it;
//! this dialog is drawn by the operating system, so granting Auto or Full
//! access always needs a person at the keyboard. It names the project, the
//! chat and the session (its harness and a short id the WebView cannot
//! choose), its safe answer is the default, an Allow given faster than a
//! person reads is refused, and only one such dialog is open at a time.

use crate::{
    acp::contract::SageAgentSessionId, acp_protocol::access_mode::SessionAccessMode,
    state::AppState,
};
use std::sync::{
    Arc, LazyLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use tauri::Manager;
use tauri_plugin_dialog::{
    DialogExt, MessageDialogButtons, MessageDialogKind, MessageDialogResult,
};

/// The quickest Allow Sage believes a person gave after reading the dialog.
pub(super) const MIN_DECISION: Duration = Duration::from_millis(750);
const ALLOW: &str = "Allow";
const DONT_ALLOW: &str = "Don't allow";
const NAME_CHARS: usize = 80;

/// What a dialog names: each part is sanitised before it is shown.
pub(super) struct SessionLabel {
    pub project: String,
    pub chat: String,
    pub harness: String,
    pub session_id: String,
}

/// How long a person has, and must at least take, to answer.
#[derive(Clone, Copy)]
pub(super) struct Timing {
    pub min_decision: Duration,
    pub limit: Duration,
}

impl Timing {
    pub(super) const NATIVE: Self = Self {
        min_decision: MIN_DECISION,
        limit: Duration::from_millis(crate::settings_store::ACCESS_CONFIRMATION_MILLIS as u64),
    };
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Refusal {
    /// Another confirmation is still open.
    Busy,
    Declined,
    /// Allowed sooner than a person could have read the dialog.
    TooFast,
    TimedOut,
}

/// Receives whether the person chose Allow.
pub(super) type Answer = Box<dyn FnOnce(bool) + Send>;

/// Shows a confirmation and reports whether the person chose Allow. The
/// answer may be dropped unsent, which counts as a refusal.
pub(super) trait ConfirmDialog {
    fn show(&self, title: String, message: String, answer: Answer);
}

/// Admits one confirmation at a time. Its slot is held until the dialog
/// itself closes, even after the command stopped waiting for it.
#[derive(Default)]
pub(super) struct ConfirmGate(Arc<AtomicBool>);

struct ConfirmSlot(Arc<AtomicBool>);

impl ConfirmGate {
    fn enter(&self) -> Option<ConfirmSlot> {
        self.0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| ConfirmSlot(self.0.clone()))
    }
}

impl Drop for ConfirmSlot {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub(super) static NATIVE_GATE: LazyLock<ConfirmGate> = LazyLock::new(ConfirmGate::default);

/// Asks once, through `dialog`, whether to allow; see the module docs.
pub(super) async fn ask(
    gate: &ConfirmGate,
    dialog: &impl ConfirmDialog,
    (title, message): (String, String),
    timing: Timing,
) -> Result<(), Refusal> {
    let slot = gate.enter().ok_or(Refusal::Busy)?;
    let (answer, answered) = tokio::sync::oneshot::channel();
    let shown = Instant::now();
    dialog.show(
        title,
        message,
        Box::new(move |allowed| {
            drop(slot);
            let _ = answer.send((allowed, shown.elapsed()));
        }),
    );
    match tokio::time::timeout(timing.limit, answered).await {
        Err(_) => Err(Refusal::TimedOut),
        Ok(Err(_) | Ok((false, _))) => Err(Refusal::Declined),
        Ok(Ok((true, after))) if after < timing.min_decision => Err(Refusal::TooFast),
        Ok(Ok((true, _))) => Ok(()),
    }
}

/// The operating system's dialog, over Sage's main window. "Don't allow"
/// is its first, default button and Escape answers Cancel, so only a
/// deliberate choice of Allow allows.
pub(super) struct NativeDialog<'a>(pub &'a tauri::AppHandle);

impl ConfirmDialog for NativeDialog<'_> {
    fn show(&self, title: String, message: String, answer: Answer) {
        let mut dialog = self
            .0
            .dialog()
            .message(message)
            .title(title)
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::YesNoCancelCustom(
                DONT_ALLOW.into(),
                ALLOW.into(),
                "Cancel".into(),
            ));
        if let Some(window) = self.0.get_window("main") {
            dialog = dialog.parent(&window);
        }
        dialog.show_with_result(move |result| {
            answer(matches!(result, MessageDialogResult::Custom(label) if label == ALLOW));
        });
    }
}

/// Text from the WebView or the database, safe to show: control, bidi and
/// zero-width characters (newlines too) become spaces or are dropped, runs
/// of space collapse, and it is cut to a short length.
pub(super) fn sanitised(text: &str) -> String {
    let shown = text
        .chars()
        .filter(|c| !is_invisible(*c))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>();
    shown
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(NAME_CHARS)
        .collect()
}

fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{061C}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

fn harness_name(binding_id: &str) -> String {
    match binding_id {
        "claude-agent-acp" | "claude-code-sdk" => "Claude Code".into(),
        "codex-app-server" => "Codex".into(),
        "opencode-acp" => "OpenCode".into(),
        "pi-rpc" => "Pi".into(),
        other => sanitised(other),
    }
}

/// The project, chat and harness of a session, as stored by Sage.
pub(super) fn session_label(state: &AppState, session_id: &SageAgentSessionId) -> SessionLabel {
    let stored = state
        .database
        .connection()
        .query_row(
            "SELECT projects.name, chats.title, agent_sessions.binding_id
             FROM agent_sessions
             JOIN chats ON chats.id=agent_sessions.chat_id
             JOIN projects ON projects.id=agent_sessions.project_id
             WHERE agent_sessions.id=?1",
            [session_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .ok();
    let (project, chat, binding) = stored.unwrap_or_default();
    SessionLabel {
        project,
        chat,
        harness: harness_name(&binding),
        session_id: session_id.as_str().to_owned(),
    }
}

pub(super) fn dialog_text(mode: SessionAccessMode, label: &SessionLabel) -> (String, String) {
    let (name, effect) = match mode {
        SessionAccessMode::Auto => (
            "Auto",
            "Claude's own checks decide which actions run without asking. Sage does not review \
             them, and there is no filesystem sandbox.",
        ),
        _ => (
            "Full access",
            "The agent can change or delete files and run commands without asking, with no \
             filesystem sandbox.",
        ),
    };
    let named = |text: &str, unnamed: &str| {
        let text = sanitised(text);
        if text.is_empty() {
            unnamed.to_owned()
        } else {
            format!("\u{201c}{text}\u{201d}")
        }
    };
    let short_id = sanitised(&label.session_id)
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    let harness = match sanitised(&label.harness) {
        harness if harness.is_empty() => "an agent".to_owned(),
        harness => harness,
    };
    (
        format!("Allow {name} for this chat?"),
        format!(
            "Project: {}\nChat: {}\nSession: {harness}, ID \u{2026}{short_id}\n\n{effect} Other \
             chats and new chats keep their own access.",
            named(&label.project, "(unnamed project)"),
            named(&label.chat, "(untitled chat)"),
        ),
    )
}

#[cfg(test)]
#[path = "native_confirm_tests.rs"]
pub(super) mod tests;
