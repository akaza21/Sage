//! Release versions a harness reports, and where a release sits relative to
//! the release Sage was tested with.
//!
//! Harnesses update on their own schedule. A release newer than the tested
//! one keeps every feature whose wire shape it still speaks; only a release
//! older than the tested one, or one whose version cannot be read, loses the
//! tested features.

use std::cmp::Ordering;

use crate::domain::HarnessCharacterisation;

const COMPONENT_DIGITS_MAX: usize = 10;
const PRERELEASE_BYTES_MAX: usize = 64;

/// A `major.minor.patch` release with an optional `-prerelease` tag. Build
/// metadata after `+` is accepted and ignored, as semantic versioning
/// specifies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReleaseVersion {
    core: (u64, u64, u64),
    prerelease: Option<String>,
}

impl ReleaseVersion {
    /// Reads the last whitespace-separated token, so both `0.168.0` and a
    /// `codex-cli 0.168.0` version line parse.
    pub(crate) fn parse(reported: &str) -> Option<Self> {
        let token = reported.split_ascii_whitespace().next_back()?;
        let token = token.split_once('+').map_or(token, |(version, _)| version);
        let (core, prerelease) = match token.split_once('-') {
            Some((core, tag)) => (core, Some(tag)),
            None => (token, None),
        };
        let mut parts = core.split('.').map(component);
        let parsed = (parts.next()??, parts.next()??, parts.next()??);
        if parts.next().is_some() {
            return None;
        }
        let prerelease = match prerelease {
            None => None,
            Some(tag)
                if !tag.is_empty()
                    && tag.len() <= PRERELEASE_BYTES_MAX
                    && tag.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-')
                    }) =>
            {
                Some(tag.to_owned())
            }
            Some(_) => return None,
        };
        Some(Self {
            core: parsed,
            prerelease,
        })
    }

    pub(crate) fn is_prerelease(&self) -> bool {
        self.prerelease.is_some()
    }

    /// Whether both releases share a `major.minor` line.
    pub(crate) fn same_minor_line(&self, other: &Self) -> bool {
        (self.core.0, self.core.1) == (other.core.0, other.core.1)
    }
}

fn component(value: &str) -> Option<u64> {
    (!value.is_empty()
        && value.len() <= COMPONENT_DIGITS_MAX
        && value.bytes().all(|byte| byte.is_ascii_digit()))
    .then(|| value.parse().ok())
    .flatten()
}

impl Ord for ReleaseVersion {
    /// A prerelease sorts before its release; two prereleases of the same
    /// release compare by tag.
    fn cmp(&self, other: &Self) -> Ordering {
        self.core
            .cmp(&other.core)
            .then_with(|| match (&self.prerelease, &other.prerelease) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(left), Some(right)) => left.cmp(right),
            })
    }
}

impl PartialOrd for ReleaseVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Places a reported release against the release Sage was tested with: the
/// tested release itself, a later one, or an earlier or unreadable one.
pub(crate) fn characterise_against(reported: &str, tested: &str) -> HarnessCharacterisation {
    let (Some(reported), Some(tested)) = (
        ReleaseVersion::parse(reported),
        ReleaseVersion::parse(tested),
    ) else {
        return HarnessCharacterisation::Uncharacterised;
    };
    match reported.cmp(&tested) {
        Ordering::Equal => HarnessCharacterisation::Characterised,
        Ordering::Greater => HarnessCharacterisation::NewerThanTested,
        Ordering::Less => HarnessCharacterisation::Uncharacterised,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_order_by_release_with_prereleases_before_their_release() {
        let ordered = [
            "0.166.9",
            "0.167.0-alpha.1",
            "0.167.0-rc.1",
            "0.167.0",
            "0.167.1",
            "0.168.0-alpha.2",
            "0.168.0",
            "1.0.0",
        ]
        .map(|version| ReleaseVersion::parse(version).unwrap());
        assert!(ordered.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(
            ReleaseVersion::parse("codex-cli 0.167.0+build.5"),
            ReleaseVersion::parse("0.167.0")
        );
    }

    #[test]
    fn an_unreadable_version_does_not_parse() {
        for reported in [
            "",
            "   ",
            "codex-cli",
            "0.167",
            "0.167.0.1",
            "latest",
            "0.167.x",
            "0.167.0-",
            "0.167.0-rc 1x",
            "00000000001.0.0",
        ] {
            assert_eq!(ReleaseVersion::parse(reported), None, "{reported:?}");
        }
    }

    #[test]
    fn a_release_is_tested_newer_or_uncharacterised_against_the_tested_release() {
        for (reported, expected) in [
            ("codex-cli 0.167.0", HarnessCharacterisation::Characterised),
            ("0.167.0+build.5", HarnessCharacterisation::Characterised),
            ("0.167.1", HarnessCharacterisation::NewerThanTested),
            ("0.170.0-alpha.3", HarnessCharacterisation::NewerThanTested),
            ("1.0.0", HarnessCharacterisation::NewerThanTested),
            ("0.167.0-rc.1", HarnessCharacterisation::Uncharacterised),
            ("0.166.9", HarnessCharacterisation::Uncharacterised),
            ("latest", HarnessCharacterisation::Uncharacterised),
        ] {
            assert_eq!(
                characterise_against(reported, "0.167.0"),
                expected,
                "{reported}"
            );
        }
    }
}
