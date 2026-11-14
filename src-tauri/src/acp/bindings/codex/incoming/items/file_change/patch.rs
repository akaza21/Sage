//! Decode only the text actually present in a single file's unified patch.
//! A contiguous hunk is a replacement excerpt, like ACP replacement text.
//! Disjoint hunks retain their positions for exact context reconstruction.

use crate::acp_protocol::file_diff::DIFF_SIDE_BYTES_MAX;

pub(super) struct Patch {
    pub old_text: String,
    pub new_text: String,
    pub additions: u64,
    pub deletions: u64,
    hunks: Vec<PatchHunk>,
}

struct PatchHunk {
    old_start: usize,
    new_start: usize,
    old_bytes: std::ops::Range<usize>,
    new_bytes: std::ops::Range<usize>,
}

mod reconstruct;

pub(super) enum PatchError {
    Invalid,
    Oversized,
}

use PatchError::{Invalid, Oversized};

pub(super) fn raw_content(content: &str, created: bool) -> Result<Patch, PatchError> {
    if content.len() > DIFF_SIDE_BYTES_MAX {
        return Err(Oversized);
    }
    let lines = content.lines().count() as u64;
    Ok(Patch {
        old_text: if created {
            String::new()
        } else {
            content.to_owned()
        },
        new_text: if created {
            content.to_owned()
        } else {
            String::new()
        },
        additions: if created { lines } else { 0 },
        deletions: if created { 0 } else { lines },
        hunks: Vec::new(),
    })
}

#[derive(Default)]
struct Hunk {
    old_remaining: usize,
    new_remaining: usize,
    old_end: usize,
    new_end: usize,
    old_start: usize,
    new_start: usize,
    old_offset: usize,
    new_offset: usize,
}

pub(super) fn decode(diff: &str) -> Result<Patch, PatchError> {
    // Bound header overhead as well as the retained sides, before allocation.
    if diff.len() > DIFF_SIDE_BYTES_MAX * 2 {
        return Err(Oversized);
    }
    let mut patch = Patch {
        old_text: String::new(),
        new_text: String::new(),
        additions: 0,
        deletions: 0,
        hunks: Vec::new(),
    };
    let mut hunk = Hunk::default();
    let mut seen_hunk = false;
    let mut previous = None;
    for line in diff.split_inclusive('\n') {
        let record = line.trim_end_matches(['\r', '\n']);
        if record.starts_with("@@ ") {
            if hunk.old_remaining != 0 || hunk.new_remaining != 0 {
                return Err(Invalid);
            }
            if seen_hunk {
                finish_hunk(&mut patch, &hunk)?;
            }
            let next = header(record, seen_hunk.then_some(&hunk))?;
            hunk = Hunk {
                old_offset: patch.old_text.len(),
                new_offset: patch.new_text.len(),
                ..next
            };
            seen_hunk = true;
            previous = None;
            continue;
        }
        if !seen_hunk {
            if record.starts_with("diff --git ")
                || record.starts_with("index ")
                || record.starts_with("--- ")
                || record.starts_with("+++ ")
                || record.starts_with("new file mode ")
                || record.starts_with("deleted file mode ")
            {
                continue;
            }
            return Err(Invalid);
        }
        if record == "\\ No newline at end of file" {
            match previous.take() {
                Some(b'-') => strip_newline(&mut patch.old_text)?,
                Some(b'+') => strip_newline(&mut patch.new_text)?,
                Some(b' ') => {
                    strip_newline(&mut patch.old_text)?;
                    strip_newline(&mut patch.new_text)?;
                }
                _ => return Err(Invalid),
            }
            continue;
        }
        let kind = *line.as_bytes().first().ok_or(Invalid)?;
        let text = line.get(1..).ok_or(Invalid)?;
        match kind {
            b'-' => {
                hunk.old_remaining = hunk.old_remaining.checked_sub(1).ok_or(Invalid)?;
                append(&mut patch.old_text, text)?;
                patch.deletions += 1;
            }
            b'+' => {
                hunk.new_remaining = hunk.new_remaining.checked_sub(1).ok_or(Invalid)?;
                append(&mut patch.new_text, text)?;
                patch.additions += 1;
            }
            b' ' => {
                hunk.old_remaining = hunk.old_remaining.checked_sub(1).ok_or(Invalid)?;
                hunk.new_remaining = hunk.new_remaining.checked_sub(1).ok_or(Invalid)?;
                append(&mut patch.old_text, text)?;
                append(&mut patch.new_text, text)?;
            }
            _ => return Err(Invalid),
        }
        previous = Some(kind);
    }
    if !seen_hunk || hunk.old_remaining != 0 || hunk.new_remaining != 0 {
        return Err(Invalid);
    }
    finish_hunk(&mut patch, &hunk)?;
    Ok(patch)
}

