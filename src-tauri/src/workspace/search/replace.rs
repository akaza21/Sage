//! Workspace Replace: what a Replace makes of a File's text. The Search
//! query's own matcher finds the matches, so Replace changes exactly what
//! Search showed.

use serde::{Deserialize, Serialize};

use super::{
    file_scan::{LineMatches, one_based},
    literal_match::{LiteralQuery, search_query},
    types::{MAX_QUERY_BYTES, MAX_SEARCH_PREVIEW_CHARS},
};
use crate::error::{AppError, AppResult};

pub const MAX_REPLACEMENT_BYTES: usize = 2 * 1024;
/// Changed lines shown for one File, and for one whole Replace.
pub const MAX_PREVIEW_LINES_PER_FILE: usize = 50;
pub const MAX_PREVIEW_LINES: usize = 2_000;

/// What to replace with what, with the Search options it was found with.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplaceSpec {
    pub query: String,
    pub replacement: String,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub whole_word: bool,
    /// `$1`, `${name}` and `$$` in the replacement refer to the match.
    #[serde(default)]
    pub use_regex: bool,
    /// The replacement takes the matched text's case: all upper, all lower
    /// or capitalized.
    #[serde(default)]
    pub preserve_case: bool,
}

/// One match, as Search reports it: 1-based line and character column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MatchPosition {
    pub line_number: u32,
    pub column_number: u32,
}

/// A changed line, before and after, trimmed and bounded as a Search preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceLinePreview {
    pub line_number: u32,
    /// The first replaced match on the line.
    pub column_number: u32,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacedText {
    pub content: String,
    pub count: u64,
    pub lines: Vec<ReplaceLinePreview>,
    pub lines_truncated: bool,
}

/// A Replace, set up once and run on any number of Files.
pub struct Replacer {
    query: LiteralQuery,
    whole_word: bool,
    replacement: String,
    use_regex: bool,
    preserve_case: bool,
}

impl Replacer {
    pub fn new(spec: &ReplaceSpec) -> AppResult<Self> {
        let query = spec.query.trim();
        if query.is_empty() {
            return Err(AppError::InvalidRequest(
                "workspace replace query must not be empty".into(),
            ));
        }
        if query.len() > MAX_QUERY_BYTES {
            return Err(AppError::ResourceLimit(format!(
                "workspace replace query exceeds the {MAX_QUERY_BYTES}-byte limit"
            )));
        }
        if spec.replacement.len() > MAX_REPLACEMENT_BYTES || spec.replacement.contains('\0') {
            return Err(AppError::ResourceLimit(format!(
                "the replacement exceeds the {MAX_REPLACEMENT_BYTES}-byte limit or holds a NUL"
            )));
        }
        // A carriage return would leave a stray line end in a File whose
        // line endings Sage keeps as they are.
        if spec.replacement.contains(['\r', '\n']) {
            return Err(AppError::InvalidRequest(
                "the replacement can't hold a line break".into(),
            ));
        }
        Ok(Self {
            query: search_query(query, spec.case_sensitive, spec.use_regex)?,
            whole_word: spec.whole_word,
            replacement: if spec.use_regex {
                braced_group_numbers(&spec.replacement)
            } else {
                spec.replacement.clone()
            },
            use_regex: spec.use_regex,
            preserve_case: spec.preserve_case,
        })
    }

    /// Replaces the matches in `content`, whose lines end in `\n`: every one,
    /// or with `only` the one Search reported there. Previews are kept for
    /// at most `preview_lines` changed lines. The replaced text may hold at
    /// most `max_bytes`: past that it stops, before building any more, with
    /// a resource limit error.
    pub fn replace(
        &self,
        content: &str,
        only: Option<MatchPosition>,
        preview_lines: usize,
        max_bytes: usize,
    ) -> AppResult<ReplacedText> {
        let mut output = String::with_capacity(content.len().min(max_bytes));
        let mut count = 0_u64;
        let mut lines = Vec::new();
        let mut lines_truncated = false;
        for (index, raw_line) in content.split('\n').enumerate() {
            if index > 0 {
                output.push('\n');
            }
            let line_number = one_based(index);
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            let ending = &raw_line[line.len()..];
            if only.is_some_and(|position| position.line_number != line_number) {
                output.push_str(raw_line);
                over_budget(&output, max_bytes)?;
                continue;
            }
            let budget = max_bytes.saturating_sub(output.len());
            let (replaced, first_column, found) = self.replace_line(line, only, budget)?;
            if found == 0 {
                output.push_str(raw_line);
                over_budget(&output, max_bytes)?;
                continue;
            }
            count += found;
            if lines.len() < preview_lines {
                lines.push(ReplaceLinePreview {
                    line_number,
                    column_number: first_column,
                    before: bounded(line),
                    after: bounded(&replaced),
                });
            } else {
                lines_truncated = true;
            }
            output.push_str(&replaced);
            output.push_str(ending);
            over_budget(&output, max_bytes)?;
        }
        Ok(ReplacedText {
            content: output,
            count,
            lines,
            lines_truncated,
        })
    }

