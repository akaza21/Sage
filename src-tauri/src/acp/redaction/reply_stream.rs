//! A harness reply streamed in deltas, kept as sent and shown redacted.
//!
//! Every harness assembles its reply here and nowhere else redacts it, so a
//! secret, a path or the home folder that straddles deltas is judged whole.
//! While the reply streams, the word still arriving is held back: it may be
//! the start of a secret, such as `/srv/alice:hun`. Text written without
//! spaces, as Chinese or Japanese is, is shown up to its last such character,
//! since no secret is spelled in it. Whenever the reply ends, completed or
//! not, it is shown whole.
//!
//! Redacting the whole reply on every delta would cost time quadratic in its
//! length, so a prefix is redacted once and kept. A prefix is kept only when
//! redacting it alone, and the text after it alone, gives exactly what
//! redacting both together gives, with [`LOOKAHEAD`] bytes after it, as far
//! as any rule reads to decide a span. A construct still open there, such as
//! an unclosed quote, fails that check; the stream then redacts again only
//! once the text after the kept prefix has doubled, showing the last view
//! until then, so the work stays linear in the reply's length.

use super::{clip_display, reply_text, spans::MAX_SPAN_BYTES};

/// Bytes that must follow a kept prefix before it is kept.
const LOOKAHEAD: usize = MAX_SPAN_BYTES;
/// Unkept text that prompts an attempt to keep a prefix.
const KEEP_AFTER: usize = 2 * LOOKAHEAD;
/// The longest word held back; a longer one is shown up to this many bytes
/// from its end.
const HELD_MAX: usize = LOOKAHEAD;
/// Characters that join a word into a secret or a path, so a word holding one
/// is held back whole, whatever script it is written in.
const TOKEN_JOINERS: [char; 5] = [':', '=', '@', '/', '\\'];

#[derive(Debug, Default)]
pub(crate) struct ReplyStream {
    raw: String,
    kept: usize,
    kept_shown: String,
    shown: String,
    shown_end: usize,
    next_redaction: usize,
    #[cfg(test)]
    redacted_bytes: usize,
}

impl ReplyStream {
    pub(crate) fn new(text: &str) -> Self {
        Self {
            raw: text.to_owned(),
            ..Self::default()
        }
    }

    /// Bytes as sent so far.
    pub(crate) fn len(&self) -> usize {
        self.raw.len()
    }

    pub(crate) fn push(&mut self, delta: &str) -> Result<(), std::collections::TryReserveError> {
        self.raw.try_reserve(delta.len())?;
        self.raw.push_str(delta);
        Ok(())
    }

    /// The reply while it streams: redacted, without the word still arriving.
    pub(crate) fn streaming(&mut self) -> String {
        let end = self.visible_end();
        if end > self.shown_end && end - self.kept >= self.next_redaction {
            let tail = self.redact(self.kept, end);
            self.shown = format!("{}{tail}", self.kept_shown);
            self.shown_end = end;
            self.try_keep(end, &tail);
        }
        self.shown.clone()
    }

    /// The whole reply, redacted, once the harness ends it for any reason or
    /// stops it to call a tool. A later view shows no less of it.
    pub(crate) fn finished(&mut self) -> String {
        let tail = self.redact(self.kept, self.raw.len());
        self.shown = format!("{}{tail}", self.kept_shown);
        self.shown_end = self.raw.len();
        self.shown.clone()
    }

    /// [`Self::finished`], clipped to `maximum` bytes when redaction markers
    /// made it longer than the text as sent.
    pub(crate) fn finished_within(&mut self, maximum: usize) -> String {
        clip_display(&self.finished(), maximum)
    }

    fn visible_end(&self) -> usize {
        let mut from = self.raw.len().saturating_sub(HELD_MAX).max(self.kept);
        while !self.raw.is_char_boundary(from) {
            from += 1;
        }
        let window = &self.raw[from..];
        let word = after_last(window, char::is_whitespace);
        let held = &window[word..];
        if held.contains(TOKEN_JOINERS) {
            return from + word;
        }
        from + word + after_last(held, ends_a_token)
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
        let unkept = end - self.kept;
        if unkept < KEEP_AFTER {
            return;
        }
        if let Some(boundary) = self.boundary(end) {
            let head = self.redact(self.kept, boundary);
            let rest = self.redact(boundary, end);
            if tail.len() == head.len() + rest.len()
                && tail.starts_with(&head)
                && tail.ends_with(&rest)
            {
                self.kept_shown.push_str(&head);
                self.kept = boundary;
                self.next_redaction = 0;
                return;
            }
        }
        self.next_redaction = unkept.saturating_mul(2);
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
