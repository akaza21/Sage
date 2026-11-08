//! Claude Code's access modes. Everything Claude-specific about access lives
//! here: which Sage access modes Claude offers, and which of its permission
//! modes carries each. Claude reports its permission modes as session modes
//! (the ACP adapter's `session/new` modes and its `mode` config option), and
//! a mode is set through the binding's ordinary session-config path, so the
//! rest of Sage only sees the harness-neutral `NativeAccessMode` rows. A
//! binding over the Claude Agent SDK maps the same `permissionMode` values and
//! can reuse this table.

use crate::acp::bindings::access_policy::NativeAccessMode;
use crate::settings_store::SessionAccessMode;

pub(crate) const CLAUDE_ACCESS_MODES: &[NativeAccessMode] = &[
    NativeAccessMode {
        id: SessionAccessMode::Ask,
        label: "Ask each time",
        description: "Review each permission request; no filesystem sandbox.",
        session_mode: "default",
    },
    NativeAccessMode {
        id: SessionAccessMode::AcceptEdits,
        label: "Accept edits",
        description: "File edits run without asking; other tools still ask. No filesystem sandbox.",
        session_mode: "acceptEdits",
    },
    NativeAccessMode {
        id: SessionAccessMode::Plan,
        label: "Plan",
        description: "Claude plans without changing files and asks before it starts.",
        session_mode: "plan",
    },
    NativeAccessMode {
        id: SessionAccessMode::Auto,
        label: "Auto",
        description: "Claude runs the actions it judges safe and asks about the rest. No filesystem sandbox.",
        session_mode: "auto",
    },
    NativeAccessMode {
        id: SessionAccessMode::FullAccess,
        label: "Full access",
        description: "Every edit, tool and command runs without asking, with no filesystem sandbox. This chat only.",
        session_mode: "bypassPermissions",
    },
];

/// Claude's permission modes from least to most permissive.
const PERMISSIVENESS: [&str; 5] = [
    "plan",
    "default",
    "acceptEdits",
    "auto",
    "bypassPermissions",
];

fn rank(mode: &str) -> Option<usize> {
    PERMISSIVENESS.iter().position(|known| *known == mode)
}

/// Whether Claude may move a turn that Sage put in `selected` into `next`
/// on its own: only to a mode no more permissive. Entering plan mode is the
/// common case. An unknown mode is never admitted.
pub(crate) fn mode_change_admitted(selected: &str, next: &str) -> bool {
    matches!((rank(selected), rank(next)), (Some(selected), Some(next)) if next <= selected)
}

/// The permission mode a permission option switches the session into, for
/// the adapter's plan-exit options (0.76: `exit-plan-*`).
pub(crate) fn option_target_mode(option_id: &str) -> Option<&'static str> {
    match option_id {
        "exit-plan-default" => Some("default"),
        "exit-plan-accept-edits" | "exit-plan-clear-accept-edits" => Some("acceptEdits"),
        "exit-plan-auto" | "exit-plan-clear-auto" => Some("auto"),
        "exit-plan-bypass" | "exit-plan-clear-bypass" => Some("bypassPermissions"),
        _ => None,
    }
}

/// Whether Sage may answer with this option. An option that changes the
/// permission mode is used only when it leads to a mode no more permissive
/// than accepting edits, and never when it also clears the conversation.
pub(crate) fn option_admitted(option_id: &str) -> bool {
    match option_target_mode(option_id) {
        None => true,
        Some(target) => {
            !option_id.starts_with("exit-plan-clear-")
                && mode_change_admitted("acceptEdits", target)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_narrower_mode_changes_and_plain_plan_exits_are_admitted() {
        assert!(mode_change_admitted("default", "plan"));
        assert!(mode_change_admitted("bypassPermissions", "acceptEdits"));
        assert!(mode_change_admitted("plan", "plan"));
        assert!(!mode_change_admitted("plan", "bypassPermissions"));
        assert!(!mode_change_admitted("acceptEdits", "auto"));
        assert!(!mode_change_admitted("default", "futureMode"));
        assert!(option_admitted("exit-plan-default"));
        assert!(option_admitted("exit-plan-accept-edits"));
        assert!(option_admitted("allow"));
        for refused in [
            "exit-plan-auto",
            "exit-plan-bypass",
            "exit-plan-clear-auto",
            "exit-plan-clear-bypass",
            "exit-plan-clear-accept-edits",
        ] {
            assert!(!option_admitted(refused), "{refused}");
        }
    }
}
