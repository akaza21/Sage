//! Workspace files a prompt names with `@path` (or `@"path with spaces"`),
//! as the composer's `@` picker writes them and as Claude Code users type
//! them. A file in a root other than the first is written `@root:path`,
//! where `root` is that root's id or, when no other root shares it, its
//! display name; a bare `@path` takes the first root that holds the file.
//! The token stays in the prompt text; each one that names a file also goes
//! to the harness as a content block: an embedded `resource` with the file's
//! text when the session advertises embedded context and the file fits the
//! dispatch budget, otherwise a `resource_link` (the ACP baseline every agent
//! accepts). Nothing is persisted: the durable prompt keeps only its text.
use super::attached_context::files::{Resources, file_is_excluded};
use crate::acp::prompt_files::secrets::path_is_secret;
use crate::workspace::{
    RootIdentity, Workspace, decode_text_bytes, leaf_identity, open_workspace_text_file,
    search::handle_contract_relative, validate_contract_path,
};
use sage_core::limits::prompt_files::{DISPATCH_BYTES, FILE_BYTES};
use std::{fs::File, io::Read, path::Path, sync::Arc};

/// Distinct files one prompt sends; later mentions stay text only.
pub(crate) const MENTION_COUNT: usize = 16;
/// Spellings looked up per prompt (a token, and its form without trailing
/// punctuation when that differs), so a prompt full of `@` words never costs
/// more than this many lookups in each root.
const PROBE_COUNT: usize = 4 * MENTION_COUNT;
/// The longest token that can name a file; the path index holds no longer
/// relative path.
const TOKEN_BYTES: usize = 4096;
const TRAILING: &[char] = &['.', ',', ';', ':', '!', '?', ')', ']', '}', '\'', '"'];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Mention {
    uri: String,
    name: String,
    size: u64,
    /// The file's text, read when embedded context was advertised and it fit.
    text: Option<String>,
}

/// The mentioned files of one prompt, in the order the prompt names them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Mentions(Option<Arc<[Mention]>>);

impl Mention {
    fn link_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.uri.len() + self.name.len()
    }
}

