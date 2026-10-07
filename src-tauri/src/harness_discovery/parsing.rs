use super::version_probe::StreamOutput;
use super::{HarnessExecutable, HarnessVersion};

#[cfg(test)]
pub(super) fn parse_harness_version(
    harness: HarnessExecutable,
    output: &StreamOutput,
) -> Option<HarnessVersion> {
    let version = parse_harness_version_text(harness, output)?;
    if harness == HarnessExecutable::CursorAgent {
        parse_date_build(&version)
    } else {
        parse_version(&version)
    }
}

pub(super) fn parse_harness_display_version(
    harness: HarnessExecutable,
    output: &StreamOutput,
) -> Option<String> {
    let version = parse_harness_version_text(harness, output)?;
    if harness == HarnessExecutable::Codex && valid_codex_prerelease(&version) {
        return Some(version);
    }
    if harness == HarnessExecutable::CursorAgent {
        return parse_date_build(&version).map(|version| version.to_string());
    }
    parse_version(&version).map(|version| version.to_string())
}

fn parse_harness_version_text(harness: HarnessExecutable, output: &StreamOutput) -> Option<String> {
    let output = std::str::from_utf8(&output.bytes).ok()?.trim();
    if output.is_empty() || output.len() > 256 || output.lines().count() != 1 {
        return None;
    }
    let version = match harness {
        HarnessExecutable::Codex => output.strip_prefix("codex-cli ")?,
        HarnessExecutable::Claude => output.strip_suffix(" (Claude Code)")?,
        HarnessExecutable::Pi | HarnessExecutable::Gemini | HarnessExecutable::OpenCode => output,
        HarnessExecutable::OhMyPi => output.strip_prefix("omp ")?,
        HarnessExecutable::CursorAgent => output,
    };
    Some(version.to_owned())
}

fn valid_codex_prerelease(value: &str) -> bool {
    let Some((base, suffix)) = value.split_once('-') else {
        return false;
    };
    parse_version(base).is_some()
        && !suffix.is_empty()
        && suffix.len() <= 64
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
}

fn parse_date_build(value: &str) -> Option<HarnessVersion> {
    let (date, build) = value.split_once('-')?;
    if build.is_empty() || build.len() > 64 || !build.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut components = date.split('.');
    let major = parse_numeric(components.next()?)?;
    let minor = parse_numeric(components.next()?)?;
    let patch = parse_numeric(components.next()?)?;
    components.next().is_none().then_some(HarnessVersion {
        major,
        minor,
        patch,
    })
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
    (component == "0" || !component.starts_with('0'))
        .then(|| parse_numeric(component))
        .flatten()
}

fn parse_numeric(component: &str) -> Option<u32> {
    (!component.is_empty()
        && component.len() <= 10
        && component.bytes().all(|byte| byte.is_ascii_digit()))
    .then(|| component.parse().ok())
    .flatten()
}
