pub(crate) use crate::domain::HarnessCharacterisation as Characterisation;

use super::release_version::characterise_against;

/// The releases each harness was tested with. Later releases keep the tested
/// features (see [`characterise_against`]).
const CODEX_TESTED_VERSION: &str = crate::acp::bindings::codex::CODEX_PINNED_VERSION;

const CLAUDE_ADAPTER_TESTED_VERSION: &str = "0.76.0";

const CLAUDE_ACP_SDK_TESTED_VERSION: &str = "1.4.0";

const OPENCODE_TESTED_VERSION: &str = "1.18.21";

pub(crate) const NODE_ENGINE_FLOOR_MAJOR: u32 = 22;

const ENGINE_RANGE_BYTES_MAX: usize = 64;

pub(crate) fn valid_numeric_version(
    value: &str,
    minimum_components: usize,
    maximum_components: usize,
) -> bool {
    let mut component_count = 0;
    value.split('.').all(|component| {
        component_count += 1;
        !component.is_empty()
            && component.len() <= 10
            && component.bytes().all(|byte| byte.is_ascii_digit())
    }) && (minimum_components..=maximum_components).contains(&component_count)
}

pub(crate) fn codex_characterisation(reported_version: &str) -> Characterisation {
    characterise_against(reported_version, CODEX_TESTED_VERSION)
}

pub(crate) fn opencode_characterisation(reported_version: &str) -> Characterisation {
    characterise_against(reported_version, OPENCODE_TESTED_VERSION)
}

/// The Claude ACP adapter and the ACP SDK it bundles each move on their own;
/// the pair is as old as its older half.
pub(crate) fn claude_characterisation(
    adapter_version: &str,
    acp_sdk_version: &str,
) -> Characterisation {
    match (
        characterise_against(adapter_version, CLAUDE_ADAPTER_TESTED_VERSION),
        characterise_against(acp_sdk_version, CLAUDE_ACP_SDK_TESTED_VERSION),
    ) {
        (Characterisation::Characterised, Characterisation::Characterised) => {
            Characterisation::Characterised
        }
        (adapter, sdk) if adapter.runs_tested_features() && sdk.runs_tested_features() => {
            Characterisation::NewerThanTested
        }
        _ => Characterisation::Uncharacterised,
    }
}

pub(crate) fn node_engine_minimum_major(declared_range: &str) -> Option<u32> {
    if declared_range.len() > ENGINE_RANGE_BYTES_MAX {
        return None;
    }
    let rest = declared_range.trim().strip_prefix(">=")?.trim_start();
    let major = rest.split('.').next()?;
    if major.is_empty() || major.len() > 10 || !major.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let valid_tail = rest
        .split('.')
        .skip(1)
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    valid_tail.then(|| major.parse().ok()).flatten()
}

pub(crate) fn node_satisfies_declared_engine(node_major: u32, declared_range: &str) -> bool {
    node_engine_minimum_major(declared_range)
        .is_some_and(|declared| node_major >= declared.max(NODE_ENGINE_FLOOR_MAJOR))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::acp::bindings::codex::CODEX_PINNED_VERSION;

    #[test]
    fn a_codex_version_is_read_from_either_the_bare_or_prefixed_form() {
        assert_eq!(
            codex_characterisation(&format!("codex-cli {CODEX_PINNED_VERSION}")),
            Characterisation::Characterised
        );
        assert_eq!(
            codex_characterisation(CODEX_PINNED_VERSION),
            Characterisation::Characterised
        );
    }

    #[test]
    fn a_later_codex_release_keeps_the_tested_features_and_an_earlier_one_does_not() {
        let [major, minor, patch] = CODEX_PINNED_VERSION
            .split('.')
            .map(|part| part.parse::<u32>().unwrap())
            .collect::<Vec<_>>()[..]
        else {
            panic!("the pinned version has three components");
        };
        let previous_minor = minor
            .checked_sub(1)
            .expect("the pinned version has an earlier minor release");
        for version in [
            format!("{major}.{minor}.{}", patch + 1),
            format!("{major}.{}.0", minor + 1),
            format!("{major}.{}.0-alpha.1", minor + 1),
            format!("{}.0.0", major + 1),
        ] {
            assert_eq!(
                codex_characterisation(&version),
                Characterisation::NewerThanTested,
                "{version}"
            );
        }
        for version in [
            format!("{major}.{previous_minor}.0"),
            format!("{CODEX_PINNED_VERSION}-rc.1"),
            "0.146.0".to_owned(),
            String::new(),
            "codex-cli".to_owned(),
            "latest".to_owned(),
        ] {
            assert_eq!(
                codex_characterisation(&version),
                Characterisation::Uncharacterised,
                "{version}"
            );
        }
    }

    #[test]
    fn the_recorded_claude_adapter_and_sdk_pair_is_characterised() {
        assert_eq!(
            claude_characterisation("0.76.0", "1.4.0"),
            Characterisation::Characterised
        );
    }

    #[test]
    fn the_recorded_opencode_build_is_characterised() {
        assert_eq!(
            opencode_characterisation("1.18.21"),
            Characterisation::Characterised
        );
        assert_eq!(
            opencode_characterisation("opencode 1.18.21"),
            Characterisation::Characterised
        );
    }

    #[test]
    fn a_later_opencode_release_is_newer_than_tested_and_an_earlier_one_is_uncharacterised() {
        for version in ["1.18.22", "1.19.0", "2.0.0"] {
            assert_eq!(
                opencode_characterisation(version),
                Characterisation::NewerThanTested,
                "{version}"
            );
        }
        for version in ["1.18.20", "1.17.21", "1.18.21-rc.1", "1.18", "", "latest"] {
            assert_eq!(
                opencode_characterisation(version),
                Characterisation::Uncharacterised,
                "{version}"
            );
        }
    }

    #[test]
    fn a_claude_adapter_pair_is_as_old_as_its_older_half() {
        for (adapter, sdk, expected) in [
            ("0.76.0", "1.5.0", Characterisation::NewerThanTested),
            ("0.77.0", "1.4.0", Characterisation::NewerThanTested),
            ("0.77.0", "1.3.9", Characterisation::Uncharacterised),
            ("0.75.0", "1.5.0", Characterisation::Uncharacterised),
        ] {
            assert_eq!(
                claude_characterisation(adapter, sdk),
                expected,
                "{adapter} {sdk}"
            );
        }
    }

    #[test]
    fn the_engine_range_this_adapter_actually_declares_parses_to_its_major() {
        assert_eq!(node_engine_minimum_major(">=22"), Some(22));
        assert_eq!(node_engine_minimum_major(">=22.0.0"), Some(22));
        assert_eq!(node_engine_minimum_major(">= 24.1"), Some(24));
    }

    #[test]
    fn an_engine_range_sage_cannot_evaluate_is_not_treated_as_a_pass() {
        for declared in ["^22", "~22.1", "22.x", ">=22 || >=20", "*", "", ">=", ">=a"] {
            assert_eq!(
                node_engine_minimum_major(declared),
                None,
                "{declared} must not be silently accepted"
            );
            assert!(!node_satisfies_declared_engine(24, declared));
        }
    }

    #[test]
    fn sages_own_node_floor_outranks_a_lower_declared_engine() {
        assert!(!node_satisfies_declared_engine(20, ">=18"));
        assert!(node_satisfies_declared_engine(22, ">=18"));
        assert!(!node_satisfies_declared_engine(22, ">=24"));
        assert!(node_satisfies_declared_engine(24, ">=24"));
    }
}
