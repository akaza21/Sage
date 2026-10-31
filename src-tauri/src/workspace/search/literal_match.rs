/// Characters outside ASCII whose lowercase form contains ASCII letters:
/// LATIN CAPITAL LETTER I WITH DOT ABOVE (U+0130) folds to `i` plus a
/// combining dot and KELVIN SIGN (U+212A) to `k`. Where neither appears, an
/// ASCII query matches exactly where ASCII case folding says it does.
const ASCII_FOLDING_SPECIALS: [&[u8]; 2] = ["\u{0130}".as_bytes(), "\u{212A}".as_bytes()];

pub(super) fn literal_matches(
    line: &str,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
) -> Vec<(usize, usize)> {
    // An ASCII line holds no special character, so ASCII folding gives what
    // full lowercasing gives, without a folded copy of the line.
    let ascii_folding = !case_sensitive && query.is_ascii() && line.is_ascii();
    let mut matches = Vec::new();
    let mut offset = 0;
    while offset < line.len() {
        let found = if ascii_folding {
            find_ascii_folded(&line.as_bytes()[offset..], query.as_bytes())
                .map(|start| (start, start + query.len()))
        } else {
            find_literal(&line[offset..], query, case_sensitive)
        };
        let Some((start, end)) = found else {
            break;
        };
        let start = offset + start;
        let end = offset + end;
        if !whole_word || has_word_boundaries(line, start, end) {
            matches.push((start, end));
        }
        offset = end;
    }
    matches
}

/// False only when no line of `content` can hold a match, so a File can be
/// passed over without splitting it into lines. True is only a maybe.
pub(super) fn file_may_match(content: &str, query: &str, case_sensitive: bool) -> bool {
    let bytes = content.as_bytes();
    if case_sensitive {
        return memchr::memmem::find(bytes, query.as_bytes()).is_some();
    }
    if !query.is_ascii()
        || ASCII_FOLDING_SPECIALS
            .iter()
            .any(|special| memchr::memmem::find(bytes, special).is_some())
    {
        return true;
    }
    find_ascii_folded(bytes, query.as_bytes()).is_some()
}

/// The first place `needle` occurs in `haystack`, ignoring ASCII case.
fn find_ascii_folded(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let (&first, rest) = needle.split_first()?;
    let (lower, upper) = (first.to_ascii_lowercase(), first.to_ascii_uppercase());
    let last_start = haystack.len().checked_sub(needle.len())?;
    let mut from = 0;
    while from <= last_start {
        let start = from + memchr::memchr2(lower, upper, &haystack[from..=last_start])?;
        if haystack[start + 1..start + needle.len()].eq_ignore_ascii_case(rest) {
            return Some(start);
        }
        from = start + 1;
    }
    None
}

fn find_literal(line: &str, query: &str, case_sensitive: bool) -> Option<(usize, usize)> {
    if case_sensitive {
        return line.find(query).map(|start| (start, start + query.len()));
    }
    let needle = query.to_lowercase();
    let mut folded = String::with_capacity(line.len());
    let mut origin = Vec::with_capacity(line.len());
    for (start, character) in line.char_indices() {
        let end = start + character.len_utf8();
        for folded_character in character.to_lowercase() {
            let mut encoded = [0; 4];
            let encoded = folded_character.encode_utf8(&mut encoded);
            folded.push_str(encoded);
            origin.extend(std::iter::repeat_n((start, end), encoded.len()));
        }
    }
    let folded_start = folded.find(&needle)?;
    let folded_end = folded_start + needle.len();
    Some((
        origin.get(folded_start)?.0,
        origin.get(folded_end.checked_sub(1)?)?.1,
    ))
}

fn has_word_boundaries(line: &str, start: usize, end: usize) -> bool {
    let before_is_word = line[..start]
        .chars()
        .next_back()
        .is_some_and(is_word_character);
    let after_is_word = line[end..].chars().next().is_some_and(is_word_character);
    !before_is_word && !after_is_word
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

#[cfg(test)]
mod tests;