    /// The line with its matches replaced, the column of the first and how
    /// many there were; a resource limit error once it outgrows `budget`.
    fn replace_line(
        &self,
        line: &str,
        only: Option<MatchPosition>,
        budget: usize,
    ) -> AppResult<(String, u32, u64)> {
        let mut ranges = Vec::new();
        let mut columns = LineMatches::new(line, 1);
        self.query
            .for_each_match(line, self.whole_word, |(start, end)| {
                let column = columns.column(start);
                match only {
                    Some(position) if position.column_number != column => {
                        position.column_number > column
                    }
                    Some(_) => {
                        ranges.push((start, end, column));
                        false
                    }
                    None => {
                        ranges.push((start, end, column));
                        true
                    }
                }
            });
        let Some(&(_, _, first_column)) = ranges.first() else {
            return Ok((line.into(), 0, 0));
        };
        let mut replaced = String::with_capacity(line.len().min(budget));
        let mut copied = 0;
        for &(start, end, _) in &ranges {
            replaced.push_str(&line[copied..start]);
            let text =
                self.replacement_for(line, start, end, budget - replaced.len().min(budget))?;
            replaced.push_str(&text);
            over_budget(&replaced, budget)?;
            copied = end;
        }
        replaced.push_str(&line[copied..]);
        over_budget(&replaced, budget)?;
        Ok((replaced, first_column, ranges.len() as u64))
    }

    fn replacement_for(
        &self,
        line: &str,
        start: usize,
        end: usize,
        budget: usize,
    ) -> AppResult<String> {
        let matched = &line[start..end];
        let mut text = String::new();
        match self.query.pattern() {
            Some(pattern) if self.use_regex => {
                // The match found at `start` is the leftmost one from there.
                match pattern.captures_at(line, start) {
                    Some(captures) if captures.get(0).map(|m| m.start()) == Some(start) => {
                        // No group is longer than the line, so this bounds
                        // the expansion before it is built.
                        let references = self.replacement.matches('$').count();
                        let most = self
                            .replacement
                            .len()
                            .saturating_add(references.saturating_mul(line.len()));
                        if most > budget {
                            return Err(too_large());
                        }
                        captures.expand(&self.replacement, &mut text);
                    }
                    _ => text.push_str(&self.replacement),
                }
            }
            _ => text.push_str(&self.replacement),
        }
        Ok(if self.preserve_case {
            preserve_case(matched, &text)
        } else {
            text
        })
    }
}

fn too_large() -> AppError {
    AppError::ResourceLimit("the replaced text would exceed the editor's size limit".into())
}

fn over_budget(text: &str, budget: usize) -> AppResult<()> {
    if text.len() > budget {
        return Err(too_large());
    }
    Ok(())
}

/// `$1a` reads as the group named `1a`, which never exists; the field's
/// hint promises `$1` is the first group, so a number after `$` ends where
/// its digits do: `$1a` becomes `${1}a`. `$$`, `${…}` and `$name` stay.
fn braced_group_numbers(replacement: &str) -> String {
    let mut output = String::with_capacity(replacement.len() + 8);
    let mut rest = replacement;
    while let Some(dollar) = rest.find('$') {
        output.push_str(&rest[..dollar]);
        let after = &rest[dollar + 1..];
        if let Some(stripped) = after.strip_prefix('$') {
            output.push_str("$$");
            rest = stripped;
            continue;
        }
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            output.push('$');
            rest = after;
            continue;
        }
        output.push_str("${");
        output.push_str(&after[..digits]);
        output.push('}');
        rest = &after[digits..];
    }
    output.push_str(rest);
    output
}

