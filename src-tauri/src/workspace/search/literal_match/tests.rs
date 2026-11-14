use std::cell::Cell;

use memchr::memmem::Finder;

use super::{
    ASCII_FOLDING_SPECIALS, LiteralQuery, ascii_folded_matches, has_word_boundaries,
    unicode_folded_matches,
};

fn literal_matches(
    line: &str,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
) -> Vec<(usize, usize)> {
    LiteralQuery::new(query, case_sensitive).line_matches(line, whole_word)
}

#[test]
fn whole_word_uses_unicode_word_boundaries() {
    assert_eq!(
        literal_matches("éneedle needle needle_value", "needle", false, true),
        vec![(9, 15)]
    );
}

#[test]
fn insensitive_matching_keeps_original_byte_ranges() {
    assert_eq!(
        literal_matches("Straße STRASSE", "strasse", false, false),
        vec![(8, 15)]
    );
}

#[test]
fn ascii_queries_still_match_characters_that_fold_into_ascii() {
    assert_eq!(
        literal_matches("\u{212A}ey", "key", false, false),
        vec![(0, 5)]
    );
    let dotted = "\u{0130}stanbul";
    assert!(LiteralQuery::new("istanbul", false).file_may_match(dotted));
    assert_eq!(literal_matches(dotted, "i", false, false), vec![(0, 2)]);
}

/// The first match of the matcher before EP-09, kept as the oracle: it
/// lowercased the rest of the line, one character at a time, and searched it.
fn reference_find(line: &str, query: &str, case_sensitive: bool) -> Option<(usize, usize)> {
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

fn reference_matches(
    line: &str,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
) -> Vec<(usize, usize)> {
    let mut matches = Vec::new();
    let mut offset = 0;
    while offset < line.len() {
        let Some((start, end)) = reference_find(&line[offset..], query, case_sensitive) else {
            break;
        };
        let (start, end) = (offset + start, offset + end);
        if !whole_word || has_word_boundaries(line, start, end) {
            matches.push((start, end));
        }
        offset = end;
    }
    matches
}

struct Pieces(u64);

impl Pieces {
    const ALPHABET: [&'static str; 22] = [
        "a", "A", "b", "B", "k", "K", "i", "I", "s", "S", "_", " ", "-", "1", "é", "É", "ß",
        "\u{212A}", "\u{0130}", "ı", "😀", "\t",
    ];

    fn next(&mut self) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 33) as usize
    }

    fn text(&mut self, max: usize, ascii_only: bool) -> String {
        let length = self.next() % max + 1;
        let pool = if ascii_only { 14 } else { Self::ALPHABET.len() };
        (0..length)
            .map(|_| Self::ALPHABET[self.next() % pool])
            .collect()
    }
}

#[test]
fn matches_the_reference_matcher_on_mixed_text() {
    let mut pieces = Pieces(0x9E37_79B9_7F4A_7C15);
    for round in 0..40_000 {
        let line = pieces.text(24, round % 3 == 0);
        let query = pieces.text(4, round % 2 == 0);
        for case_sensitive in [false, true] {
            for whole_word in [false, true] {
                assert_eq!(
                    literal_matches(&line, &query, case_sensitive, whole_word),
                    reference_matches(&line, &query, case_sensitive, whole_word),
                    "line {line:?}, query {query:?}, case {case_sensitive}, word {whole_word}"
                );
            }
        }
    }
}

#[test]
fn folding_window_by_window_matches_folding_whole_lines() {
    let mut pieces = Pieces(0x2545_F491_4F6C_DD1D);
    for round in 0..20_000 {
        let ascii = round % 2 == 0;
        let line = pieces.text(40, ascii);
        let query = pieces.text(4, ascii);
        let folded = query.to_lowercase();
        let finder = Finder::new(&folded);
        let expected = reference_matches(&line, &query, false, false);
        for window in 1..=9 {
            let mut found = Vec::new();
            unicode_folded_matches(&line, &finder, window, |range| {
                found.push(range);
                true
            });
            assert_eq!(
                found, expected,
                "line {line:?}, query {query:?}, window {window}"
            );
            if ascii {
                let mut found = Vec::new();
                ascii_folded_matches(line.as_bytes(), &finder, window, |range| {
                    found.push(range);
                    true
                });
                assert_eq!(
                    found, expected,
                    "line {line:?}, query {query:?}, window {window}"
                );
            }
        }
    }
}

#[test]
fn the_file_check_never_passes_over_a_file_with_a_match() {
    let mut pieces = Pieces(0xD1B5_4A32_D192_ED03);
    let mut skipped = 0;
    for round in 0..20_000 {
        let content = (0..3)
            .map(|_| pieces.text(16, round % 3 == 0))
            .collect::<Vec<_>>()
            .join("\n");
        let query = pieces.text(3, round % 2 == 0);
        for case_sensitive in [false, true] {
            let any_line_matches = content
                .split('\n')
                .any(|line| !reference_matches(line, &query, case_sensitive, false).is_empty());
            if !LiteralQuery::new(&query, case_sensitive).file_may_match(&content) {
                skipped += 1;
                assert!(
                    !any_line_matches,
                    "content {content:?}, query {query:?}, case {case_sensitive}"
                );
            }
        }
    }
    assert!(skipped > 5_000, "only {skipped} files passed over");
}

#[test]
fn only_the_listed_characters_fold_into_ascii() {
    let folding_into_ascii = (0x80..=0x10_FFFF)
        .filter_map(char::from_u32)
        .filter(|character| character.to_lowercase().any(|folded| folded.is_ascii()))
        .map(|character| character.to_string())
        .collect::<Vec<_>>();
    assert_eq!(folding_into_ascii, ASCII_FOLDING_SPECIALS);
}

thread_local! {
    // Matching is synchronous; each test thread measures only its own work.
    static FOLDED_BYTES: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn record_folded_bytes(bytes: usize) {
    FOLDED_BYTES.with(|work| work.set(work.get() + bytes));
}

/// Counts bytes folded, including overlap, rather than shared-runner time:
/// memmem is linear, and this guards the folding around it against
/// repeatedly processing a candidate or the remaining suffix.
#[test]
fn near_misses_cost_linear_time() {
    for (length, prefix) in [(1024, 255), (2048, 511), (2 * 1024 * 1024, 511)] {
        let content = "a".repeat(length);
        let query = format!("{}b", "a".repeat(prefix));
        let exact = LiteralQuery::new(&query, true);
        assert!(!exact.file_may_match(&content));
        assert!(exact.line_matches(&content, false).is_empty());

        let ascii = LiteralQuery::new(&query, false);
        let unicode = LiteralQuery::new(&format!("{}é", "a".repeat(prefix)), false);
        for (path, literal, file_check) in [
            ("ASCII file", &ascii, true),
            ("ASCII line", &ascii, false),
            ("Unicode line", &unicode, false),
        ] {
            FOLDED_BYTES.with(|work| work.set(0));
            if file_check {
                assert!(!literal.file_may_match(&content));
            } else {
                assert!(literal.line_matches(&content, false).is_empty());
            }
            let work = FOLDED_BYTES.with(Cell::get);
            // Each byte is folded once, plus bounded window overlap. A naive
            // matcher folds roughly (length - prefix) * prefix bytes instead.
            assert!(
                (length..=2 * length).contains(&work),
                "{path}: folded {work} bytes for {length} input bytes, prefix {prefix}"
            );
        }
    }
}
