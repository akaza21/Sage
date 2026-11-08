//! Workspace files a prompt names with `@path` (or `@"path with spaces"`),
//! as the composer's `@` picker writes them and as Claude Code users type
//! them. The token stays in the prompt text; each one that names a file in
//! one of the project's roots also goes to the harness as a content block:
//! an embedded `resource` with the file's text when the session advertises
//! embedded context and the file fits the dispatch budget, otherwise a
//! `resource_link` (the ACP baseline every agent accepts). Nothing is
//! persisted: the durable prompt keeps only its text.
use super::attached_context::files::{FILE_BYTES, secrets::path_is_secret};
use crate::workspace::{Workspace, open_workspace_text_file, validate_contract_path};
use std::{io::Read, path::Path, sync::Arc};

/// Distinct files one prompt sends; later mentions stay text only.
pub(crate) const MENTION_COUNT: usize = 16;
/// `@` tokens looked up per prompt, so a prompt full of `@` words never
/// costs more than this many path probes per root.
const PROBE_COUNT: usize = 4 * MENTION_COUNT;
/// The longest token that can name a file; the path index holds no longer
/// relative path.
const TOKEN_BYTES: usize = 4096;
/// Sentence punctuation a typed mention may end with ("see @a.rs.").
const TRAILING: &[char] = &['.', ',', ';', ':', '!', '?', ')', ']', '}', '\'', '"'];

/// One mentioned file as the harness receives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Mention {
    /// `file://` URI of the file inside its root.
    uri: String,
    name: String,
    size: u64,
    /// The file's text, read when embedded context was advertised and it fit.
    text: Option<String>,
}

/// The mentioned files of one prompt, in the order the prompt names them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Mentions(Option<Arc<[Mention]>>);

impl Mentions {
    pub(crate) fn retained_bytes(&self) -> usize {
        self.0.as_ref().map_or(0, |all| {
            std::mem::size_of_val(&**all)
                + all
                    .iter()
                    .map(|m| m.uri.len() + m.name.len() + m.text.as_ref().map_or(0, String::len))
                    .sum::<usize>()
        })
    }
    /// The content blocks, embedded only while `embedded` is still offered.
    pub(crate) fn blocks(&self, embedded: bool) -> impl Iterator<Item = serde_json::Value> + '_ {
        self.0.iter().flat_map(|all| all.iter()).map(move |m| {
            match m.text.as_deref().filter(|_| embedded) {
                Some(text) => serde_json::json!({
                    "type": "resource",
                    "resource": {"uri": m.uri, "text": text},
                }),
                None => serde_json::json!({
                    "type": "resource_link",
                    "uri": m.uri,
                    "name": m.name,
                    "size": m.size,
                }),
            }
        })
    }
}

impl super::intent::Prompt {
    /// The prompt's ACP content: its text, staged files, then mentions.
    pub(crate) fn wire_blocks(
        &self,
        embedded: bool,
    ) -> Result<Vec<serde_json::Value>, &'static str> {
        let mut blocks = self.resources.wire(self.as_str(), embedded)?;
        blocks.extend(self.mentions.blocks(embedded));
        Ok(blocks)
    }
}

/// The `@` tokens of `text` that could name a file, in order and without
/// repeats: each starts the text or follows whitespace, and is either quoted
/// (`@"a b.txt"`) or runs to the next whitespace.
pub(crate) fn mention_tokens(text: &str) -> Vec<&str> {
    let mut tokens: Vec<&str> = Vec::new();
    let mut previous = None;
    for (index, character) in text.char_indices() {
        let starts = character == '@' && previous.is_none_or(char::is_whitespace);
        previous = Some(character);
        if !starts {
            continue;
        }
        let rest = &text[index + 1..];
        let token = match rest.strip_prefix('"') {
            Some(quoted) => match quoted.find(['"', '\n']) {
                Some(end) if quoted[end..].starts_with('"') => &quoted[..end],
                _ => continue,
            },
            None => rest.split(char::is_whitespace).next().unwrap_or_default(),
        };
        if token.is_empty() || token.len() > TOKEN_BYTES || tokens.contains(&token) {
            continue;
        }
        tokens.push(token);
        if tokens.len() == PROBE_COUNT {
            break;
        }
    }
    tokens
}

/// Resolves `tokens` against `roots` in root order; a token that names no
/// regular file inside a root stays text. Embedded text is read only when
/// `embed` is set, for files no larger than the per-file cap and while
/// `budget` bytes remain; credential files are only ever linked.
pub(crate) fn resolve(
    tokens: &[String],
    roots: &[Workspace],
    embed: bool,
    budget: usize,
) -> Mentions {
    let mut budget = budget;
    let mut found: Vec<Mention> = Vec::new();
    for token in tokens {
        if found.len() == MENTION_COUNT {
            break;
        }
        let trimmed = token.trim_end_matches(TRAILING);
        let candidates = [token.as_str(), trimmed];
        let mention = candidates
            .iter()
            .filter(|path| !path.is_empty())
            .find_map(|path| {
                roots
                    .iter()
                    .find_map(|root| mention(root, path, embed, &mut budget))
            });
        if let Some(mention) = mention
            && !found.iter().any(|seen| seen.uri == mention.uri)
        {
            found.push(mention);
        }
    }
    Mentions((!found.is_empty()).then(|| found.into()))
}

fn mention(root: &Workspace, relative: &str, embed: bool, budget: &mut usize) -> Option<Mention> {
    validate_contract_path(relative, false).ok()?;
    // Opened once beneath the held root, no-follow: a link cannot lead out.
    let file = open_workspace_text_file(root.root(), root.root_identity(), relative, None).ok()?;
    let size = file.metadata().ok()?.len();
    let path = root.root().join(relative);
    let uri = url::Url::from_file_path(&path).ok()?.to_string();
    let name = relative.rsplit('/').next().unwrap_or(relative).to_owned();
    let cap = FILE_BYTES.min(*budget);
    let text = (embed && size <= cap as u64 && !path_is_secret(Path::new(relative), false))
        .then(|| read_text(file, cap))
        .flatten();
    if let Some(text) = &text {
        *budget -= text.len();
    }
    Some(Mention {
        uri,
        name,
        size,
        text,
    })
}

/// The file's text when it is UTF-8 and still within `cap` as read.
fn read_text(file: std::fs::File, cap: usize) -> Option<String> {
    let mut bytes = Vec::new();
    file.take(cap as u64 + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() > cap {
        return None;
    }
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
#[path = "prompt_mentions_tests.rs"]
mod tests;
