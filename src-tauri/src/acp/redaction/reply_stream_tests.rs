use super::{ITEM_BYTES, KEEP_EVERY, LOOKAHEAD, REFRESH_SHARE, ReplyStream};
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
    long.push(&"a".repeat(3 * LOOKAHEAD)).unwrap();
    assert_eq!(long.streaming(), "");
}

#[test]
fn a_joined_word_longer_than_any_span_is_held_back_whole() {
    let line = format!(
        "{}postgres://admin:S3cretPass@db/app{} done",
        "var a=1;".repeat(LOOKAHEAD / 4),
        ";b=2".repeat(LOOKAHEAD / 2)
    );
    for chunk in [1, 7, 64] {
        let (views, mut stream) = streamed(&format!("Minified: {line}"), chunk);
        for (sent, view) in views {
            assert!(!view.contains("S3cr"), "by {chunk} at {sent}");
        }
        assert!(!stream.finished().contains("S3cr"));
    }
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
            for (index, (sent, view)) in views.into_iter().enumerate() {
                if view != previous && (chunk > 64 || index % 8 == 0) {
                    let expected = reply_text(visible(&text[..sent]));
                    assert_eq!(view, expected, "{piece} by {chunk} at {sent}");
                }
                previous = view;
            }
        }
    }
}

fn work_and_longest_wait(text: &str, chunk: usize) -> (usize, usize) {
    let mut stream = ReplyStream::default();
    let (mut previous, mut since, mut longest) = (String::new(), 0, 0);
    for piece in text.as_bytes().chunks(chunk) {
        stream.push(std::str::from_utf8(piece).unwrap()).unwrap();
        let view = stream.streaming();
        since += piece.len();
        if view != previous {
            longest = longest.max(since);
            since = 0;
        }
        previous = view;
    }
    (stream.redacted_bytes, longest)
}

#[test]
fn streaming_a_plain_reply_redacts_each_byte_a_few_times() {
    let text = "plain words in a long reply, line after line.\n".repeat(256 * 1024 / 47);
    let (work, longest) = work_and_longest_wait(&text, 16);
    assert!(
        work <= 12 * text.len(),
        "{work} bytes redacted for {}",
        text.len()
    );
    assert!(
        longest <= (LOOKAHEAD + 2 * KEEP_EVERY) / REFRESH_SHARE + 64,
        "the view waited {longest} bytes"
    );
}

#[test]
fn streaming_work_stays_linear_and_the_view_moves_once_a_construct_closes() {
    let filler = "word and a /srv/path/x line\n";
    for (opening, closing) in [
        ("password=\"", "\" then"),
        (
            "-----BEGIN PRIVATE KEY-----\n",
            "-----END PRIVATE KEY-----\n",
        ),
        ("config = {\"nested\": [", "]}\n"),
        (
            "-----BEGIN CERTIFICATE-----\n",
            "-----END CERTIFICATE-----\n",
        ),
    ] {
        let text = format!(
            "{}{opening}{}{closing}{}",
            filler.repeat(64),
            "word and more words ".repeat(40),
            filler.repeat(256 * 1024 / filler.len())
        );
        let (work, longest) = work_and_longest_wait(&text, 256);
        assert!(work <= 32 * text.len(), "{opening}: {work} bytes redacted");
        assert!(
            longest <= 2 * LOOKAHEAD,
            "{opening}: the view waited {longest} bytes"
        );
    }
    for opening in [
        "--password 'hunter2 rejected; it isn't valid ",
        "token=\"open ",
    ] {
        let text = format!("{opening}{}", filler.repeat(256 * 1024 / filler.len()));
        let (work, _) = work_and_longest_wait(&text, 256);
        assert!(work <= 32 * text.len(), "{opening}: {work} bytes redacted");
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
    let mut stream = ReplyStream::new(&"token=abc ".repeat(ITEM_BYTES / 10));
    let streaming = stream.streaming();
    let whole = stream.finished();
    for shown in [&streaming, &whole] {
        assert!(shown.len() <= ITEM_BYTES, "{} bytes", shown.len());
        assert!(shown.ends_with('\u{2026}'));
    }
    assert!(
        whole.starts_with("[redacted] [redacted] "),
        "{}",
        &whole[..80]
    );
}

/// A small xorshift generator, so each case is reproducible from its seed.
struct Cases(u64);

impl Cases {
    fn below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % bound as u64) as usize
    }

    fn secret(&mut self) -> String {
        const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
        (0..20)
            .map(|_| char::from(ALPHABET[self.below(ALPHABET.len())]))
            .collect()
    }
}

