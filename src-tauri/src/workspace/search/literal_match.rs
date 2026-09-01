pub(super) fn literal_matches(
    line: &str,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
) -> Vec<(usize, usize)> {
    let mut matches = Vec::new();
    let mut offset = 0;
    while offset < line.len() {
        let Some((start, end)) = find_literal(&line[offset..], query, case_sensitive) else {
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
mod tests {
    use super::literal_matches;

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
}
