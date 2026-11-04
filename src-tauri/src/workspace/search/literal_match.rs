use memchr::memmem::Finder;

/// Characters outside ASCII whose lowercase form contains ASCII letters:
/// LATIN CAPITAL LETTER I WITH DOT ABOVE (U+0130) folds to `i` plus a
/// combining dot and KELVIN SIGN (U+212A) to `k`. Where neither appears, an
/// ASCII query matches exactly where ASCII case folding says it does.
const ASCII_FOLDING_SPECIALS: [&str; 2] = ["\u{0130}", "\u{212A}"];

/// Case-insensitive matching folds text one window of about this many bytes
/// at a time, so its memory stays small however long a line is, and each
/// byte is folded once however many matches it holds.
const FOLD_WINDOW_BYTES: usize = 16 * 1024;

/// A literal query, set up once per search and shared by every thread that
/// matches with it. All matching is linear in the text searched.
pub(super) struct LiteralQuery {
    case_sensitive: bool,
    ascii: bool,
    /// The query as typed.
    exact: Finder<'static>,
    /// The query lowercased as a whole, which for ASCII is ASCII folding.
    folded: Finder<'static>,
    specials: [Finder<'static>; 2],
}

impl LiteralQuery {
    pub(super) fn new(text: &str, case_sensitive: bool) -> Self {
        Self {
            case_sensitive,
            ascii: text.is_ascii(),
            exact: Finder::new(text).into_owned(),
            folded: Finder::new(&text.to_lowercase()).into_owned(),
            specials: ASCII_FOLDING_SPECIALS.map(|special| Finder::new(special).into_owned()),
        }
    }

    /// Hands `visit` the byte range of each of the line's matches until it
    /// returns false: leftmost first, each search resuming where the last
    /// match ended. A line can hold millions, so none are collected here.
    pub(super) fn for_each_match(
        &self,
        line: &str,
        whole_word: bool,
        mut visit: impl FnMut((usize, usize)) -> bool,
    ) {
        // A match that is not a whole word is passed over, but the next
        // search still resumes after it.
        let mut report = |(start, end)| {
            if whole_word && !has_word_boundaries(line, start, end) {
                return true;
            }
            visit((start, end))
        };
        if self.case_sensitive {
            let mut offset = 0;
            while let Some(start) = self.exact.find(&line.as_bytes()[offset..]) {
                let start = offset + start;
                offset = start + self.exact.needle().len();
                if !report((start, offset)) {
                    return;
                }
            }
        } else if self.ascii && line.is_ascii() {
            // An ASCII line holds no special character, so ASCII folding
            // gives what full lowercasing gives.
            ascii_folded_matches(line.as_bytes(), &self.folded, FOLD_WINDOW_BYTES, report);
        } else {
            unicode_folded_matches(line, &self.folded, FOLD_WINDOW_BYTES, report);
        }
    }

    #[cfg(test)]
    pub(super) fn line_matches(&self, line: &str, whole_word: bool) -> Vec<(usize, usize)> {
        let mut matches = Vec::new();
        self.for_each_match(line, whole_word, |range| {
            matches.push(range);
            true
        });
        matches
    }

    /// False only when no line of `content` can hold a match, so a File can
    /// be passed over without splitting it into lines. True is only a maybe.
    pub(super) fn file_may_match(&self, content: &str) -> bool {
        let bytes = content.as_bytes();
        if self.case_sensitive {
            return self.exact.find(bytes).is_some();
        }
        if !self.ascii
            || self
                .specials
                .iter()
                .any(|special| special.find(bytes).is_some())
        {
            return true;
        }
        let mut found = false;
        ascii_folded_matches(bytes, &self.folded, FOLD_WINDOW_BYTES, |_| {
            found = true;
            false
        });
        found
    }
}

/// Reports each match of the ASCII-lowercase `needle` in `haystack`,
/// ignoring ASCII case, until `report` returns false. Each window of
/// `window` bytes, plus enough to finish a match that starts in it, is
/// lowercased once into a fixed buffer.
fn ascii_folded_matches(
    haystack: &[u8],
    needle: &Finder<'_>,
    window: usize,
    mut report: impl FnMut((usize, usize)) -> bool,
) {
    let needle_len = needle.needle().len();
    let mut buffer = Vec::new();
    let mut offset = 0;
    while offset < haystack.len() {
        let core_end = (offset + window).min(haystack.len());
        let window_end = (core_end + needle_len - 1).min(haystack.len());
        buffer.clear();
        #[cfg(test)]
        tests::record_folded_bytes(window_end - offset);
        buffer.extend(
            haystack[offset..window_end]
                .iter()
                .map(u8::to_ascii_lowercase),
        );
        let mut next = core_end;
        let mut from = 0;
        while let Some(found) = needle.find(&buffer[from..]) {
            let start = offset + from + found;
            // A match starting past the core is the next window's.
            if start >= core_end {
                break;
            }
            let end = start + needle_len;
            if !report((start, end)) {
                return;
            }
            from = end - offset;
            next = next.max(end);
        }
        offset = next;
    }
}

/// Reports each match of the lowercased `needle` in `line` lowercased one
/// character at a time, as ranges of whole characters of `line`, until
/// `report` returns false. This is what lowercasing the rest of the line
/// after each match and searching it gives, one window at a time.
fn unicode_folded_matches(
    line: &str,
    needle: &Finder<'_>,
    window: usize,
    mut report: impl FnMut((usize, usize)) -> bool,
) {
    let needle_len = needle.needle().len();
    // A match folds from at most one character per byte of the needle, plus
    // the one it may start inside, each at most four bytes long.
    let reach = 4 * (needle_len + 1);
    let mut folded = Vec::new();
    let mut origin = Vec::new();
    let mut offset = 0;
    while offset < line.len() {
        let core_end = ceil_char_boundary(line, offset + window);
        let window_end = ceil_char_boundary(line, core_end + reach);
        folded.clear();
        origin.clear();
        #[cfg(test)]
        tests::record_folded_bytes(window_end - offset);
        for (start, character) in line[offset..window_end].char_indices() {
            let start = offset + start;
            let end = start + character.len_utf8();
            for folded_character in character.to_lowercase() {
                let mut encoded = [0; 4];
                let encoded = folded_character.encode_utf8(&mut encoded);
                folded.extend_from_slice(encoded.as_bytes());
                origin.extend(std::iter::repeat_n((start, end), encoded.len()));
            }
        }
        let mut next = core_end;
        let mut from = 0;
        while let Some(found) = needle.find(&folded[from..]) {
            let folded_start = from + found;
            let start = origin[folded_start].0;
            if start >= core_end {
                break;
            }
            let end = origin[folded_start + needle_len - 1].1;
            if !report((start, end)) {
                return;
            }
            next = next.max(end);
            // Resume after the whole last character, as the search of the
            // rest of the line would.
            from = folded_start + needle_len;
            while origin.get(from).is_some_and(|&(start, _)| start < end) {
                from += 1;
            }
        }
        offset = next;
    }
}

/// The first character boundary of `text` at or after `index`, capped at its
/// length.
fn ceil_char_boundary(text: &str, index: usize) -> usize {
    (index.min(text.len())..=text.len())
        .find(|&candidate| text.is_char_boundary(candidate))
        .unwrap_or(text.len())
}

pub(super) fn has_word_boundaries(line: &str, start: usize, end: usize) -> bool {
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