/// `replacement` in the case of `matched`: all upper, all lower or
/// capitalized; any other mix leaves it as typed.
pub fn preserve_case(matched: &str, replacement: &str) -> String {
    let letters = || matched.chars().filter(|c| c.is_alphabetic());
    if letters().next().is_none() {
        return replacement.into();
    }
    if letters().all(char::is_uppercase) && letters().nth(1).is_some() {
        return replacement.to_uppercase();
    }
    if letters().all(char::is_lowercase) {
        return replacement.to_lowercase();
    }
    let mut rest = letters();
    if rest.next().is_some_and(char::is_uppercase) && rest.all(char::is_lowercase) {
        let mut characters = replacement.chars();
        return match characters.next() {
            Some(first) => first.to_uppercase().chain(characters).collect(),
            None => String::new(),
        };
    }
    replacement.into()
}

fn bounded(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.chars().count() <= MAX_SEARCH_PREVIEW_CHARS {
        return trimmed.into();
    }
    let mut preview = trimmed
        .chars()
        .take(MAX_SEARCH_PREVIEW_CHARS - 1)
        .collect::<String>();
    preview.push('…');
    preview
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(query: &str, replacement: &str) -> ReplaceSpec {
        ReplaceSpec {
            query: query.into(),
            replacement: replacement.into(),
            case_sensitive: false,
            whole_word: false,
            use_regex: false,
            preserve_case: false,
        }
    }

    fn replace(spec: &ReplaceSpec, content: &str) -> ReplacedText {
        Replacer::new(spec)
            .unwrap()
            .replace(content, None, MAX_PREVIEW_LINES_PER_FILE, usize::MAX)
            .unwrap()
    }

    #[test]
    fn replaces_every_literal_match_and_previews_each_line() {
        let done = replace(&spec("old", "new"), "old and OLD\nnone\r\nold\n");
        assert_eq!(done.content, "new and new\nnone\r\nnew\n");
        assert_eq!(done.count, 3);
        assert_eq!(
            done.lines,
            [
                ReplaceLinePreview {
                    line_number: 1,
                    column_number: 1,
                    before: "old and OLD".into(),
                    after: "new and new".into(),
                },
                ReplaceLinePreview {
                    line_number: 3,
                    column_number: 1,
                    before: "old".into(),
                    after: "new".into(),
                },
            ]
        );
    }

    #[test]
    fn regex_groups_fill_the_replacement() {
        let done = replace(
            &ReplaceSpec {
                use_regex: true,
                case_sensitive: true,
                ..spec(r"(\w+)\.unwrap\(\)", "$1.expect(\"${1} is set\")")
            },
            "let a = config.unwrap();\nlet b = Value.unwrap();\n",
        );
        assert_eq!(
            done.content,
            "let a = config.expect(\"config is set\");\nlet b = Value.expect(\"Value is set\");\n"
        );
        assert_eq!(done.count, 2);
        let named = replace(
            &ReplaceSpec {
                use_regex: true,
                ..spec(r"(?P<key>\w+)=(?P<value>\w+)", "$value=$key $$")
            },
            "a=b",
        );
        assert_eq!(named.content, "b=a $");
    }

    #[test]
    fn an_invalid_regex_is_refused_by_name() {
        let Err(error) = Replacer::new(&ReplaceSpec {
            use_regex: true,
            ..spec("(unclosed", "x")
        }) else {
            panic!("an invalid expression must be refused");
        };
        assert!(
            error.to_string().contains("regular expression is invalid"),
            "{error}"
        );
    }

    #[test]
    fn empty_regex_matches_change_nothing() {
        let done = replace(
            &ReplaceSpec {
                use_regex: true,
                ..spec("x*", "y")
            },
            "abc\nxx\n",
        );
        assert_eq!(done.content, "abc\ny\n");
        assert_eq!(done.count, 1);
    }

    #[test]
    fn preserve_case_follows_each_match() {
        let done = replace(
            &ReplaceSpec {
                preserve_case: true,
                ..spec("value", "result")
            },
            "value Value VALUE vAlUe\n",
        );
        assert_eq!(done.content, "result Result RESULT result\n");
        assert_eq!(preserve_case("A", "next"), "Next");
        assert_eq!(preserve_case("_1", "next"), "next");
    }

    #[test]
    fn whole_word_and_case_options_narrow_the_matches() {
        let done = replace(
            &ReplaceSpec {
                whole_word: true,
                case_sensitive: true,
                ..spec("id", "key")
            },
            "id idle ID id_x (id)\n",
        );
        assert_eq!(done.content, "key idle ID id_x (key)\n");
    }

    #[test]
    fn only_replaces_the_match_search_reported_there() {
        let replacer = Replacer::new(&spec("é", "e")).unwrap();
        let content = "é é\né\n";
        let done = replacer
            .replace(
                content,
                Some(MatchPosition {
                    line_number: 1,
                    column_number: 3,
                }),
                MAX_PREVIEW_LINES_PER_FILE,
                usize::MAX,
            )
            .unwrap();
        assert_eq!(done.content, "é e\né\n");
        assert_eq!(done.count, 1);
        assert_eq!(done.lines[0].column_number, 3);
        let gone = replacer
            .replace(
                content,
                Some(MatchPosition {
                    line_number: 1,
                    column_number: 2,
                }),
                MAX_PREVIEW_LINES_PER_FILE,
                usize::MAX,
            )
            .unwrap();
        assert_eq!(gone.count, 0);
        assert_eq!(gone.content, content);
    }

    #[test]
    fn previews_stop_at_their_bound_but_every_match_is_replaced() {
        let content = "x\n".repeat(MAX_PREVIEW_LINES_PER_FILE + 5);
        let done = replace(&spec("x", "y"), &content);
        assert_eq!(done.count, (MAX_PREVIEW_LINES_PER_FILE + 5) as u64);
        assert_eq!(done.lines.len(), MAX_PREVIEW_LINES_PER_FILE);
        assert!(done.lines_truncated);
        assert_eq!(done.content, "y\n".repeat(MAX_PREVIEW_LINES_PER_FILE + 5));
    }

    #[test]
    fn a_replacement_past_its_bound_is_refused() {
        assert!(Replacer::new(&spec("a", &"b".repeat(MAX_REPLACEMENT_BYTES + 1))).is_err());
        assert!(Replacer::new(&spec("  ", "b")).is_err());
    }

    #[test]
    fn a_line_break_in_the_replacement_is_refused() {
        for replacement in ["a\rb", "a\nb", "\r\n"] {
            assert!(
                matches!(
                    Replacer::new(&spec("a", replacement)),
                    Err(AppError::InvalidRequest(_))
                ),
                "{replacement:?}"
            );
        }
    }

    #[test]
    fn a_numbered_group_ends_with_its_digits() {
        let done = replace(
            &ReplaceSpec {
                use_regex: true,
                ..spec("(o)(l)d", "$1a$2_$$1${1}x$name")
            },
            "old\n",
        );
        assert_eq!(done.content, "oal_$1ox\n");
        assert_eq!(braced_group_numbers("$12b$$3$"), "${12}b$$3$");
    }

    /// Every character becomes 2 KB, so 1 MB would build about 2 GB: the
    /// Replace stops as soon as the output passes its budget instead.
    #[test]
    fn output_growth_stops_at_the_byte_budget() {
        let replacer = Replacer::new(&ReplaceSpec {
            use_regex: true,
            ..spec(".", &"r".repeat(MAX_REPLACEMENT_BYTES))
        })
        .unwrap();
        let content = "x".repeat(1024 * 1024);
        let budget = 64 * 1024;
        let Err(error) = replacer.replace(&content, None, 0, budget) else {
            panic!("the output must stop at its budget");
        };
        assert!(matches!(error, AppError::ResourceLimit(_)), "{error}");
        // Many lines each within budget are refused once their total is not.
        let lines = "x\n".repeat(budget);
        assert!(replacer.replace(&lines, None, 0, budget).is_err());
        // A `$0`-heavy replacement is bounded before it expands.
        let echo = Replacer::new(&ReplaceSpec {
            use_regex: true,
            ..spec(".+", &"$0".repeat(1000))
        })
        .unwrap();
        assert!(echo.replace(&content, None, 0, budget).is_err());
        // Within budget the same Replace runs.
        let small = replace(&spec("x", "yy"), "xx\nx\n");
        assert_eq!(small.content, "yyyy\nyy\n");
    }
}
