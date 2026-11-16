//! A harness reply streamed in deltas, kept as sent and shown redacted.
//!
//! Every harness assembles its reply here and nowhere else redacts it, so a
//! secret, a path or the home folder that straddles deltas is judged whole.
//! While the reply streams, the word still arriving is held back: it may be
//! the start of a secret, such as `/srv/alice:hun`. Text written without
//! spaces, as Chinese or Japanese is, is shown up to its last such character,
//! since no secret is spelled in it, unless the word holds a character that
//! joins it into a secret or a path. Whenever the reply ends, completed or
//! not, the whole text as sent is redacted at once and shown, clipped to what
//! an item may hold, so what is kept never depends on how it streamed.
//!
//! Redacting the whole reply on every delta would cost time quadratic in its
//! length, so while it streams a redacted prefix is kept and only the text
//! after it is redacted again. A prefix is kept only when redacting it alone,
//! and the [`LOOKAHEAD`] bytes or more after it alone, gives exactly what
//! redacting both together gives; no rule reads further than that to decide
//! a span. The view is redacted again once the text since the last view is
//! an eighth of the unkept text, and a prefix is tried at most every
//! [`KEEP_EVERY`] bytes or eighth of the unkept text, so the work stays
//! linear in the reply's length.

use super::{clip_display, reply_text, spans::MAX_SPAN_BYTES};
use sage_core::limits::LIMITS;

/// Bytes that must follow a kept prefix, as far as any rule reads to decide
/// a span.
const LOOKAHEAD: usize = MAX_SPAN_BYTES;
/// The fewest bytes between attempts to keep a prefix.
const KEEP_EVERY: usize = 4096;
/// The share of the unkept text that must arrive before it is redacted again.
const REFRESH_SHARE: usize = 8;
/// Characters that join a word into a secret or a path, so a word holding one
/// is held back whole, whatever script it is written in.
const TOKEN_JOINERS: [char; 5] = [':', '=', '@', '/', '\\'];
const ITEM_BYTES: usize = smaller(
    LIMITS.wire.normalized_item_bytes,
    LIMITS.wire.item_bytes_per_turn,
) as usize;

const fn smaller(left: u64, right: u64) -> u64 {
    if left < right { left } else { right }
}

#[derive(Debug, Default)]
pub(crate) struct ReplyStream {
    raw: String,
    word_start: usize,
    word_joined: bool,
    wide_end: usize,
    kept: usize,
    kept_shown: String,
    shown: String,
    shown_end: usize,
    attempted_end: usize,
    #[cfg(test)]
    redacted_bytes: usize,
}

impl ReplyStream {
    pub(crate) fn new(text: &str) -> Self {
        let mut stream = Self::default();
        stream.raw.push_str(text);
        stream.track(0);
        stream
    }

    /// Bytes as sent so far.
    pub(crate) fn len(&self) -> usize {
        self.raw.len()
    }

    pub(crate) fn push(&mut self, delta: &str) -> Result<(), std::collections::TryReserveError> {
        self.raw.try_reserve(delta.len())?;
        let from = self.raw.len();
        self.raw.push_str(delta);
        self.track(from);
        Ok(())
    }

    /// The reply while it streams: redacted, without the word still arriving.
    pub(crate) fn streaming(&mut self) -> String {
        let end = self.visible_end();
        let unkept = end - self.kept;
        if end > self.shown_end && end - self.shown_end >= unkept / REFRESH_SHARE {
            let tail = self.redact(self.kept, end);
            self.shown = self.after_kept(&tail);
            self.shown_end = end;
            if unkept >= LOOKAHEAD + KEEP_EVERY
                && end - self.attempted_end >= KEEP_EVERY.max(unkept / REFRESH_SHARE)
            {
                self.attempted_end = end;
                self.try_keep(end, &tail);
            }
        }
        self.shown.clone()
    }

    /// The whole reply, redacted at once, when the harness ends it for any
    /// reason or stops it to call a tool. A later view shows no less of it.
    pub(crate) fn finished(&mut self) -> String {
        let whole = self.redact(0, self.raw.len());
        self.shown = clip_display(&whole, ITEM_BYTES);
        self.shown_end = self.raw.len();
        self.shown.clone()
    }

    fn track(&mut self, from: usize) {
        for (index, character) in self.raw[from..].char_indices() {
            let after = from + index + character.len_utf8();
            if character.is_whitespace() {
                self.word_start = after;
                self.word_joined = false;
                self.wide_end = after;
            } else if TOKEN_JOINERS.contains(&character) {
                self.word_joined = true;
            } else if ends_a_token(character) {
                self.wide_end = after;
            }
        }
    }

    fn visible_end(&self) -> usize {
        let end = if self.word_joined {
            self.word_start
        } else {
            self.wide_end
        };
        end.max(self.kept)
    }

    fn after_kept(&self, tail: &str) -> String {
        let shown = format!("{}{tail}", self.kept_shown);
        clip_display(&shown, ITEM_BYTES)
    }

    fn redact(&mut self, start: usize, end: usize) -> String {
        #[cfg(test)]
        {
            self.redacted_bytes += end - start;
        }
        if start == end {
            return String::new();
        }
        reply_text(&self.raw[start..end])
    }

    fn try_keep(&mut self, end: usize, tail: &str) {
        let Some(boundary) = self.boundary(end) else {
            return;
        };
        let head = self.redact(self.kept, boundary);
        let rest = self.redact(boundary, end);
        if tail.len() == head.len() + rest.len() && tail.starts_with(&head) && tail.ends_with(&rest)
        {
            self.kept_shown.push_str(&head);
            self.kept = boundary;
        }
    }

    /// The end of the last line, or failing that the last word or wide
    /// character, that has [`LOOKAHEAD`] bytes after it.
    fn boundary(&self, end: usize) -> Option<usize> {
        let mut limit = end.checked_sub(LOOKAHEAD)?;
        while !self.raw.is_char_boundary(limit) {
            limit -= 1;
        }
        let window = self.raw.get(self.kept..limit)?;
        let breaks: [fn(char) -> bool; 3] = [
            |character| character == '\n',
            char::is_whitespace,
            ends_a_token,
        ];
        let after = breaks
            .into_iter()
            .map(|matches| after_last(window, matches))
            .find(|after| *after > 0)
            .unwrap_or(0);
        (after > 0).then_some(self.kept + after)
    }
}

/// The byte just past the last character in `text` that matches, or 0.
fn after_last(text: &str, matches: impl Fn(char) -> bool) -> usize {
    text.char_indices()
        .rfind(|(_, character)| matches(*character))
        .map_or(0, |(index, character)| index + character.len_utf8())
}

/// A character no secret is spelled with: an ideograph, kana, hangul or other
/// wide script, or punctuation and symbols outside ASCII.
fn ends_a_token(character: char) -> bool {
    !character.is_ascii() && (character >= '\u{2e80}' || !character.is_alphanumeric())
}

#[cfg(test)]
#[path = "reply_stream_tests.rs"]
mod tests;
