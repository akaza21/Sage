use super::version_probe::StreamOutput;
use super::{HarnessExecutable, HarnessVersion};

pub(super) fn parse_harness_version(
    harness: HarnessExecutable,
    output: &StreamOutput,
) -> Option<HarnessVersion> {
    let output = std::str::from_utf8(&output.bytes).ok()?.trim();
    if output.is_empty() || output.len() > 256 || output.lines().count() != 1 {
        return None;
    }
    let version = match harness {
        HarnessExecutable::Codex => output.strip_prefix("codex-cli ")?,
        HarnessExecutable::Claude => output.strip_suffix(" (Claude Code)")?,
    };
    parse_version(version)
}

pub(super) fn normalized_legacy_version(output: &StreamOutput) -> Option<String> {
    if output.truncated {
        return None;
    }
    let output = std::str::from_utf8(&output.bytes).ok()?.trim();
    let version = output.lines().next()?.trim();
    (!version.is_empty() && version.len() <= 512).then(|| version.to_owned())
}

fn parse_version(value: &str) -> Option<HarnessVersion> {
    let mut components = value.split('.');
    let major = parse_component(components.next()?)?;
    let minor = parse_component(components.next()?)?;
    let patch = parse_component(components.next()?)?;
    components.next().is_none().then_some(HarnessVersion {
        major,
        minor,
        patch,
    })
}

fn parse_component(component: &str) -> Option<u32> {
    (!component.is_empty()
        && component.len() <= 10
        && component.bytes().all(|byte| byte.is_ascii_digit())
        && (component == "0" || !component.starts_with('0')))
    .then(|| component.parse().ok())
    .flatten()
}
