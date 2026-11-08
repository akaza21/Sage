use super::pi::bridge::install::Unavailable;
use crate::settings_store::SessionAccessMode;

pub(crate) const CODEX_ASK_NATIVE_APPROVAL_PROVEN: bool = false;

const PI_READ_ONLY: &[(SessionAccessMode, &str, &str)] = &[(
    SessionAccessMode::ReadOnly,
    "Read-only",
    "Read-only tools only; workspace editing is not admitted.",
)];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessModeCapability {
    pub(crate) id: SessionAccessMode,
    pub(crate) label: String,
    pub(crate) description: String,
    pub(crate) fallback: bool,
}

pub(crate) fn access_modes(
    binding: &str,
    version: &str,
    characterised: bool,
) -> Vec<AccessModeCapability> {
    let pi_bridge = match binding {
        "pi-rpc" => super::pi::bridge::install::installed(version).map(drop),
        _ => Err(Unavailable::UnsupportedVersion),
    };
    admitted_modes(binding, version, characterised, pi_bridge)
}

/// The modes Sage enforces for a binding, given whether the Pi bridge that
/// carries Pi's workspace edits is available.
pub(crate) fn admitted_modes(
    binding: &str,
    version: &str,
    characterised: bool,
    pi_bridge: Result<(), Unavailable>,
) -> Vec<AccessModeCapability> {
    use SessionAccessMode::{Ask, ReadOnly, WorkspaceWrite};
    let rows: &[(SessionAccessMode, &str, &str)] = match binding {
        "codex-app-server" if version == "0.146.0" && characterised => &[
            (
                ReadOnly,
                "Read-only",
                "Read workspace files; no writes or network.",
            ),
            (
                WorkspaceWrite,
                "Workspace edits",
                "Edits and commands run without asking inside the workspace and temp folders; no network access.",
            ),
            (
                Ask,
                "Ask each time",
                "Review native approval requests; read-only sandbox with no network.",
            ),
        ],
        "codex-app-server" => &[(
            ReadOnly,
            "Read-only",
            "Read workspace files; no writes or network.",
        )],
        "claude-agent-acp" => &[(
            Ask,
            "Ask each time",
            "Review each permission request; no filesystem sandbox.",
        )],
        "pi-rpc" if characterised => match pi_bridge {
            Ok(_) => &[
                (
                    ReadOnly,
                    "Read-only",
                    "Sage declines every Pi write and shell command.",
                ),
                (
                    WorkspaceWrite,
                    "Workspace edits",
                    "Ask before each verified workspace edit; shell commands are denied.",
                ),
            ],
            // Say why edits are off instead of quietly offering less.
            Err(Unavailable::Missing | Unavailable::Modified) => &[(
                ReadOnly,
                "Read-only",
                "Workspace edits are unavailable: this Sage build's Pi bridge is missing or altered.",
            )],
            Err(Unavailable::UnsupportedVersion) => PI_READ_ONLY,
        },
        "pi-rpc" => PI_READ_ONLY,
        "opencode-acp" => &[
            (
                ReadOnly,
                "Read-only",
                "Sage declines every permission request and file write.",
            ),
            (
                Ask,
                "Ask each time",
                "Review non-read tool permission requests before execution.",
            ),
            (
                WorkspaceWrite,
                "Workspace edits",
                "Auto-approve verified workspace edits; ask for other tools.",
            ),
        ],
        _ => &[],
    };
    rows.iter()
        .filter(|(id, _, _)| {
            binding != "codex-app-server" || *id != Ask || CODEX_ASK_NATIVE_APPROVAL_PROVEN
        })
        .enumerate()
        .map(|(index, (id, label, description))| AccessModeCapability {
            id: *id,
            label: (*label).into(),
            description: (*description).into(),
            fallback: index == 0,
        })
        .collect()
}

pub(crate) fn effective_mode(
    binding: &str,
    version: &str,
    requested: Option<SessionAccessMode>,
    characterised: bool,
) -> Option<SessionAccessMode> {
    let admitted = access_modes(binding, version, characterised);
    admitted
        .iter()
        .find(|mode| Some(mode.id) == requested)
        .or_else(|| {
            admitted.iter().find(|mode| {
                mode.fallback && (requested.is_none() || mode.id == SessionAccessMode::ReadOnly)
            })
        })
        .map(|mode| mode.id)
}

