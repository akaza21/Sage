use std::ffi::OsString;

#[cfg(test)]
use serde_json::Value;
use serde_json::json;

use crate::acp::bindings::adapter::AdapterRuntimeError;

use super::profile::StandardAcpProfile;

const CONFIG_CONTENT_KEY: &str = "OPENCODE_CONFIG_CONTENT";

pub(super) fn launch_environment(
    profile: &StandardAcpProfile,
    read: impl Fn(&str) -> Option<OsString>,
) -> Result<Vec<(OsString, OsString)>, AdapterRuntimeError> {
    let mut entries = super::environment::launch_environment(profile, read)
        .ok_or(AdapterRuntimeError::LaunchUnavailable)?;
    entries.push(launch_config()?);
    Ok(entries)
}

fn launch_config() -> Result<(OsString, OsString), AdapterRuntimeError> {
    // Every non-read action must round trip through Sage before execution.
    let config = json!({"permission": {
        "*": "ask",
        "bash": "ask",
        "edit": "ask",
        "read": "allow",
        "task": "ask",
        "webfetch": "ask",
        "write": "ask"
    }});
    let content =
        serde_json::to_string(&config).map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
    Ok((OsString::from(CONFIG_CONTENT_KEY), OsString::from(content)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inherited_opencode_config_cannot_introduce_unadmitted_mcp_or_permissions() {
        let profile = super::super::profile_for("opencode").expect("OpenCode profile");
        let inherited = r#"{"mcp":{"unauthorized":{"type":"remote","url":"https://example.com"}},"permission":{"*":"allow"}}"#;
        let entries = launch_environment(profile, |key| {
            if key == CONFIG_CONTENT_KEY {
                Some(OsString::from(inherited))
            } else {
                std::env::var_os(key)
            }
        })
        .expect("launch environment");
        let content = entries
            .iter()
            .filter(|(key, _)| key == CONFIG_CONTENT_KEY)
            .map(|(_, value)| value.to_str().expect("UTF-8"))
            .collect::<Vec<_>>();
        assert_eq!(content.len(), 1);
        let policy: Value = serde_json::from_str(content[0]).expect("policy JSON");
        assert_eq!(policy.as_object().expect("config object").len(), 1);
        assert!(policy.get("mcp").is_none());
        assert!(policy.get("agent").is_none());
        assert_eq!(policy["permission"]["*"], "ask");
        assert_eq!(policy["permission"]["read"], "allow");
    }

    #[test]
    fn read_permission_is_serialized_after_wildcard_ask() {
        let (_, content) = launch_config().expect("policy config");
        let content = content.to_str().expect("UTF-8");
        let deny = content.find("\"*\":\"ask\"").expect("wildcard ask");
        let read = content.find("\"read\":\"allow\"").expect("read allow");
        assert!(deny < read, "OpenCode applies the last matching rule");
    }
}