fn finish_hunk(patch: &mut Patch, hunk: &Hunk) -> Result<(), PatchError> {
    // EOF markers must not collapse two reported lines into one saved line.
    if patch.old_text[hunk.old_offset..]
        .split_inclusive('\n')
        .count()
        != hunk.old_end - hunk.old_start
        || patch.new_text[hunk.new_offset..]
            .split_inclusive('\n')
            .count()
            != hunk.new_end - hunk.new_start
    {
        return Err(Invalid);
    }
    // Separately bound hunk metadata, including zero-line malicious hunks.
    if patch.hunks.len() == 1024 {
        return Err(Oversized);
    }
    patch.hunks.push(PatchHunk {
        old_start: hunk.old_start,
        new_start: hunk.new_start,
        old_bytes: hunk.old_offset..patch.old_text.len(),
        new_bytes: hunk.new_offset..patch.new_text.len(),
    });
    Ok(())
}

fn append(side: &mut String, text: &str) -> Result<(), PatchError> {
    if text.len() > DIFF_SIDE_BYTES_MAX.saturating_sub(side.len()) {
        return Err(Oversized);
    }
    side.push_str(text);
    Ok(())
}

fn strip_newline(side: &mut String) -> Result<(), PatchError> {
    if !side.ends_with('\n') {
        return Err(Invalid);
    }
    side.pop();
    Ok(())
}

fn header(line: &str, previous: Option<&Hunk>) -> Result<Hunk, PatchError> {
    let mut parts = line.split_whitespace();
    if parts.next() != Some("@@") {
        return Err(Invalid);
    }
    let (old_start, old_remaining) = range(parts.next().ok_or(Invalid)?, '-')?;
    let (new_start, new_remaining) = range(parts.next().ok_or(Invalid)?, '+')?;
    if parts.next() != Some("@@") {
        return Err(Invalid);
    }
    if let Some(previous) = previous {
        let old_gap = old_start.checked_sub(previous.old_end).ok_or(Invalid)?;
        let new_gap = new_start.checked_sub(previous.new_end).ok_or(Invalid)?;
        if old_gap != new_gap {
            return Err(Invalid);
        }
    } else if old_start != new_start {
        return Err(Invalid);
    }
    Ok(Hunk {
        old_remaining,
        new_remaining,
        old_end: old_start.checked_add(old_remaining).ok_or(Invalid)?,
        new_end: new_start.checked_add(new_remaining).ok_or(Invalid)?,
        old_start,
        new_start,
        ..Hunk::default()
    })
}

fn range(text: &str, prefix: char) -> Result<(usize, usize), PatchError> {
    let text = text.strip_prefix(prefix).ok_or(Invalid)?;
    let (start, count) = text.split_once(',').unwrap_or((text, "1"));
    let number = |value: &str| {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(Invalid);
        }
        value.parse::<usize>().map_err(|_| Invalid)
    };
    let count = number(count)?;
    let start = number(start)?;
    let index = if count == 0 {
        start
    } else {
        start.checked_sub(1).ok_or(Invalid)?
    };
    Ok((index, count))
}
