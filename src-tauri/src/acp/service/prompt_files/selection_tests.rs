//! Which staged files a send carries: only the batch it names, or for Retry
//! the files the chat's latest dispatched prompt carried.
use super::*;

const AT: i64 = 7;

fn paste(stages: &mut FileStages, chat: &str, text: &str) -> String {
    stages
        .append_text("p", chat, AT, None, text.into())
        .unwrap()
        .revision
}

#[test]
fn a_send_that_names_no_batch_leaves_it_for_the_next_message() {
    let mut stages = FileStages::default();
    let revision = paste(&mut stages, "c", "pasted log");
    // A queued message or a slash command dispatches with no selection.
    let queued = stages.selected("p", "c", None).unwrap();
    assert!(queued.0.is_empty());
    stages.consume("p", "c", &queued);
    assert_eq!(stages.list("p", "c").unwrap().revision, revision);
    // The message being written names the batch and takes it.
    let named = stages
        .selected("p", "c", Some(&StagedSelection::Revision(revision.clone())))
        .unwrap();
    assert_eq!(named.0[0].bytes, b"pasted log");
    stages.consume("p", "c", &named);
    assert!(stages.list("p", "c").is_none());
    // A revision that is no longer staged is refused, never sent empty.
    assert!(
        stages
            .selected("p", "c", Some(&StagedSelection::Revision(revision)))
            .is_err()
    );
}

#[test]
fn retry_resends_what_the_latest_message_carried() {
    let mut stages = FileStages::default();
    let last = Some(&StagedSelection::LastSent);
    assert!(stages.selected("p", "c", last).unwrap().0.is_empty());
    let revision = paste(&mut stages, "c", "first");
    let sent = stages
        .selected("p", "c", Some(&StagedSelection::Revision(revision)))
        .unwrap();
    stages.consume("p", "c", &sent);
    // A new paste staged for the next message is not what Retry resends.
    let next = paste(&mut stages, "c", "second");
    let retried = stages.selected("p", "c", last).unwrap();
    assert!(retried.same_selection(&sent));
    stages.consume("p", "c", &retried);
    assert!(
        stages
            .selected("p", "c", last)
            .unwrap()
            .same_selection(&sent)
    );
    assert_eq!(stages.list("p", "c").unwrap().revision, next);
    // A later message without files means Retry carries none.
    stages.consume("p", "c", &Resources::default());
    assert!(stages.selected("p", "c", last).unwrap().0.is_empty());
    stages.clear_chat("p", "c");
    assert!(stages.list("p", "c").is_none());
}

#[test]
fn kept_sent_files_are_bounded_and_a_released_retry_is_refused() {
    let mut stages = FileStages::default();
    let chats: Vec<String> = (0..=OWNER_COUNT).map(|i| format!("c{i}")).collect();
    for chat in &chats {
        let revision = paste(&mut stages, chat, "kept");
        let sent = stages
            .selected("p", chat, Some(&StagedSelection::Revision(revision)))
            .unwrap();
        stages.consume("p", chat, &sent);
    }
    let held = stages
        .sent
        .values()
        .filter(|s| s.resources.is_some())
        .count();
    assert_eq!(held, OWNER_COUNT);
    let last = Some(&StagedSelection::LastSent);
    let released = chats
        .iter()
        .filter(|chat| stages.selected("p", chat, last).is_err())
        .count();
    assert_eq!(released, 1);
    // The chat that sent most recently always keeps its files.
    assert!(
        !stages
            .selected("p", chats.last().unwrap(), last)
            .unwrap()
            .0
            .is_empty()
    );
}
