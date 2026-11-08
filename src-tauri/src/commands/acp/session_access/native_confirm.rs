//! The native confirmation of a permissive access mode. The WebView's own
//! dialog explains the mode, but a script in the WebView could answer it;
//! this dialog is drawn by the operating system, so granting Auto or Full
//! access always needs a person at the keyboard. It names the chat.

use crate::{
    acp::contract::SageAgentSessionId, settings_store::SessionAccessMode, state::AppState,
};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

/// The chat's title, for the dialog. An untitled or unreadable chat reads
/// as "this chat".
fn chat_title(state: &AppState, session_id: &SageAgentSessionId) -> String {
    state
        .database
        .connection()
        .query_row(
            "SELECT chats.title FROM agent_sessions JOIN chats ON chats.id=agent_sessions.chat_id
             WHERE agent_sessions.id=?1",
            [session_id.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
        .map(|title| title.trim().chars().take(80).collect::<String>())
        .filter(|title| !title.is_empty())
        .map_or_else(
            || "this chat".to_owned(),
            |title| format!("\u{201c}{title}\u{201d}"),
        )
}

pub(super) fn dialog_text(mode: SessionAccessMode, chat: &str) -> (String, String) {
    let (label, effect) = match mode {
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
    (
        format!("Allow {label} in {chat}?"),
        format!("{effect} Other chats and new chats keep their own access."),
    )
}

/// Asks the person, in a native dialog, to allow `mode` in the chat.
pub(super) async fn confirmed(
    app: &tauri::AppHandle,
    state: &AppState,
    session_id: &SageAgentSessionId,
    mode: SessionAccessMode,
) -> bool {
    let (title, message) = dialog_text(mode, &chat_title(state, session_id));
    let (answer, answered) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(message)
        .title(title)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Allow".into(),
            "Cancel".into(),
        ))
        .show(move |allowed| {
            let _ = answer.send(allowed);
        });
    answered.await.unwrap_or(false)
}
