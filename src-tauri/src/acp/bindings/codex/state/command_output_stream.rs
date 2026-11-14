//! Aggregate admission must use the truthful `retained_bytes` charge rather
//! than an assumed total heap size.

use std::{collections::VecDeque, fmt, mem::size_of};

use crate::acp::{
    bindings::codex::incoming::command_output_token,
    command_output::{RetainedCommandOutput, retain_command_output},
};
use sage_core::limits::{LIMITS, RETAINED_TOOL_OUTPUT_LINES};

const REDACTED: &str = "[redacted]";
const SAFE_CHUNK_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandOutputStreamError {
    Allocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SafeChunk {
    text: String,
    start: usize,
}

impl SafeChunk {
    fn remaining(&self) -> &str {
        &self.text[self.start..]
    }

    fn remaining_len(&self) -> usize {
        self.text.len().saturating_sub(self.start)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CommandOutputStream {
    chunks: VecDeque<SafeChunk>,
    safe_bytes: usize,
    complete_lines: usize,
    first_line_bytes: Option<usize>,
    ends_with_newline: bool,
    truncated: bool,
    pending: String,
    redact_next: bool,
    discarding_token: bool,
    #[cfg(test)]
    fail_next_chunk_reserve: bool,
}

impl fmt::Debug for CommandOutputStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommandOutputStream")
            .field("safe_bytes", &self.safe_bytes)
            .field("complete_lines", &self.complete_lines)
            .field("pending_bytes", &self.pending.len())
            .field("truncated", &self.truncated)
            .field("redact_next", &self.redact_next)
            .field("discarding_token", &self.discarding_token)
            .finish_non_exhaustive()
    }
}

impl Default for CommandOutputStream {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandOutputStream {
    pub(crate) fn new() -> Self {
        Self {
            chunks: VecDeque::new(),
            safe_bytes: 0,
            complete_lines: 0,
            first_line_bytes: None,
            ends_with_newline: true,
            truncated: false,
            pending: String::new(),
            redact_next: false,
            discarding_token: false,
            #[cfg(test)]
            fail_next_chunk_reserve: false,
        }
    }

    /// Appends a delta without publishing incomplete raw tokens. For
    /// validate-before-commit callers, clone, append, validate
    /// `retained_bytes`, then commit the clone.
    pub(crate) fn append(&mut self, delta: &str) -> Result<(), CommandOutputStreamError> {
        let mut token_start = 0;
        for (index, character) in delta.char_indices() {
            if !character.is_whitespace() {
                continue;
            }
            self.append_pending(&delta[token_start..index])?;
            self.finish_token()?;
            let whitespace_end = index + character.len_utf8();
            self.append_safe(&delta[index..whitespace_end])?;
            token_start = whitespace_end;
        }
        self.append_pending(&delta[token_start..])
    }

    pub(crate) fn finish(&mut self) -> Result<(), CommandOutputStreamError> {
        self.finish_token()
    }

    /// Replaces state without mutating the prior value until all fallible work
    /// has succeeded.
    pub(crate) fn replace_complete(
        &mut self,
        aggregate: &str,
    ) -> Result<(), CommandOutputStreamError> {
        let replacement = Self::from_complete(aggregate)?;
        *self = replacement;
        Ok(())
    }

    /// Replaces a nonterminal aggregate while preserving its private trailing
    /// token for a later streamed delta or terminal flush.
    pub(crate) fn replace_unfinished(
        &mut self,
        aggregate: &str,
    ) -> Result<(), CommandOutputStreamError> {
        let mut replacement = Self::new();
        replacement.append(aggregate)?;
        *self = replacement;
        Ok(())
    }

    pub(crate) fn from_complete(aggregate: &str) -> Result<Self, CommandOutputStreamError> {
        let mut stream = Self::new();
        stream.append(aggregate)?;
        stream.finish()?;
        Ok(stream)
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::new();
    }

    pub(crate) fn retained_output(
        &self,
    ) -> Result<RetainedCommandOutput, CommandOutputStreamError> {
        let mut text = String::new();
        text.try_reserve_exact(self.safe_bytes)
            .map_err(|_| CommandOutputStreamError::Allocation)?;
        for chunk in &self.chunks {
            text.push_str(chunk.remaining());
        }
        let mut retained =
            retain_command_output(&text).ok_or(CommandOutputStreamError::Allocation)?;
        retained.truncated |= self.truncated;
        Ok(retained)
    }

    pub(crate) fn render(&self) -> Result<String, CommandOutputStreamError> {
        self.retained_output()?
            .display_text()
            .ok_or(CommandOutputStreamError::Allocation)
    }

    /// Charges all physically retained UTF-8 text, including a consumed front
    /// prefix still owned by a rolling chunk, plus private pending-token bytes.
    /// Callers stage the whole delta on a clone and discard that clone on any
    /// error before using this charge for snapshot admission.
    pub(crate) fn retained_bytes(&self) -> usize {
        self.chunks.iter().fold(self.pending.len(), |total, chunk| {
            total.saturating_add(chunk.text.len())
        })
    }

    /// Reports observed string/deque backing capacity for diagnostics. It does
    /// not include allocator metadata. A clone has separate backing storage;
    /// projection creates bounded marker-free/display strings on demand.
    pub(crate) fn allocated_bytes(&self) -> usize {
        let chunk_bytes = self.chunks.iter().fold(0_usize, |total, chunk| {
            total.saturating_add(chunk.text.capacity())
        });
        chunk_bytes
            .saturating_add(self.pending.capacity())
            .saturating_add(
                self.chunks
                    .capacity()
                    .saturating_mul(size_of::<SafeChunk>()),
            )
    }

    fn append_pending(&mut self, value: &str) -> Result<(), CommandOutputStreamError> {
        if value.is_empty() || self.discarding_token {
            return Ok(());
        }
        let maximum = LIMITS.wire.retained_tool_output_bytes as usize;
        let accepted = prefix_at_most(value, maximum.saturating_sub(self.pending.len()));
        if !accepted.is_empty() {
            self.pending
                .try_reserve_exact(accepted.len())
                .map_err(|_| CommandOutputStreamError::Allocation)?;
            self.pending.push_str(accepted);
        }
        if accepted.len() < value.len() {
            self.pending = String::new();
            self.discarding_token = true;
            self.truncated = true;
            self.append_safe(REDACTED)?;
        }
        Ok(())
    }

    fn finish_token(&mut self) -> Result<(), CommandOutputStreamError> {
        if self.discarding_token {
            self.discarding_token = false;
            self.redact_next = false;
            return Ok(());
        }
        if self.pending.is_empty() {
            return Ok(());
        }
        let pending = std::mem::take(&mut self.pending);
        let (replacement, redact_next) = command_output_token(&pending, self.redact_next);
        if let Err(error) = self.append_safe(replacement) {
            self.pending = pending;
            return Err(error);
        }
        self.redact_next = redact_next;
        Ok(())
    }

    fn append_safe(&mut self, mut value: &str) -> Result<(), CommandOutputStreamError> {
        while !value.is_empty() {
            let part = prefix_at_most(value, SAFE_CHUNK_BYTES);
            self.append_safe_chunk(part)?;
            value = &value[part.len()..];
        }
        Ok(())
    }

    fn append_safe_chunk(&mut self, value: &str) -> Result<(), CommandOutputStreamError> {
        let next_safe_bytes = self
            .safe_bytes
            .checked_add(value.len())
            .ok_or(CommandOutputStreamError::Allocation)?;
        let next_complete_lines = self
            .complete_lines
            .checked_add(value.bytes().filter(|byte| *byte == b'\n').count())
            .ok_or(CommandOutputStreamError::Allocation)?;
        let next_first_line_bytes = if self.first_line_bytes.is_none() {
            value.find('\n').and_then(|index| {
                next_safe_bytes
                    .checked_sub(value.len())
                    .and_then(|bytes| bytes.checked_add(index + 1))
            })
        } else {
            self.first_line_bytes
        };
        self.append_chunk(value)?;
        self.safe_bytes = next_safe_bytes;
        self.complete_lines = next_complete_lines;
        self.first_line_bytes = next_first_line_bytes;
        self.ends_with_newline = value.ends_with('\n');
        self.enforce_output_bounds();
        Ok(())
    }

    fn append_chunk(&mut self, value: &str) -> Result<(), CommandOutputStreamError> {
        let tail_room = self
            .chunks
            .back()
            .map_or(0, |chunk| SAFE_CHUNK_BYTES.saturating_sub(chunk.text.len()));
        let tail_part = prefix_at_most(value, tail_room);
        if tail_part.len() == value.len() && !tail_part.is_empty() {
            let Some(tail) = self.chunks.back_mut() else {
                return Err(CommandOutputStreamError::Allocation);
            };
            tail.text
                .try_reserve_exact(tail_part.len())
                .map_err(|_| CommandOutputStreamError::Allocation)?;
            tail.text.push_str(tail_part);
            return Ok(());
        }

        let remaining = &value[tail_part.len()..];
        let mut next_text = String::new();
        next_text
            .try_reserve_exact(remaining.len())
            .map_err(|_| CommandOutputStreamError::Allocation)?;
        next_text.push_str(remaining);
        self.reserve_chunk_slot()?;
        if !tail_part.is_empty() {
            let Some(tail) = self.chunks.back_mut() else {
                return Err(CommandOutputStreamError::Allocation);
            };
            tail.text
                .try_reserve_exact(tail_part.len())
                .map_err(|_| CommandOutputStreamError::Allocation)?;
            tail.text.push_str(tail_part);
        }
        self.chunks.push_back(SafeChunk {
            text: next_text,
            start: 0,
        });
        Ok(())
    }

    fn reserve_chunk_slot(&mut self) -> Result<(), CommandOutputStreamError> {
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_chunk_reserve) {
            return Err(CommandOutputStreamError::Allocation);
        }
        self.chunks
            .try_reserve(1)
            .map_err(|_| CommandOutputStreamError::Allocation)
    }

    #[cfg(test)]
    fn fail_next_chunk_reserve(&mut self) {
        self.fail_next_chunk_reserve = true;
    }

    fn enforce_output_bounds(&mut self) {
        while self.logical_lines() > RETAINED_TOOL_OUTPUT_LINES {
            self.drop_first_line();
            self.truncated = true;
        }
        let maximum = LIMITS.wire.retained_tool_output_bytes as usize;
        while self.safe_bytes > maximum {
            if self
                .first_line_bytes
                .is_some_and(|bytes| bytes < self.safe_bytes)
            {
                self.drop_first_line();
            } else {
                self.drop_prefix_at_least(self.safe_bytes.saturating_sub(maximum));
            }
            self.truncated = true;
        }
    }

    fn drop_first_line(&mut self) {
        let Some(bytes) = self.first_line_bytes else {
            return;
        };
        self.drop_prefix(bytes);
        self.complete_lines = self.complete_lines.saturating_sub(1);
        self.refresh_first_line();
    }

    fn drop_prefix_at_least(&mut self, mut bytes: usize) {
        while bytes > 0 {
            let Some(chunk) = self.chunks.front() else {
                return;
            };
            let part = chunk.remaining();
            if bytes >= part.len() {
                bytes = bytes.saturating_sub(part.len());
                self.drop_prefix(part.len());
                continue;
            }
            while bytes < part.len() && !part.is_char_boundary(bytes) {
                bytes += 1;
            }
            self.drop_prefix(bytes);
            return;
        }
    }

    fn drop_prefix(&mut self, mut bytes: usize) {
        while bytes > 0 {
            let Some(front) = self.chunks.front_mut() else {
                return;
            };
            let removed = front.remaining_len().min(bytes);
            front.start += removed;
            self.safe_bytes = self.safe_bytes.saturating_sub(removed);
            bytes = bytes.saturating_sub(removed);
            if front.start == front.text.len() {
                self.chunks.pop_front();
            }
        }
        if self.safe_bytes == 0 {
            self.ends_with_newline = true;
        }
    }

    fn refresh_first_line(&mut self) {
        self.first_line_bytes = None;
        let mut offset = 0_usize;
        for chunk in &self.chunks {
            let part = chunk.remaining();
            if let Some(index) = part.find('\n') {
                self.first_line_bytes = offset.checked_add(index + 1);
                return;
            }
            offset = offset.saturating_add(part.len());
        }
    }

    fn logical_lines(&self) -> usize {
        self.complete_lines
            .saturating_add(usize::from(self.safe_bytes > 0 && !self.ends_with_newline))
    }
}

fn prefix_at_most(value: &str, maximum: usize) -> &str {
    let mut end = value.len().min(maximum);
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(stream: &CommandOutputStream) -> String {
        stream
            .render()
            .unwrap_or_else(|error| panic!("render failed: {error:?}"))
    }

    fn retained(stream: &CommandOutputStream) -> RetainedCommandOutput {
        stream
            .retained_output()
            .unwrap_or_else(|error| panic!("snapshot failed: {error:?}"))
    }

    #[test]
    fn split_bearer_and_opaque_value_never_publish_the_secret() {
        let mut stream = CommandOutputStream::new();
        stream.append("Bearer").unwrap();
        assert_eq!(display(&stream), "");
        stream.append(" opaque-credential").unwrap();
        assert!(!display(&stream).contains("opaque-credential"));
        stream.finish().unwrap();
        assert_eq!(display(&stream), "Bearer [redacted]");
    }

    #[test]
    fn split_paths_never_publish_an_intermediate_prefix() {
        for parts in [["/tmp", "/secret "], ["file:", "///private/key "]] {
            let mut stream = CommandOutputStream::new();
            stream.append(parts[0]).unwrap();
            assert_eq!(display(&stream), "");
            stream.append(parts[1]).unwrap();
            let rendered = display(&stream);
            assert_eq!(rendered, "[path] ");
            assert!(!rendered.contains("secret"));
            assert!(!rendered.contains("private"));
        }
    }

    #[test]
    fn waits_for_complete_generic_tokens_and_preserves_tabs_and_crlf() {
        let mut stream = CommandOutputStream::new();
        stream.append("hel").unwrap();
        assert_eq!(display(&stream), "");
        stream.append("lo\tworld\r\n").unwrap();
        assert_eq!(display(&stream), "hello\tworld\r\n");
        stream.append("final").unwrap();
        assert_eq!(display(&stream), "hello\tworld\r\n");
        stream.finish().unwrap();
        assert_eq!(display(&stream), "hello\tworld\r\nfinal");
    }

    #[test]
    fn utf8_scalars_progress_when_a_tail_has_insufficient_room() {
        for scalar in ["é", "€", "😀"] {
            let aggregate = format!("{}{} ", "a".repeat(4095), scalar);
            let stream = CommandOutputStream::from_complete(&aggregate).unwrap();
            assert_eq!(retained(&stream).text, aggregate);
        }
    }

    #[test]
    fn debug_never_includes_raw_pending_output() {
        let mut stream = CommandOutputStream::new();
        stream.append("Bearer ").unwrap();
        stream.append("opaque-credential-/private/path").unwrap();
        let debug = format!("{stream:?}");
        assert!(!debug.contains("opaque-credential"));
        assert!(!debug.contains("/private/path"));
        assert!(debug.contains("pending_bytes"));
    }

    #[test]
    fn failed_chunk_reservation_leaves_the_stream_coherent_for_staged_discard() {
        let original =
            CommandOutputStream::from_complete(&format!("{} ", "x".repeat(4096))).unwrap();
        let before = retained(&original);
        let before_bytes = original.retained_bytes();
        let mut staged = original.clone();
        staged.fail_next_chunk_reserve();
        assert_eq!(
            staged.append(&format!("{} ", "n".repeat(4095))),
            Err(CommandOutputStreamError::Allocation)
        );
        assert_eq!(retained(&original), before);
        assert_eq!(original.retained_bytes(), before_bytes);
        assert!(retained(&staged).text.ends_with(&"n".repeat(4095)));
    }

    #[test]
    fn exact_and_plus_one_pending_tokens_are_bounded_and_redacted_once() {
        let maximum = LIMITS.wire.retained_tool_output_bytes as usize;
        let mut exact = CommandOutputStream::new();
        exact.append(&"a".repeat(maximum)).unwrap();
        assert_eq!(exact.retained_bytes(), maximum);
        assert_eq!(display(&exact), "");
        exact.finish().unwrap();
        assert_eq!(retained(&exact).text.len(), maximum);
        assert!(!retained(&exact).truncated);

        let mut over = CommandOutputStream::new();
        over.append(&"a".repeat(maximum + 1)).unwrap();
        assert!(over.retained_bytes() <= maximum);
        assert!(retained(&over).truncated);
        assert_eq!(retained(&over).text.matches(REDACTED).count(), 1);
        over.append(" more ").unwrap();
        assert_eq!(retained(&over).text.matches(REDACTED).count(), 1);
    }

    #[test]
    fn exact_and_plus_one_output_bounds_keep_one_display_marker() {
        let maximum = LIMITS.wire.retained_tool_output_bytes as usize;
        let mut exact = CommandOutputStream::from_complete(&"x".repeat(maximum)).unwrap();
        assert_eq!(display(&exact).len(), maximum);
        assert!(!retained(&exact).truncated);
        exact.append(" ").unwrap();
        let rendered = display(&exact);
        assert!(retained(&exact).truncated);
        assert_eq!(rendered.matches("[output truncated]").count(), 1);
        exact.append("next ").unwrap();
        assert_eq!(display(&exact).matches("[output truncated]").count(), 1);

        let lines = (0..=RETAINED_TOOL_OUTPUT_LINES)
            .map(|index| format!("line-{index}\n"))
            .collect::<String>();
        let stream = CommandOutputStream::from_complete(&lines).unwrap();
        let rendered = display(&stream);
        assert!(retained(&stream).truncated);
        assert_eq!(rendered.matches("[output truncated]").count(), 1);
        assert_eq!(
            rendered.split_inclusive('\n').count(),
            RETAINED_TOOL_OUTPUT_LINES
        );

        let exact_with_tail = (0..RETAINED_TOOL_OUTPUT_LINES)
            .map(|index| format!("line-{index}\n"))
            .collect::<String>()
            + "unterminated";
        let stream = CommandOutputStream::from_complete(&exact_with_tail).unwrap();
        let output = retained(&stream);
        assert!(output.truncated);
        assert_eq!(
            output.text.split_inclusive('\n').count(),
            RETAINED_TOOL_OUTPUT_LINES
        );
        assert_eq!(
            display(&stream).split_inclusive('\n').count(),
            RETAINED_TOOL_OUTPUT_LINES
        );
    }

    #[test]
    fn retained_bytes_charges_consumed_prefixes_still_held_by_chunks() {
        let maximum = LIMITS.wire.retained_tool_output_bytes as usize;
        let stream = CommandOutputStream::from_complete(&"x ".repeat(maximum / 2 + 1)).unwrap();
        assert_eq!(stream.safe_bytes, maximum);
        assert!(stream.retained_bytes() > stream.safe_bytes);
        assert!(stream.allocated_bytes() >= stream.retained_bytes());
    }

    #[test]
    fn clear_and_replace_reset_stream_state() {
        let mut stream = CommandOutputStream::new();
        stream.append("Bearer secret ").unwrap();
        assert!(display(&stream).contains("[redacted]"));
        stream.clear();
        assert_eq!(stream.retained_bytes(), 0);
        assert_eq!(display(&stream), "");
        stream.append("safe ").unwrap();
        assert_eq!(display(&stream), "safe ");

        stream.append("Bearer ").unwrap();
        stream.replace_complete("replacement ").unwrap();
        assert_eq!(display(&stream), "replacement ");
        stream.append("next ").unwrap();
        assert_eq!(display(&stream), "replacement next ");
    }
}