pub(crate) fn effective_for_binding(
    registry: &super::registry::BindingRegistry,
    binding: &str,
    requested: Option<SessionAccessMode>,
) -> Option<SessionAccessMode> {
    let record = registry.route(binding).ok()?;
    let bound = record.bound_build()?;
    effective_mode(
        record.access_policy_binding_id(),
        &bound.reported_version,
        requested,
        bound.characterisation.is_characterised(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn uncharacterised_codex_cannot_offer_workspace_write() {
        let modes = access_modes("codex-app-server", "0.146.0", false);
        assert_eq!(modes.len(), 1);
        assert_eq!(modes[0].id, SessionAccessMode::ReadOnly);
    }

    #[test]
    fn each_binding_defaults_to_its_safest_admitted_policy() {
        use SessionAccessMode::{Ask, ReadOnly};
        for (binding, expected) in [
            ("claude-agent-acp", Some(Ask)),
            ("codex-app-server", Some(ReadOnly)),
            ("opencode-acp", Some(ReadOnly)),
            ("pi-rpc", Some(ReadOnly)),
            ("omp-native-acp", None),
        ] {
            assert_eq!(
                effective_mode(binding, "0.146.0", None, true),
                expected,
                "{binding}"
            );
        }
    }

    #[test]
    fn unsupported_saved_access_never_escalates() {
        use SessionAccessMode::{Ask, ReadOnly, WorkspaceWrite};
        assert_eq!(
            effective_mode("codex-app-server", "0.146.0", Some(Ask), true),
            Some(ReadOnly)
        );
        assert_eq!(
            effective_mode("claude-agent-acp", "1", Some(ReadOnly), true),
            None
        );
        assert_eq!(
            effective_mode("claude-agent-acp", "1", Some(Ask), true),
            Some(Ask)
        );
        assert_eq!(
            effective_mode("opencode-acp", "1", Some(WorkspaceWrite), true),
            Some(WorkspaceWrite)
        );
        assert_eq!(
            effective_mode("omp-native-acp", "1", Some(ReadOnly), true),
            None
        );
    }

    #[test]
    fn access_policy_wire_only_admits_enforced_modes() {
        for (binding, version, modes) in [
            (
                "codex-app-server",
                "0.146.0",
                vec!["readOnly", "workspaceWrite"],
            ),
            ("codex-app-server", "0.145.0", vec!["readOnly"]),
            ("claude-agent-acp", "1", vec!["ask"]),
            (
                "opencode-acp",
                "1",
                vec!["readOnly", "ask", "workspaceWrite"],
            ),
            ("omp-native-acp", "1", vec![]),
            ("unknown", "1", vec![]),
        ] {
            let wire = serde_json::to_value(access_modes(binding, version, true)).unwrap();
            assert_eq!(
                wire.as_array()
                    .unwrap()
                    .iter()
                    .map(|mode| mode["id"].clone())
                    .collect::<Vec<_>>(),
                modes.iter().map(|mode| json!(mode)).collect::<Vec<_>>(),
                "{binding}"
            );
            for mode in wire.as_array().unwrap() {
                assert!(!mode["label"].as_str().unwrap().is_empty());
                assert!(!mode["description"].as_str().unwrap().is_empty());
                assert_ne!(mode["label"], "Full access");
                assert_ne!(mode["label"], "Approve for me");
                assert_eq!(mode["fallback"], mode["id"] == modes[0]);
            }
        }
        let codex =
            serde_json::to_value(access_modes("codex-app-server", "0.146.0", true)).unwrap();
        assert_eq!(
            codex[1]["description"],
            "Edits and commands run without asking inside the workspace and temp folders; no network access."
        );
    }

    #[test]
    fn pi_offers_workspace_edits_only_with_its_bridge_and_says_why_not() {
        use SessionAccessMode::{ReadOnly, WorkspaceWrite};
        let modes = |bridge| {
            admitted_modes("pi-rpc", "0.83.0", true, bridge)
                .into_iter()
                .map(|mode| (mode.id, mode.description))
                .collect::<Vec<_>>()
        };
        let available = modes(Ok(()));
        assert_eq!(
            available.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![ReadOnly, WorkspaceWrite]
        );
        for missing in [Unavailable::Missing, Unavailable::Modified] {
            assert_eq!(
                modes(Err(missing)),
                vec![(
                    ReadOnly,
                    "Workspace edits are unavailable: this Sage build's Pi bridge is missing or altered."
                        .to_owned()
                )]
            );
        }
        assert_eq!(
            modes(Err(Unavailable::UnsupportedVersion)),
            vec![(
                ReadOnly,
                "Read-only tools only; workspace editing is not admitted.".to_owned()
            )]
        );
        assert_eq!(
            admitted_modes("pi-rpc", "0.83.0", false, Ok(()))
                .into_iter()
                .map(|mode| mode.id)
                .collect::<Vec<_>>(),
            vec![ReadOnly]
        );
    }
}
