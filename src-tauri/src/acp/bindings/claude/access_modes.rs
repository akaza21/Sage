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
