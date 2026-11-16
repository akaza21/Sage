use super::{HELD_MAX, KEEP_AFTER, ReplyStream};
use crate::acp::redaction::text::reply_text;

fn streamed(text: &str, chunk: usize) -> (Vec<(usize, String)>, ReplyStream) {
    let mut stream = ReplyStream::default();
    let mut shown = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + chunk).min(text.len());
        while !text.is_char_boundary(end) {
            end += 1;
        }
        stream.push(&text[start..end]).unwrap();
        shown.push((end, stream.streaming()));
        start = end;
    }
    (shown, stream)
}

fn visible(text: &str) -> &str {
    &text[..ReplyStream::new(text).visible_end()]
}

#[test]
fn a_word_still_arriving_is_never_shown_until_it_ends() {
    for deltas in [
        &["/srv/alice:hun", "ter2@db/x", " done"][..],
        &["token=hun", "ter2 done"],
        &["see sk-ab", "cdef0123456789 done"],
        &["Bearer ", "hun", "ter2 done"],
        &["见 https://alice:密", "码@host 完"],
    ] {
        let mut stream = ReplyStream::default();
        for delta in deltas {
            stream.push(delta).unwrap();
            let shown = stream.streaming();
            for part in ["hun", "abcdef", "密"] {
                assert!(!shown.contains(part), "{deltas:?}: {shown}");
            }
        }
        let finished = stream.finished();
        assert_eq!(finished, reply_text(&deltas.concat()), "{deltas:?}");
    }
}

#[test]
fn text_without_spaces_streams_up_to_its_last_wide_character() {
    let mut stream = ReplyStream::default();
    for delta in ["这是一段", "没有空格的中文", "段落abc"] {
        stream.push(delta).unwrap();
    }
    assert_eq!(stream.streaming(), "这是一段没有空格的中文段落");
    let mut long = ReplyStream::default();
    long.push(&"a".repeat(3 * HELD_MAX)).unwrap();
    assert_eq!(long.streaming().len(), 3 * HELD_MAX - HELD_MAX);
}

#[test]
fn every_view_is_the_redacted_text_shown_so_far_or_the_view_before() {
    let filler = "plain words keep the reply long enough to keep a prefix. ".repeat(160);
    let home = std::env::home_dir().expect("home folder");
    let home = home.to_str().expect("UTF-8 home folder").to_owned();
    let tricky = [
        "Bearer\nhunter2 and Bearer hunter3 then".to_owned(),
        "password=\"first line\nsecond line\nthird\" after".to_owned(),
        "-----BEGIN PRIVATE KEY-----\nMIIEvAIBADANBgkqhkiG9w0BAQEFAASC\nYWJjZGVmZ2hpams=\n-----END PRIVATE KEY-----\n"
            .to_owned(),
        "/srv/alice:hunter2@db/bob:secondpass@host/x".to_owned(),
        format!("{home}/.config and {home} Smith/x and {home}2/y"),
        "{\"token\": \"abc def\", \"name\": \"x\"}".to_owned(),
        "--password 'hunter2 rejected; it isn't valid".to_owned(),
    ];
    for piece in &tricky {
        let text = format!("{filler}\n{piece}\n{filler}{piece} {filler}\n{piece}");
        for chunk in [61, 1500] {
            let (views, mut stream) = streamed(&text, chunk);
            assert_eq!(stream.finished(), reply_text(&text), "{piece} by {chunk}");
            let mut previous = String::new();
            for (sent, view) in views {
                if view != previous {
                    let expected = reply_text(visible(&text[..sent]));
                    assert_eq!(view, expected, "{piece} by {chunk} at {sent}");
                }
                previous = view;
            }
        }
    }
}

#[test]
fn streaming_work_per_delta_stays_bounded_even_when_a_construct_never_closes() {
    let per_delta = |opening: &str, bytes: usize| {
        let text = format!(
            "{opening}{}",
            "word and a /srv/path/x line\n".repeat(bytes / 28)
        );
        let mut stream = ReplyStream::default();
        let mut deltas = 0;
        for chunk in text.as_bytes().chunks(256) {
            stream.push(std::str::from_utf8(chunk).unwrap()).unwrap();
            stream.streaming();
            deltas += 1;
        }
        stream.redacted_bytes / deltas
    };
    for opening in [
        "",
        "--password 'hunter2 rejected; it isn't valid ",
        "-----BEGIN PRIVATE KEY-----\n",
        "config = {\"nested\": [",
    ] {
        let short = per_delta(opening, 48 * 1024);
        let long = per_delta(opening, 256 * 1024);
        assert!(long <= 3 * KEEP_AFTER, "{opening}: {long} bytes per delta");
        assert!(
            long <= short * 2,
            "{opening}: work per delta grew from {short} to {long}"
        );
    }
}

#[test]
fn a_reply_shown_whole_for_a_tool_call_is_not_taken_back_by_the_next_delta() {
    let mut stream = ReplyStream::default();
    stream.push("I'll write that single file.").unwrap();
    assert_eq!(stream.streaming(), "I'll write that single ");
    assert_eq!(stream.finished(), "I'll write that single file.");
    stream.push("Done").unwrap();
    assert_eq!(stream.streaming(), "I'll write that single file.");
    stream.push(" now").unwrap();
    assert_eq!(stream.streaming(), "I'll write that single file.Done ");
}

#[test]
fn a_whole_reply_longer_than_an_item_is_clipped_not_dropped() {
    let mut stream = ReplyStream::new(&"token=abc ".repeat(100));
    let whole = stream.finished();
    let clipped = stream.finished_within(whole.len() - 10);
    assert!(clipped.len() <= whole.len() - 10, "{clipped}");
    assert!(clipped.ends_with('\u{2026}'), "{clipped}");
    assert!(whole.starts_with(clipped.trim_end_matches('\u{2026}')));
}
