use super::*;
use std::sync::Mutex;

/// A dialog whose answer the test gives: at once, or never (kept open).
#[derive(Default)]
pub(in crate::commands::acp) struct Dialog {
    pub answer: Option<bool>,
    pub shown: Mutex<Vec<(String, String)>>,
    pub open: Mutex<Vec<Answer>>,
}

impl Dialog {
    pub fn answering(answer: bool) -> Self {
        Self {
            answer: Some(answer),
            ..Self::default()
        }
    }
}

impl ConfirmDialog for Dialog {
    fn show(&self, title: String, message: String, answer: Answer) {
        self.shown.lock().unwrap().push((title, message));
        match self.answer {
            Some(allowed) => answer(allowed),
            None => self.open.lock().unwrap().push(answer),
        }
    }
}

pub(in crate::commands::acp) const PATIENT: Timing = Timing {
    min_decision: Duration::ZERO,
    limit: Duration::from_secs(5),
};

fn text() -> (String, String) {
    ("title".into(), "message".into())
}

fn label(project: &str, chat: &str) -> SessionLabel {
    SessionLabel {
        project: project.into(),
        chat: chat.into(),
        harness: "Claude Code".into(),
        session_id: "session-0123456789abcdef".into(),
    }
}

#[test]
fn the_dialog_names_the_project_chat_and_session() {
    let (title, message) = dialog_text(
        SessionAccessMode::FullAccess,
        &label("Sage", "Fix the build"),
    );
    assert_eq!(title, "Allow Full access for this chat?");
    assert!(message.contains("Project: \u{201c}Sage\u{201d}"));
    assert!(message.contains("Chat: \u{201c}Fix the build\u{201d}"));
    assert!(message.contains("Session: Claude Code, ID \u{2026}89abcdef"));
    assert!(message.contains("no filesystem sandbox"));
    let (title, message) = dialog_text(SessionAccessMode::Auto, &label("", " \n"));
    assert_eq!(title, "Allow Auto for this chat?");
    assert!(message.contains("Project: (unnamed project)"));
    assert!(message.contains("Chat: (untitled chat)"));
}

#[test]
fn names_lose_control_bidi_and_line_characters() {
    // A title that would fake a second line, reverse text or hide words.
    let chat = "Notes\n\nProject: \u{201c}Other\u{201d}\r\u{202E}evil\u{202C}\u{200B}\u{2066}x\u{2069}\t\u{0007}end";
    let shown = sanitised(chat);
    assert_eq!(shown, "Notes Project: \u{201c}Other\u{201d} evilx end");
    assert!(!shown.chars().any(|c| c.is_control() || is_invisible(c)));
    assert_eq!(sanitised(&"x".repeat(500)).chars().count(), NAME_CHARS);
    let (_, message) = dialog_text(SessionAccessMode::FullAccess, &label("A\nB", chat));
    // The only line breaks are the dialog's own.
    assert_eq!(message.lines().count(), 5);
    assert!(message.starts_with("Project: \u{201c}A B\u{201d}\nChat: \u{201c}Notes Project:"));
}

#[tokio::test]
async fn only_a_deliberate_allow_is_accepted() {
    let gate = ConfirmGate::default();
    let allowed = Dialog::answering(true);
    assert_eq!(ask(&gate, &allowed, text(), PATIENT).await, Ok(()));
    assert_eq!(allowed.shown.lock().unwrap().len(), 1);
    // Cancel, Escape and "Don't allow" all answer false.
    let refused = Dialog::answering(false);
    assert_eq!(
        ask(&gate, &refused, text(), PATIENT).await,
        Err(Refusal::Declined)
    );
    // An Allow quicker than a person reads is refused.
    let hasty = Timing {
        min_decision: Duration::from_secs(60),
        ..PATIENT
    };
    assert_eq!(
        ask(&gate, &allowed, text(), hasty).await,
        Err(Refusal::TooFast)
    );
    assert_eq!(MIN_DECISION, Duration::from_millis(750));
}

#[tokio::test]
async fn one_confirmation_is_open_at_a_time_and_unanswered_ones_time_out() {
    let gate = ConfirmGate::default();
    let open = Dialog::default();
    let brief = Timing {
        min_decision: Duration::ZERO,
        limit: Duration::from_millis(20),
    };
    assert_eq!(
        ask(&gate, &open, text(), brief).await,
        Err(Refusal::TimedOut)
    );
    // The first dialog is still on screen, so no second one opens.
    let other = Dialog::answering(true);
    assert_eq!(
        ask(&gate, &other, text(), PATIENT).await,
        Err(Refusal::Busy)
    );
    assert!(other.shown.lock().unwrap().is_empty());
    // A late Allow on the timed-out dialog grants nothing, and frees the gate.
    let late = open.open.lock().unwrap().pop().unwrap();
    late(true);
    assert_eq!(ask(&gate, &other, text(), PATIENT).await, Ok(()));
    // A dialog closed without an answer counts as a refusal.
    let dropped = Dialog::default();
    let asked = ask(&gate, &dropped, text(), PATIENT);
    let closing = async {
        tokio::task::yield_now().await;
        dropped.open.lock().unwrap().clear();
    };
    let (result, ()) = tokio::join!(asked, closing);
    assert_eq!(result, Err(Refusal::Declined));
    assert_eq!(ask(&gate, &other, text(), PATIENT).await, Ok(()));
    assert_eq!(Timing::NATIVE.limit, Duration::from_secs(120));
}
