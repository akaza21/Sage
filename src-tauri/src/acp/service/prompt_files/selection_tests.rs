use super::*;

const AT: i64 = 7;

fn paste(stages: &mut FileStages, chat: &str, text: &str) -> String {
    stages
        .append_text("p", chat, AT, None, text.into())
        .unwrap()
        .revision
}

fn named(revision: &str) -> Selection {
    Selection::Revision(revision.to_owned())
}

fn retry(intent: &str, attachments: usize) -> Selection {
    Selection::Retry(RetryOrigin {
        client_intent_id: intent.into(),
        attachments: Some(attachments),
    })
}

#[test]
fn a_send_that_names_no_batch_leaves_it_for_the_next_message() {
    let mut stages = FileStages::default();
    let revision = paste(&mut stages, "c", "pasted log");
    let queued = stages.selected("p", "c", &Selection::None).unwrap();
    assert!(queued.0.is_empty());
    stages.consume("p", "c", AT, "queued", &queued);
    assert_eq!(stages.list("p", "c").unwrap().revision, revision);
    let sent = stages.selected("p", "c", &named(&revision)).unwrap();
    assert_eq!(sent.0[0].bytes, b"pasted log");
    stages.consume("p", "c", AT, "written", &sent);
    assert!(stages.list("p", "c").is_none());
    assert!(stages.selected("p", "c", &named(&revision)).is_err());
}

#[test]
fn retry_resends_what_that_turn_carried() {
    let mut stages = FileStages::default();
    let revision = paste(&mut stages, "c", "first");
    let sent = stages.selected("p", "c", &named(&revision)).unwrap();
    stages.consume("p", "c", AT, "first", &sent);
    let next = paste(&mut stages, "c", "second");
    let retried = stages.selected("p", "c", &retry("first", 1)).unwrap();
    assert!(retried.same_selection(&sent));
    stages.consume("p", "c", AT, "again", &retried);
    let again = stages.selected("p", "c", &retry("again", 1)).unwrap();
    assert!(again.same_selection(&sent));
    assert_eq!(stages.list("p", "c").unwrap().revision, next);
    stages.consume("p", "c", AT, "plain", &Resources::default());
    assert!(
        stages
            .selected("p", "c", &retry("plain", 0))
            .unwrap()
            .0
            .is_empty()
    );
    stages.clear_chat("p", "c");
    assert!(stages.list("p", "c").is_none());
}

#[test]
fn retry_without_a_record_is_refused_when_the_turn_carried_files() {
    let mut stages = FileStages::default();
    let refused = stages.selected("p", "c", &retry("before-restart", 2));
    assert!(refused.unwrap_err().contains("no longer held"));
    assert!(
        stages
            .selected("p", "c", &retry("before-restart", 0))
            .unwrap()
            .0
            .is_empty()
    );
    let revision = paste(&mut stages, "c", "other");
    let sent = stages.selected("p", "c", &named(&revision)).unwrap();
    stages.consume("p", "c", AT, "other", &sent);
    assert!(stages.selected("p", "c", &retry("older", 1)).is_err());
    assert!(
        stages
            .selected("p", "c", &retry("other", 1))
            .unwrap()
            .same_selection(&sent)
    );
}

#[test]
fn kept_sent_files_are_bounded_by_recency_and_a_released_retry_is_refused() {
    let mut stages = FileStages::default();
    let chats: Vec<String> = (0..=OWNER_COUNT).map(|i| format!("c{i}")).collect();
    // The first chat sends last-but-one, so it is not the oldest.
    let order: Vec<&String> = chats[1..].iter().chain([&chats[0]]).collect();
    for chat in &order {
        let revision = paste(&mut stages, chat, "kept");
        let sent = stages.selected("p", chat, &named(&revision)).unwrap();
        stages.consume("p", chat, AT, chat, &sent);
    }
    let released: Vec<_> = chats
        .iter()
        .filter(|chat| stages.selected("p", chat, &retry(chat, 1)).is_err())
        .collect();
    assert_eq!(released, [&chats[1]]);
}

#[test]
fn kept_sent_files_count_in_the_memory_cap_and_give_way_to_a_new_stage() {
    let mut stages = FileStages::default();
    let big = "x".repeat(TOTAL_BYTES);
    let mut sent_chats = Vec::new();
    for i in 0.. {
        let chat = format!("s{i}");
        let revision = paste(&mut stages, &chat, &big);
        let sent = stages.selected("p", &chat, &named(&revision)).unwrap();
        stages.consume("p", &chat, AT, &chat, &sent);
        sent_chats.push(chat);
        if stages.retained(None) + TOTAL_BYTES * 2 > STORE_BYTES {
            break;
        }
    }
    paste(&mut stages, "new", &big);
    paste(&mut stages, "newer", &big);
    assert!(stages.retained(None) <= STORE_BYTES);
    assert!(
        stages
            .selected("p", &sent_chats[0], &retry(&sent_chats[0], 1))
            .is_err()
    );
}
