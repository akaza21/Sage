//! Claude Code's access modes. Everything Claude-specific about access lives
//! here: which Sage access modes Claude offers, and which of its permission
//! modes carries each. Claude reports its permission modes as session modes
//! (the ACP adapter's `session/new` modes and its `mode` config option), and
//! a mode is set through the binding's ordinary session-config path, so the
//! rest of Sage only sees the harness-neutral `NativeAccessMode` rows. The
//! binding over Claude Code's SDK protocol reports the same permission modes
//! through the same path, without the one it cannot select.

use crate::acp::bindings::access_policy::NativeAccessMode;
use crate::acp_protocol::access_mode::SessionAccessMode;

const ASK: NativeAccessMode = NativeAccessMode {
    id: SessionAccessMode::Ask,
    label: "Ask each time",
    description: "Review each permission request; no filesystem sandbox.",
    session_mode: "default",
};
const ACCEPT_EDITS: NativeAccessMode = NativeAccessMode {
    id: SessionAccessMode::AcceptEdits,
    label: "Accept edits",
    description: "File edits run without asking; other tools still ask. No filesystem sandbox.",
    session_mode: "acceptEdits",
};
const PLAN: NativeAccessMode = NativeAccessMode {
    id: SessionAccessMode::Plan,
    label: "Plan",
    description: "Claude plans without changing files and asks before it starts.",
    session_mode: "plan",
};
const AUTO: NativeAccessMode = NativeAccessMode {
    id: SessionAccessMode::Auto,
    label: "Auto",
    description: "Claude runs the actions it judges safe and asks about the rest. No filesystem sandbox.",
    session_mode: "auto",
};
const FULL_ACCESS: NativeAccessMode = NativeAccessMode {
    id: SessionAccessMode::FullAccess,
    label: "Full access",
    description: "Every edit, tool and command runs without asking, with no filesystem sandbox. This chat only.",
    session_mode: "bypassPermissions",
};

pub(crate) const CLAUDE_ACCESS_MODES: &[NativeAccessMode] =
    &[ASK, ACCEPT_EDITS, PLAN, AUTO, FULL_ACCESS];

/// Claude Code over its SDK protocol. Its `bypassPermissions` mode needs a
/// launch flag Sage does not pass, so Full access is not offered there.
pub(crate) const CLAUDE_SDK_ACCESS_MODES: &[NativeAccessMode] = &[ASK, ACCEPT_EDITS, PLAN, AUTO];

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
        // The SDK bridge's allow-always on a file edit.
        "allow-always-accept-edits" => Some("acceptEdits"),
        "exit-plan-auto" | "exit-plan-clear-auto" => Some("auto"),
        "exit-plan-bypass" | "exit-plan-clear-bypass" => Some("bypassPermissions"),
        _ => None,
    }
}

/// The plan-exit options Sage may answer with: each one's kind, and the
/// permission mode it leaves the session in, no more permissive than
/// accepting edits and without clearing the conversation.
const PLAN_EXIT_OPTIONS: [(&str, &str, &str); 3] = [
    ("exit-plan-default", "allow_once", "default"),
    ("exit-plan-accept-edits", "allow_always", "acceptEdits"),
    ("allow-always-accept-edits", "allow_always", "acceptEdits"),
];

/// Whether Sage may answer with this option. A request that switches the
/// permission mode (Claude's plan exit, `switch_mode`), or an option that
/// names a plan exit, is answered only with a known plan-exit option of its
/// known kind; any other, whatever its id, including one a newer adapter
/// adds, is refused.
pub(crate) fn option_admitted(option_id: &str, kind: &str, switches_mode: bool) -> bool {
    let plan_exit = switches_mode
        || option_id.starts_with("exit-plan-")
        || option_target_mode(option_id).is_some();
    !plan_exit
        || PLAN_EXIT_OPTIONS.iter().any(|(id, known_kind, target)| {
            *id == option_id
                && *known_kind == kind
                && option_target_mode(option_id) == Some(*target)
        })
}

/// The wider of two permission modes; an unknown one is never the wider.
pub(crate) fn wider_mode<'a>(selected: &'a str, sanctioned: Option<&'a str>) -> &'a str {
    match sanctioned {
        Some(sanctioned)
            if !mode_change_admitted(selected, sanctioned) && rank(sanctioned).is_some() =>
        {
            sanctioned
        }
        _ => selected,
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
        let plan_exit = |id: &str, kind: &str| option_admitted(id, kind, true);
        assert!(plan_exit("exit-plan-default", "allow_once"));
        assert!(plan_exit("exit-plan-accept-edits", "allow_always"));
        // Ordinary permission requests keep their own allow options.
        assert!(option_admitted("allow", "allow_once", false));
        assert!(option_admitted("allow-with-updates", "allow_always", false));
        for (refused, kind) in [
            ("exit-plan-auto", "allow_always"),
            ("exit-plan-bypass", "allow_always"),
            ("exit-plan-clear-auto", "allow_always"),
            ("exit-plan-clear-bypass", "allow_always"),
            ("exit-plan-clear-accept-edits", "allow_always"),
            ("exit-plan-future-mode", "allow_once"),
            ("exit-plan-", "allow_once"),
            // A known id with another kind, or none.
            ("exit-plan-default", "allow_always"),
            ("exit-plan-accept-edits", ""),
            // A mode switch offered under any other id.
            ("allow", "allow_once"),
            ("allow-with-updates", "allow_always"),
            ("plan-exit-bypass", "allow_always"),
        ] {
            assert!(!plan_exit(refused, kind), "{refused} {kind}");
        }
        // A plan-exit id is refused even on a request not marked as a switch.
        assert!(!option_admitted("exit-plan-bypass", "allow_always", false));
        assert!(option_admitted("exit-plan-default", "allow_once", false));
    }

    #[test]
    fn a_turn_may_narrow_from_the_wider_of_its_selected_and_sanctioned_modes() {
        assert_eq!(wider_mode("plan", Some("acceptEdits")), "acceptEdits");
        assert_eq!(wider_mode("acceptEdits", Some("default")), "acceptEdits");
        assert_eq!(wider_mode("default", None), "default");
        assert_eq!(wider_mode("default", Some("futureMode")), "default");
    }
}
