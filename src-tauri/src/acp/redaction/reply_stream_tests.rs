use super::{KEEP_AFTER, ReplyStream};
use crate::acp::redaction::text::reply_text;

fn streamed(text: &str, chunk: usize) -> (Vec<String>, String, usize) {
    let mut stream = ReplyStream::default();
    let mut shown = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + chunk).min(text.len());
        while !text.is_char_boundary(end) {
            end += 1;
        }
        stream.push(&text[start..end]).unwrap();
        shown.push(stream.streaming());
        start = end;
    }
    (shown, stream.finished(), stream.kept)
}

fn up_to_last_whitespace(text: &str) -> &str {
    text.char_indices()
        .rfind(|(_, character)| character.is_whitespace())
        .map_or("", |(index, character)| {
            &text[..index + character.len_utf8()]
        })
}

#[test]
fn a_word_still_arriving_is_never_shown_until_it_ends() {
    for deltas in [
        &["/srv/alice:hun", "ter2@db/x", " done"][..],
        &["token=hun", "ter2 done"],
        &["see sk-ab", "cdef0123456789 done"],
        &["Bearer ", "hun", "ter2 done"],
    ] {
        let mut stream = ReplyStream::default();
        for delta in deltas {
            stream.push(delta).unwrap();
            let shown = stream.streaming();
            for part in ["hun", "abcdef"] {
                assert!(!shown.contains(part), "{deltas:?}: {shown}");
            }
        }
        let finished = stream.finished();
        assert_eq!(finished, reply_text(&deltas.concat()), "{deltas:?}");
        assert!(finished.ends_with(" done"), "{finished}");
    }
}

#[test]
fn every_view_matches_redacting_the_whole_text_however_it_streams() {
    let filler = "plain words keep the reply long enough to keep a prefix. ".repeat(40);
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
        for chunk in [3, 64, 1500] {
            let (views, finished, kept) = streamed(&text, chunk);
            assert_eq!(finished, reply_text(&text), "{piece} by {chunk}");
            let fails_closed = finished.ends_with("[redacted]");
            assert!(
                fails_closed || kept > 0,
                "{piece} by {chunk} keeps a prefix"
            );
            let mut sent = 0;
            for view in views {
                sent = (sent + chunk).min(text.len());
                while !text.is_char_boundary(sent) {
                    sent += 1;
                }
                let expected = reply_text(up_to_last_whitespace(&text[..sent]));
                assert_eq!(view, expected, "{piece} by {chunk} at {sent}");
            }
        }
    }
}

#[test]
fn streaming_work_per_delta_does_not_grow_with_the_reply() {
    let per_delta = |bytes: usize| {
        let text = "word and a /srv/path/x line\n".repeat(bytes / 28);
        let mut stream = ReplyStream::default();
        let mut deltas = 0;
        for chunk in text.as_bytes().chunks(16) {
            stream.push(std::str::from_utf8(chunk).unwrap()).unwrap();
            stream.streaming();
            deltas += 1;
        }
        assert!(stream.kept > 0, "a long reply keeps a redacted prefix");
        stream.redacted_bytes / deltas
    };
    let short = per_delta(32 * 1024);
    let long = per_delta(256 * 1024);
    assert!(long <= 3 * KEEP_AFTER, "{long} bytes redacted per delta");
    assert!(
        long <= short * 2,
        "work per delta grew from {short} to {long}"
    );
}
