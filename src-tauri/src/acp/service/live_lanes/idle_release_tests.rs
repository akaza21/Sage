use super::*;

fn chat(name: &str) -> SageChatId {
    SageChatId::parse(format!("chat-{name}")).unwrap()
}

const MINUTE: Duration = Duration::from_secs(60);

#[test]
fn a_shown_chat_is_due_only_after_the_long_idle_wait() {
    let start = Instant::now();
    let mut view = View {
        shown: Some([chat("a")].into_iter().collect()),
        ..View::default()
    };
    assert!(!view.due(&chat("a"), start, start + 14 * MINUTE));
    assert!(view.due(&chat("a"), start, start + SHOWN_IDLE));
    assert!(view.hidden_since.is_empty(), "a shown Chat is never marked");
}

#[test]
fn a_hidden_chat_is_due_once_both_idle_and_out_of_view_for_the_short_wait() {
    let start = Instant::now();
    let mut view = View {
        shown: Some(HashSet::new()),
        ..View::default()
    };
    // Idle long enough, but first seen out of view only now.
    assert!(!view.due(&chat("b"), start, start + 3 * MINUTE));
    assert!(!view.due(&chat("b"), start, start + 4 * MINUTE));
    assert!(view.due(&chat("b"), start, start + 5 * MINUTE));
    // Out of view long enough, but a Turn just ended.
    let turn_ended = start + 4 * MINUTE;
    assert!(!view.due(&chat("b"), turn_ended, start + 5 * MINUTE));
    assert!(view.due(&chat("b"), turn_ended, turn_ended + HIDDEN_IDLE));
}

#[test]
fn without_a_view_report_no_chat_counts_as_hidden() {
    let start = Instant::now();
    let mut view = View::default();
    assert!(!view.due(&chat("c"), start, start + 10 * MINUTE));
    assert!(view.hidden_since.is_empty());
    assert!(view.due(&chat("c"), start, start + SHOWN_IDLE));
}
