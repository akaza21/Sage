//! A harness reply streamed in deltas, kept as sent and shown redacted.
//!
//! Every harness assembles its reply here and nowhere else redacts it, so a
//! secret, a path or the home folder that straddles deltas is judged whole.
//! While the reply streams, only text up to its last whitespace is shown: a
//! word still arriving, such as `/srv/alice:hun`, may be the start of a
//! secret. The finished reply is shown whole.
//!
//! Redacting the whole reply on every delta would cost time quadratic in its
//! length, so a prefix is redacted once and kept. A prefix is kept only when
//! redacting it alone, and the text after it alone, gives exactly what
//! redacting both together gives, with at least [`LOOKAHEAD`] bytes after it:
//! anything still open there, such as an unclosed quote or private key, fails
//! closed over the whole text and so cannot pass that check.

use super::reply_text;

/// Bytes that must follow a kept prefix before it is kept.
const LOOKAHEAD: usize = 1024;
/// Unkept text that prompts an attempt to keep a prefix.
const KEEP_AFTER: usize = 4 * LOOKAHEAD;

#[derive(Debug, Default)]
pub(crate) struct ReplyStream {
    raw: String,
    kept: usize,
    kept_shown: String,
    next_attempt: usize,
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

    /// The reply while it streams: redacted, and only up to its last whitespace.
    pub(crate) fn streaming(&mut self) -> String {
        let end = self.raw[self.kept..]
            .char_indices()
            .rfind(|(_, character)| character.is_whitespace())
            .map_or(self.kept, |(index, character)| {
                self.kept + index + character.len_utf8()
            });
        let tail = self.redact(self.kept, end);
        let shown = format!("{}{tail}", self.kept_shown);
        self.try_keep(end, &tail);
        shown
    }

    /// The whole reply, redacted, once the harness says it is complete.
    pub(crate) fn finished(&mut self) -> String {
        let tail = self.redact(self.kept, self.raw.len());
        format!("{}{tail}", self.kept_shown)
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
        if unkept < KEEP_AFTER.max(self.next_attempt) {
            return;
        }
        self.next_attempt = unkept + KEEP_AFTER;
        let Some(boundary) = self.boundary(end) else {
            return;
        };
        let head = self.redact(self.kept, boundary);
        let rest = self.redact(boundary, end);
        if tail.len() == head.len() + rest.len() && tail.starts_with(&head) && tail.ends_with(&rest)
        {
            self.kept_shown.push_str(&head);
            self.kept = boundary;
            self.next_attempt = 0;
        }
    }

    /// The end of the last line, or failing that the last word, that has
    /// [`LOOKAHEAD`] bytes after it.
    fn boundary(&self, end: usize) -> Option<usize> {
        let mut limit = end.checked_sub(LOOKAHEAD)?;
        while !self.raw.is_char_boundary(limit) {
            limit -= 1;
        }
        let window = self.raw.get(self.kept..limit)?;
        let (index, character) = window
            .char_indices()
            .rfind(|(_, character)| *character == '\n')
            .or_else(|| {
                window
                    .char_indices()
                    .rfind(|(_, character)| character.is_whitespace())
            })?;
        Some(self.kept + index + character.len_utf8())
    }
}

#[cfg(test)]
#[path = "reply_stream_tests.rs"]
mod tests;
