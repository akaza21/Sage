//! Refusal policy for keys listed by Git, including bounded, redacted diagnostics.

use std::{collections::BTreeSet, fmt::Write as _};

const MAX_NAMED_KEYS: usize = 3;
const MAX_SUBSECTION_CHARS: usize = 64;

/// Builds the public refusal for a NUL-separated `git config --name-only`
/// listing, or `None` when no listed key runs a command. Keys are classified
/// as bytes: Git accepts subsections that are not UTF-8.
pub(super) fn refusal_message(listing: &[u8]) -> Option<String> {
    let named = named_keys(listing)?;
    Some(format!(
        "Sage did not run Git here because this repository's Git config sets keys that run \
         programs ({named}). Remove them from the repository config, or use the terminal for \
         Git in this folder."
    ))
}

/// Git's checks that keep index paths from naming an entry Windows (NTFS)
/// or macOS (HFS+) would resolve to something else, by canonical key.
const PATH_PROTECTIONS: [&str; 2] = ["core.protectntfs", "core.protecthfs"];

/// The path-protection keys the listing sets, whatever their value; the
/// caller reads each value and refuses anything but `true`.
pub(super) fn path_protection_keys(listing: &[u8]) -> Vec<&'static str> {
    PATH_PROTECTIONS
        .into_iter()
        .filter(|key| {
            listing
                .split(|byte| *byte == 0)
                .any(|listed| listed.eq_ignore_ascii_case(key.as_bytes()))
        })
        .collect()
}

pub(super) fn path_protection_refusal(key: &str) -> String {
    let name = match key {
        "core.protecthfs" => "core.protectHFS",
        _ => "core.protectNTFS",
    };
    format!(
        "Sage did not run Git here because this repository turns off \
         Git's path protection ({name}). Remove that setting, or use the terminal for Git in \
         this folder."
    )
}

/// The bounded, redacted list of command-running keys, or `None`.
fn named_keys(listing: &[u8]) -> Option<String> {
    let keys = listing
        .split(|byte| *byte == 0)
        .filter_map(ConfigKey::parse)
        .filter(ConfigKey::runs_command)
        .map(|key| key.display_name())
        .collect::<BTreeSet<_>>();
    if keys.is_empty() {
        return None;
    }
    let mut named = keys
        .iter()
        .take(MAX_NAMED_KEYS)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let remaining = keys.len().saturating_sub(MAX_NAMED_KEYS);
    if remaining > 0 {
        let _ = write!(named, " and {remaining} more");
    }
    Some(named)
}

/// A `section[.subsection].name` key as `git config --name-only` prints it.
#[derive(Debug, PartialEq, Eq)]
struct ConfigKey<'a> {
    section: Vec<u8>,
    subsection: Option<&'a [u8]>,
    name: Vec<u8>,
}

impl<'a> ConfigKey<'a> {
    fn parse(key: &'a [u8]) -> Option<Self> {
        let first = key.iter().position(|byte| *byte == b'.')?;
        let last = key.iter().rposition(|byte| *byte == b'.')?;
        let section = &key[..first];
        let name = &key[last + 1..];
        let subsection = (last > first).then(|| &key[first + 1..last]);
        (!section.is_empty() && !name.is_empty()).then(|| Self {
            section: section.to_ascii_lowercase(),
            subsection,
            name: name.to_ascii_lowercase(),
        })
    }

    /// Keys whose value Git runs as a program, plus includes, whose target
    /// files and conditions this check does not follow. Keys Sage overrides
    /// on every invocation (`core.hooksPath`, `core.fsmonitor`, and
    /// `credential.helper`, whose empty command-line value clears every
    /// helper the config files set, URL-scoped `credential.<url>.helper`
    /// included) are absent.
    fn runs_command(&self) -> bool {
        if self.is_remote_helper_alias() {
            return true;
        }
        let scoped = self.subsection.is_some();
        match (self.section.as_slice(), self.name.as_slice()) {
            (b"include", _) | (b"includeif", _) => true,
            (b"filter", b"clean" | b"smudge" | b"process") => scoped,
            (b"diff", b"textconv" | b"command") => scoped,
            (b"diff", b"external") => !scoped,
            (b"merge", b"driver") => scoped,
            (
                b"core",
                b"sshcommand"
                | b"askpass"
                | b"gitproxy"
                | b"alternaterefscommand"
                | b"editor"
                | b"pager",
            ) => !scoped,
            (b"remote", b"uploadpack" | b"receivepack" | b"vcs") => scoped,
            (b"sequence", b"editor") => !scoped,
            (b"gpg", b"program" | b"defaultkeycommand") => true,
            _ => false,
        }
    }

    /// Even an admitted protocol can be forced through `ssh::url`, for
    /// example. If `git-remote-ssh` is absent, Git falls back to this alias.
    /// Other aliases cannot shadow the builtins Sage invokes.
    fn is_remote_helper_alias(&self) -> bool {
        self.section == b"alias"
            && self
                .subsection
                .unwrap_or(&self.name)
                .get(..7)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"remote-"))
    }

    /// The key as shown to the user. Subsections are user-controlled (and a
    /// credential subsection can be a URL with a token), so only plain
    /// identifiers are echoed.
    fn display_name(&self) -> String {
        // Alias names are repository-controlled command names, potentially
        // very long or split across a subsection. Never echo them or values.
        if self.is_remote_helper_alias() {
            return "alias.remote-*".to_owned();
        }
        let section = String::from_utf8_lossy(&self.section);
        let name = String::from_utf8_lossy(&self.name);
        match self.subsection.and_then(plain_identifier) {
            Some(subsection) => format!("{section}.{subsection}.{name}"),
            None if self.subsection.is_some() => format!("{section}.*.{name}"),
            None => format!("{section}.{name}"),
        }
    }
}

fn plain_identifier(bytes: &[u8]) -> Option<&str> {
    let plain = !bytes.is_empty()
        && bytes.len() <= MAX_SUBSECTION_CHARS
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    plain.then(|| std::str::from_utf8(bytes).ok()).flatten()
}

#[cfg(test)]
mod tests;
