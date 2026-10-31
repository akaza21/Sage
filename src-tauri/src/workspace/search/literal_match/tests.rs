use super::{ASCII_FOLDING_SPECIALS, file_may_match, literal_matches};

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
    assert!(file_may_match(dotted, "istanbul", false));
    assert_eq!(literal_matches(dotted, "i", false, false), vec![(0, 2)]);
}

/// The matcher before the ASCII fast path, kept as the oracle: every
/// insensitive search lowercased a copy of each line.
fn reference_matches(
    line: &str,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
) -> Vec<(usize, usize)> {
    let mut matches = Vec::new();
    let mut offset = 0;
    while offset < line.len() {
        let Some((start, end)) = super::find_literal(&line[offset..], query, case_sensitive) else {
            break;
        };
        let (start, end) = (offset + start, offset + end);
        if !whole_word || super::has_word_boundaries(line, start, end) {
            matches.push((start, end));
        }
        offset = end;
    }
    matches
}

/// A small deterministic generator, so a failure always reproduces.
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
            if !file_may_match(&content, &query, case_sensitive) {
                skipped += 1;
                assert!(
                    !any_line_matches,
                    "content {content:?}, query {query:?}, case {case_sensitive}"
                );
            }
        }
    }
    // The check has to pass files over, or it saves nothing.
    assert!(skipped > 5_000, "only {skipped} files passed over");
}

#[test]
fn only_the_listed_characters_fold_into_ascii() {
    let folding_into_ascii = (0x80..=0x10_FFFF)
        .filter_map(char::from_u32)
        .filter(|character| character.to_lowercase().any(|folded| folded.is_ascii()))
        .map(|character| character.to_string())
        .collect::<Vec<_>>();
    let listed = ASCII_FOLDING_SPECIALS
        .iter()
        .map(|bytes| std::str::from_utf8(bytes).unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(folding_into_ascii, listed);
}
