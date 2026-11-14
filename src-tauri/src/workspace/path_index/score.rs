const MATCH: i32 = 16;
const SEGMENT_START: i32 = 30;
const WORD_START: i32 = 20;
const CAMEL: i32 = 18;
const DIGIT_START: i32 = 8;
const RUN: i32 = 18;
const EXACT_CASE: i32 = 1;
const GAP_OPEN: i32 = 3;
const GAP_EXTEND: i32 = 1;
const BASENAME: i32 = 50;
const BASENAME_START: i32 = 20;

/// Longer input is cut here.
pub(super) const MAX_QUERY_CHARS: usize = 256;

pub(super) struct Query {
    pieces: Vec<Piece>,
}

struct Piece {
    lower: Vec<char>,
    raw: Vec<char>,
    /// The lowercase piece as bytes, when it is all ASCII.
    ascii: Option<Vec<u8>>,
}

impl Query {
    /// `\` is read as `/`. `None` when the query is empty.
    pub(super) fn new(text: &str) -> Option<Self> {
        let mut budget = MAX_QUERY_CHARS;
        let mut pieces = Vec::new();
        for word in text.split_whitespace() {
            let raw = word
                .chars()
                .map(|character| if character == '\\' { '/' } else { character })
                .take(budget)
                .collect::<Vec<_>>();
            if raw.is_empty() {
                break;
            }
            budget -= raw.len();
            let lower = raw
                .iter()
                .map(|character| lower(*character))
                .collect::<Vec<_>>();
            pieces.push(Piece {
                ascii: lower
                    .iter()
                    .all(char::is_ascii)
                    .then(|| lower.iter().map(|character| *character as u8).collect()),
                lower,
                raw,
            });
        }
        (!pieces.is_empty()).then_some(Self { pieces })
    }
}

trait Unit: Copy {
    fn character(self) -> char;
}

impl Unit for u8 {
    fn character(self) -> char {
        self as char
    }
}

impl Unit for char {
    fn character(self) -> char {
        self
    }
}

fn lower(character: char) -> char {
    if character.is_ascii() {
        character.to_ascii_lowercase()
    } else {
        character.to_lowercase().next().unwrap_or(character)
    }
}

/// Higher is better; `None` when the query doesn't match. `base` is the byte
/// offset where the basename starts.
pub(super) fn score(query: &Query, path: &str, base: usize, ascii: bool) -> Option<i32> {
    let mut total = 0;
    for piece in &query.pieces {
        total += best(piece, path, base, ascii, None)?;
    }
    Some(total)
}

/// The matched characters of `path`, as UTF-16 offsets (what JavaScript
/// strings index by), for highlighting.
pub(super) fn positions(query: &Query, path: &str, base: usize) -> Vec<u32> {
    let ascii = path.is_ascii();
    let mut out = Vec::new();
    for piece in &query.pieces {
        let mut matched = Vec::with_capacity(piece.lower.len());
        best(piece, path, base, ascii, Some(&mut matched));
        out.extend(matched);
    }
    out.sort_unstable();
    out.dedup();
    if ascii {
        return out.into_iter().map(|index| index as u32).collect();
    }
    let mut utf16 = Vec::with_capacity(path.len());
    let mut offset = 0;
    for character in path.chars() {
        utf16.push(offset);
        offset += character.len_utf16() as u32;
    }
    out.into_iter().map(|index| utf16[index]).collect()
}

fn best(
    piece: &Piece,
    path: &str,
    base: usize,
    ascii: bool,
    out: Option<&mut Vec<usize>>,
) -> Option<i32> {
    if ascii {
        let wanted = piece.ascii.as_deref()?;
        if !in_order(wanted, path.as_bytes()) {
            return None;
        }
        return best_in(piece, path.as_bytes(), base, out);
    }
    let characters = path.chars().collect::<Vec<_>>();
    let base = path[..base].chars().count();
    best_in(piece, &characters, base, out)
}

fn in_order(wanted: &[u8], hay: &[u8]) -> bool {
    let mut from = 0;
    for &want in wanted {
        let rest = &hay[from..];
        let found = if want.is_ascii_lowercase() {
            memchr::memchr2(want, want.to_ascii_uppercase(), rest)
        } else {
            memchr::memchr(want, rest)
        };
        match found {
            Some(offset) => from += offset + 1,
            None => return false,
        }
    }
    true
}

fn best_in<U: Unit>(
    query: &Piece,
    hay: &[U],
    base: usize,
    mut out: Option<&mut Vec<usize>>,
) -> Option<i32> {
    // Shorter paths win ties, as in VS Code.
    let length_penalty = (hay.len() / 10) as i32;
    if let Some(score) = align(query, hay, base, out.as_deref_mut()) {
        return Some(score + BASENAME - length_penalty);
    }
    if let Some(positions) = out.as_deref_mut() {
        positions.clear();
    }
    align(query, hay, 0, out).map(|score| score - length_penalty)
}

fn align<U: Unit>(
    query: &Piece,
    hay: &[U],
    from: usize,
    out: Option<&mut Vec<usize>>,
) -> Option<i32> {
    let wanted = &query.lower;
    let mut next = 0;
    let mut end = None;
    for (index, unit) in hay.iter().enumerate().skip(from) {
        if lower(unit.character()) == wanted[next] {
            next += 1;
            if next == wanted.len() {
                end = Some(index);
                break;
            }
        }
    }
    let end = end?;
    let mut start = end;
    let mut remaining = wanted.len();
    for index in (from..=end).rev() {
        if lower(hay[index].character()) == wanted[remaining - 1] {
            remaining -= 1;
            if remaining == 0 {
                start = index;
                break;
            }
        }
    }
    Some(score_window(query, hay, from, start, end, out))
}

fn score_window<U: Unit>(
    query: &Piece,
    hay: &[U],
    base: usize,
    start: usize,
    end: usize,
    mut out: Option<&mut Vec<usize>>,
) -> i32 {
    let mut score = 0;
    let mut next = 0;
    let mut previous_match: Option<usize> = None;
    let mut in_gap = false;
    for index in start..=end {
        let character = hay[index].character();
        if next < query.lower.len() && lower(character) == query.lower[next] {
            score += MATCH + boundary(hay, index);
            if character == query.raw[next] {
                score += EXACT_CASE;
            }
            if previous_match == Some(index.wrapping_sub(1)) {
                score += RUN;
            }
            if index == base && base > 0 {
                score += BASENAME_START;
            }
            if let Some(positions) = out.as_deref_mut() {
                positions.push(index);
            }
            previous_match = Some(index);
            next += 1;
            in_gap = false;
        } else {
            score -= if in_gap { GAP_EXTEND } else { GAP_OPEN };
            in_gap = true;
        }
    }
    score
}

fn boundary<U: Unit>(hay: &[U], index: usize) -> i32 {
    let Some(previous) = index.checked_sub(1).map(|before| hay[before].character()) else {
        return SEGMENT_START;
    };
    let current = hay[index].character();
    match previous {
        '/' => SEGMENT_START,
        '_' | '-' | '.' | ' ' => WORD_START,
        _ if previous.is_lowercase() && current.is_uppercase() => CAMEL,
        _ if !previous.is_ascii_digit() && current.is_ascii_digit() => DIGIT_START,
        _ => 0,
    }
}