#[test]
fn every_view_of_a_randomly_split_reply_hides_what_the_whole_reply_hides() {
    let home = std::env::home_dir().expect("home folder");
    let home = home.to_str().expect("UTF-8 home folder").to_owned();
    for seed in 1..=12u64 {
        let mut cases = Cases(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let mut text = String::new();
        let mut secrets = Vec::new();
        while text.len() < 3 * LOOKAHEAD {
            let secret = cases.secret();
            let leads = [
                "", "回答", "パス", "，", "。", "：", "（", "「", "、", "«", "’", "\u{3000}",
                "\u{a0}", "—",
            ];
            let lead = leads[cases.below(leads.len())];
            text.push_str(lead);
            let piece = match cases.below(13) {
                0 => format!("token={secret} "),
                1 => format!("password: {secret}\n"),
                2 => format!("Authorization: Bearer {secret}\n"),
                3 => format!("postgres://admin:{secret}@db/app "),
                4 => format!("export API_KEY=\"{secret} and more\"\n"),
                5 => format!("-----BEGIN PRIVATE KEY-----\n{secret}\n-----END PRIVATE KEY-----\n"),
                6 => format!("--password {secret} "),
                7 => format!("{home}/project/src/main.rs and /srv/alice/x "),
                8 => "这是一段没有空格的中文回答。".to_owned(),
                9 => "\"a quoted phrase\" in prose ".to_owned(),
                10 => match cases.below(3) {
                    0 => format!("password=密码—{secret} "),
                    1 => format!("Bearer 密码{secret}，完 "),
                    _ => format!("https://alice:密{secret}—x@db/app "),
                },
                _ => "plain words in a long reply, line after line.\n".repeat(1 + cases.below(40)),
            };
            if piece.contains(&secret) {
                secrets.push(secret);
            }
            text.push_str(&piece);
        }
        let whole = reply_text(&text);
        for secret in &secrets {
            let at = text.find(secret.as_str()).unwrap_or(0);
            assert!(
                !whole.contains(secret.as_str()),
                "seed {seed}: {:?} shown whole",
                &text[text.floor_char_boundary(at.saturating_sub(80))
                    ..text.ceil_char_boundary(at + 40)]
            );
        }
        let mut stream = ReplyStream::default();
        let mut sent = 0;
        let mut views = 0;
        let mut previous = String::new();
        while sent < text.len() {
            let mut end = (sent + 1 + cases.below(300)).min(text.len());
            while !text.is_char_boundary(end) {
                end += 1;
            }
            stream.push(&text[sent..end]).unwrap();
            sent = end;
            let view = stream.streaming();
            let same = view
                .bytes()
                .zip(previous.bytes())
                .take_while(|(new, old)| new == old)
                .count();
            let mut from = same.saturating_sub(secrets[0].len());
            while !view.is_char_boundary(from) {
                from -= 1;
            }
            for secret in &secrets {
                for start in 0..=secret.len() - 6 {
                    assert!(
                        !view[from..].contains(&secret[start..start + 6]),
                        "seed {seed} at {sent}: part of {secret} shown"
                    );
                }
            }
            previous = view.clone();
            views += 1;
            if views % 16 == 0 {
                let shown = &text[..stream.shown_end];
                assert_eq!(view, reply_text(shown), "seed {seed} at {sent}");
            }
        }
        assert_eq!(stream.finished(), whole, "seed {seed}");
    }
}

#[test]
fn a_finished_reply_is_the_whole_text_redacted_at_once() {
    let text = format!(
        "{}-----BEGIN RSA PRIV{}",
        "plain words in a long reply.\n".repeat(2 * LOOKAHEAD / 29),
        "ATE KEY-----\nMIIEvAIBADANBgkqhkiG9w0BAQEFAASC\n"
    );
    let (_, mut stream) = streamed(&text, 13);
    assert_eq!(stream.finished(), reply_text(&text));
}

#[test]
fn a_value_after_a_kept_prefix_keeps_the_key_or_header_before_it() {
    let filler = "plain words keep the reply long enough to keep a prefix. ".repeat(400);
    for opener in ["Authorization: Bearer", "--password", "token:"] {
        let gap = " ".repeat(2 * LOOKAHEAD);
        let text = format!("{filler}\n{opener}{gap}hunter2secret done\n{filler}");
        for chunk in [61, 1500] {
            let (views, mut stream) = streamed(&text, chunk);
            for (sent, view) in views {
                assert!(!view.contains("hunter2"), "{opener} by {chunk} at {sent}");
            }
            assert!(!stream.finished().contains("hunter2"), "{opener}");
        }
    }
}