impl Mentions {
    pub(crate) fn retained_bytes(&self) -> usize {
        self.0.as_ref().map_or(0, |all| {
            all.iter()
                .map(|m| m.link_bytes() + m.text.as_ref().map_or(0, String::len))
                .sum()
        })
    }
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

impl crate::acp::transcript_quote::Prompt {
    /// The bytes the dispatch budget counts: everything the prompt holds
    /// but its images, which have a budget of their own, plus `files`.
    fn dispatch_bytes(&self, files: &Resources) -> usize {
        self.retained_bytes() - self.images.retained_bytes() + files.retained_bytes()
    }
    pub(crate) fn mention_budget(&self, files: &Resources) -> usize {
        DISPATCH_BYTES.saturating_sub(self.dispatch_bytes(files))
    }
    pub(crate) fn fits_dispatch(&self, files: &Resources) -> bool {
        self.dispatch_bytes(files) <= DISPATCH_BYTES
    }
}

/// The `@` tokens of `text` that could name a file, in order and without
/// repeats: each starts the text or follows whitespace or an opening
/// parenthesis that does, and is either quoted (`@"a b.txt"`) or runs to the
/// next whitespace.
pub(crate) fn mention_tokens(text: &str) -> Vec<&str> {
    let mut tokens: Vec<&str> = Vec::new();
    let (mut previous, mut before) = (None, None);
    for (index, character) in text.char_indices() {
        let opens = |c: Option<char>| c.is_none_or(char::is_whitespace);
        let starts =
            character == '@' && (opens(previous) || previous == Some('(') && opens(before));
        (before, previous) = (previous, Some(character));
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

pub(crate) struct MentionRoot {
    pub(crate) workspace: Workspace,
    pub(crate) name: String,
}

/// A credential file is only ever linked.
pub(crate) fn resolve(
    tokens: &[String],
    roots: &[MentionRoot],
    embed: bool,
    budget: usize,
) -> Mentions {
    let mut budget = budget;
    let mut probes = 0;
    let mut seen: Vec<RootIdentity> = Vec::new();
    let mut found: Vec<Mention> = Vec::new();
    for token in tokens {
        if found.len() == MENTION_COUNT || probes == PROBE_COUNT {
            break;
        }
        let trimmed = token.trim_end_matches(TRAILING);
        let spellings = std::iter::once(token.as_str())
            .chain((trimmed != token && !trimmed.is_empty()).then_some(trimmed));
        let opened = spellings.take(PROBE_COUNT - probes).find_map(|spelling| {
            probes += 1;
            let (roots, relative) = target(spelling, roots)?;
            roots
                .iter()
                .find_map(|root| open(&root.workspace, relative))
        });
        let Some((root, file, size, identity, relative)) = opened else {
            continue;
        };
        if seen.contains(&identity) {
            continue;
        }
        seen.push(identity);
        if let Some(mention) = mention(root, file, size, &relative, embed, &mut budget) {
            found.push(mention);
        }
    }
    Mentions((!found.is_empty()).then(|| found.into()))
}

/// `root:path` names a root by its id or unique display name; any other `:` (a Windows stream or drive) names nothing.
fn target<'a>(spelling: &'a str, roots: &'a [MentionRoot]) -> Option<(&'a [MentionRoot], &'a str)> {
    let Some((qualifier, relative)) = spelling.split_once(':') else {
        return Some((roots, spelling));
    };
    let by_id = roots
        .iter()
        .position(|r| r.workspace.root_id() == qualifier);
    let mut named = roots
        .iter()
        .enumerate()
        .filter(|(_, r)| r.name == qualifier);
    let index = by_id.or_else(|| match (named.next(), named.next()) {
        (Some((index, _)), None) => Some(index),
        _ => None,
    })?;
    Some((std::slice::from_ref(&roots[index]), relative))
}

type Opened<'a> = (&'a Workspace, File, u64, RootIdentity, String);

/// Opens `relative` beneath the held root, no-follow, so a link cannot lead
/// out; the result carries the file's identity and the path the filesystem
/// itself names it by (never a short name, stream or other alias as typed).
fn open<'a>(root: &'a Workspace, relative: &str) -> Option<Opened<'a>> {
    if relative.contains([':', '\0']) {
        return None;
    }
    validate_contract_path(relative, false).ok()?;
    let file = open_workspace_text_file(root.root(), root.root_identity(), relative, None).ok()?;
    let metadata = file.metadata().ok()?;
    let identity = leaf_identity(&file, &metadata).ok()?;
    let canonical = handle_contract_relative(root, &file).ok()?;
    Some((root, file, metadata.len(), identity, canonical))
}

fn mention(
    root: &Workspace,
    file: File,
    size: u64,
    relative: &str,
    embed: bool,
    budget: &mut usize,
) -> Option<Mention> {
    let path = root.root().join(relative);
    // The staged-file rules apply, failing closed: an excluded file is not sent.
    if file_is_excluded(&path).unwrap_or(true) {
        return None;
    }
    let mut mention = Mention {
        uri: url::Url::from_file_path(&path).ok()?.to_string(),
        name: relative.rsplit('/').next().unwrap_or(relative).to_owned(),
        size,
        text: None,
    };
    *budget = budget.checked_sub(mention.link_bytes())?;
    let cap = FILE_BYTES.min(*budget);
    if embed && size <= cap as u64 && !path_is_secret(Path::new(relative), false) {
        mention.text = read_text(file, cap);
    }
    *budget -= mention.text.as_ref().map_or(0, String::len);
    Some(mention)
}

/// The file's text when it is text by the text view's rule (UTF-8, no NUL)
/// and still within `cap` as read.
fn read_text(file: File, cap: usize) -> Option<String> {
    let mut bytes = Vec::new();
    file.take(cap as u64 + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() > cap {
        return None;
    }
    decode_text_bytes(bytes).ok()
}

#[cfg(test)]
#[path = "prompt_mentions_tests.rs"]
mod tests;
