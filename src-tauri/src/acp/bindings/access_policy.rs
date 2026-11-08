use super::pi::bridge::install::Unavailable;
use crate::acp::bindings::codex::CODEX_PINNED_VERSION;
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
    /// The harness session mode this access mode selects. The composer offers
    /// the mode only while the live session advertises this id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) session_mode_id: Option<String>,
}

/// An access mode a harness enforces itself, through one of its own session
/// (permission) modes. A binding whose access works this way lists its rows
/// in its own module; everything else here, and the settings, grants,
/// diagnostics and composer, stay harness-neutral. Before each prompt Sage
/// selects `session_mode` through the binding's session-config path, and a
/// mode the live session does not advertise is refused, never approximated.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NativeAccessMode {
    pub(crate) id: SessionAccessMode,
    pub(crate) label: &'static str,
    pub(crate) description: &'static str,
    pub(crate) session_mode: &'static str,
}

/// The curated binding's native access modes, first being its default.
fn native_access_modes(binding: &str) -> &'static [NativeAccessMode] {
    match binding {
        "claude-agent-acp" => super::claude::CLAUDE_ACCESS_MODES,
        _ => &[],
    }
}

/// The harness session mode that carries `mode` for `binding`, when the
/// binding enforces access through session modes.
pub(crate) fn session_mode(binding: &str, mode: SessionAccessMode) -> Option<&'static str> {
    native_access_modes(binding)
        .iter()
        .find(|native| native.id == mode)
        .map(|native| native.session_mode)
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
    let native = native_access_modes(binding);
    if !native.is_empty() {
        return native
            .iter()
            .enumerate()
            .map(|(index, mode)| AccessModeCapability {
                id: mode.id,
                label: mode.label.into(),
                description: mode.description.into(),
                fallback: index == 0 && mode.id.is_default_eligible(),
                session_mode_id: Some(mode.session_mode.into()),
            })
            .collect();
    }
    let rows: &[(SessionAccessMode, &str, &str)] = match binding {
        "codex-app-server" if version == CODEX_PINNED_VERSION && characterised => &[
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
        "claude-code-sdk" => &[(
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
        // A user-defined agent: Sage answers its permission requests and
        // stops a Turn whose agent runs a non-read tool without asking.
        "custom-acp" => &[
            (
                Ask,
                "Ask each time",
                "Review each permission request; a tool run without asking stops the Turn.",
            ),
            (
                ReadOnly,
                "Read-only",
                "Sage declines every permission request and file write.",
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
            session_mode_id: None,
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

/// The mode a prompt runs under: the chat's own choice while the binding
/// still offers exactly that mode, else the binding's stored default. A
/// choice the binding cannot carry never lowers or raises the default.
pub(crate) fn turn_mode(
    binding: &str,
    version: &str,
    characterised: bool,
    chat: Option<SessionAccessMode>,
    default: Option<SessionAccessMode>,
) -> Option<SessionAccessMode> {
    if let Some(chat) = chat
        && effective_mode(binding, version, Some(chat), characterised) == Some(chat)
    {
        return Some(chat);
    }
    effective_mode(binding, version, default, characterised)
}

pub(crate) fn turn_mode_for_binding(
    registry: &super::registry::BindingRegistry,
    binding: &str,
    chat: Option<SessionAccessMode>,
    default: Option<SessionAccessMode>,
) -> Option<SessionAccessMode> {
    let record = registry.route(binding).ok()?;
    let bound = record.bound_build()?;
    turn_mode(
        record.access_policy_binding_id(),
        &bound.reported_version,
        bound.characterisation.is_characterised(),
        chat,
        default,
    )
}

/// Whether the routed binding offers `mode` to a chat.
pub(crate) fn offers_mode(
    registry: &super::registry::BindingRegistry,
    binding: &str,
    mode: SessionAccessMode,
) -> bool {
    turn_mode_for_binding(registry, binding, Some(mode), None) == Some(mode)
}

/// Whether `mode` is in the routed binding's own table of access modes,
/// whatever build is bound or whether one is: a chat may always step down to
/// such a mode, even while its harness is unavailable, and to no other.
pub(crate) fn lists_mode(
    registry: &super::registry::BindingRegistry,
    binding: &str,
    mode: SessionAccessMode,
) -> bool {
    use SessionAccessMode::{Ask, ReadOnly, WorkspaceWrite};
    let Ok(record) = registry.route(binding) else {
        return false;
    };
    let policy = record.access_policy_binding_id();
    let native = native_access_modes(policy);
    if !native.is_empty() {
        return native.iter().any(|native| native.id == mode);
    }
    match policy {
        "codex-app-server" => {
            matches!(mode, ReadOnly | WorkspaceWrite)
                || (mode == Ask && CODEX_ASK_NATIVE_APPROVAL_PROVEN)
        }
        "opencode-acp" => matches!(mode, ReadOnly | Ask | WorkspaceWrite),
        "pi-rpc" => matches!(mode, ReadOnly | WorkspaceWrite),
        "claude-code-sdk" => mode == Ask,
        _ => false,
    }
}

/// Whether the routed binding's access is enforced through its harness
/// session modes. Only the access path may then change the session mode.
/// None when the binding cannot be routed, so the caller decides how to
/// fail closed.
pub(crate) fn governs_session_modes(
    registry: &super::registry::BindingRegistry,
    binding: &str,
) -> Option<bool> {
    registry
        .route(binding)
        .ok()
        .map(|record| !native_access_modes(record.access_policy_binding_id()).is_empty())
}

/// The harness session mode that carries `mode` for a routed binding.
pub(crate) fn session_mode_for_binding(
    registry: &super::registry::BindingRegistry,
    binding: &str,
    mode: SessionAccessMode,
) -> Option<&'static str> {
    session_mode(
        registry.route(binding).ok()?.access_policy_binding_id(),
        mode,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn uncharacterised_codex_cannot_offer_workspace_write() {
        let modes = access_modes("codex-app-server", CODEX_PINNED_VERSION, false);
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
                effective_mode(binding, CODEX_PINNED_VERSION, None, true),
                expected,
                "{binding}"
            );
        }
    }

    #[test]
    fn unsupported_saved_access_never_escalates() {
        use SessionAccessMode::{Ask, ReadOnly, WorkspaceWrite};
        assert_eq!(
            effective_mode("codex-app-server", CODEX_PINNED_VERSION, Some(Ask), true),
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
                CODEX_PINNED_VERSION,
                vec!["readOnly", "workspaceWrite"],
            ),
            ("codex-app-server", "0.145.0", vec!["readOnly"]),
            (
                "claude-agent-acp",
                "1",
                vec!["ask", "acceptEdits", "plan", "auto", "fullAccess"],
            ),
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
                assert_ne!(mode["label"], "Approve for me");
                assert_eq!(mode["fallback"], mode["id"] == modes[0]);
                // Full access is offered only where a harness session mode
                // carries it natively, and it is never the fallback.
                if mode["id"] == "fullAccess" {
                    assert_eq!(mode["sessionModeId"], "bypassPermissions");
                    assert_eq!(mode["fallback"], false);
                }
            }
        }
        let codex =
            serde_json::to_value(access_modes("codex-app-server", CODEX_PINNED_VERSION, true))
                .unwrap();
        assert_eq!(
            codex[1]["description"],
            "Edits and commands run without asking inside the workspace and temp folders; no network access."
        );
    }

    #[test]
    fn claude_access_modes_select_its_advertised_permission_modes() {
        use SessionAccessMode::{
            AcceptEdits, Ask, Auto, FullAccess, Plan, ReadOnly, WorkspaceWrite,
        };
        let wire = serde_json::to_value(access_modes("claude-agent-acp", "0.76.0", true)).unwrap();
        assert_eq!(
            wire.as_array()
                .unwrap()
                .iter()
                .map(|mode| (mode["id"].clone(), mode["sessionModeId"].clone()))
                .collect::<Vec<_>>(),
            vec![
                (json!("ask"), json!("default")),
                (json!("acceptEdits"), json!("acceptEdits")),
                (json!("plan"), json!("plan")),
                (json!("auto"), json!("auto")),
                (json!("fullAccess"), json!("bypassPermissions")),
            ]
        );
        for (mode, native) in [
            (Ask, Some("default")),
            (AcceptEdits, Some("acceptEdits")),
            (Plan, Some("plan")),
            (Auto, Some("auto")),
            (FullAccess, Some("bypassPermissions")),
            (ReadOnly, None),
            (WorkspaceWrite, None),
        ] {
            assert_eq!(session_mode("claude-agent-acp", mode), native, "{mode:?}");
        }
        // Other harnesses keep their own enforcement; none maps to a
        // session mode, and none offers the new modes.
        for binding in ["codex-app-server", "opencode-acp", "pi-rpc"] {
            assert_eq!(session_mode(binding, FullAccess), None);
            assert!(
                access_modes(binding, CODEX_PINNED_VERSION, true)
                    .iter()
                    .all(
                        |mode| !matches!(mode.id, AcceptEdits | Plan | Auto | FullAccess)
                            && mode.session_mode_id.is_none()
                    )
            );
            // A grant those harnesses cannot carry falls back to read-only.
            assert_eq!(
                effective_mode(binding, CODEX_PINNED_VERSION, Some(FullAccess), true),
                Some(ReadOnly)
            );
        }
        assert_eq!(
            effective_mode("claude-agent-acp", "1", Some(FullAccess), true),
            Some(FullAccess)
        );
        assert_eq!(
            effective_mode("claude-agent-acp", "1", None, true),
            Some(Ask)
        );
    }

    #[test]
    fn a_chat_choice_applies_only_where_the_binding_carries_it() {
        use SessionAccessMode::{
            AcceptEdits, Ask, Auto, FullAccess, Plan, ReadOnly, WorkspaceWrite,
        };
        let claude = |chat, default| turn_mode("claude-agent-acp", "0.76.0", true, chat, default);
        assert_eq!(claude(Some(FullAccess), None), Some(FullAccess));
        assert_eq!(claude(Some(FullAccess), Some(Plan)), Some(FullAccess));
        assert_eq!(claude(Some(Auto), Some(AcceptEdits)), Some(Auto));
        assert_eq!(claude(Some(Plan), Some(AcceptEdits)), Some(Plan));
        // Without a choice a chat runs under the binding default, which
        // starts at Ask each time and can be any non-Full mode.
        assert_eq!(claude(None, None), Some(Ask));
        assert_eq!(claude(None, Some(AcceptEdits)), Some(AcceptEdits));
        assert_eq!(claude(None, Some(ReadOnly)), None);
        // A choice Claude cannot carry leaves the default in charge.
        assert_eq!(claude(Some(WorkspaceWrite), Some(Plan)), Some(Plan));
        // A choice a harness cannot carry keeps its own default rather than
        // falling to some other mode.
        assert_eq!(
            turn_mode(
                "codex-app-server",
                CODEX_PINNED_VERSION,
                true,
                Some(FullAccess),
                Some(WorkspaceWrite)
            ),
            Some(WorkspaceWrite)
        );
        assert_eq!(
            turn_mode("opencode-acp", "1", true, Some(FullAccess), None),
            Some(ReadOnly)
        );
        assert_eq!(
            turn_mode("opencode-acp", "1", true, Some(Ask), Some(ReadOnly)),
            Some(Ask)
        );
    }

    #[test]
    fn pi_offers_workspace_edits_only_with_its_bridge_and_says_why_not() {
        use SessionAccessMode::{ReadOnly, WorkspaceWrite};
        let modes = |bridge| {
            admitted_modes("pi-rpc", crate::acp::bindings::pi::PI_VERSION, true, bridge)
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
            admitted_modes(
                "pi-rpc",
                crate::acp::bindings::pi::PI_VERSION,
                false,
                Ok(())
            )
            .into_iter()
            .map(|mode| mode.id)
            .collect::<Vec<_>>(),
            vec![ReadOnly]
        );
    }
}
