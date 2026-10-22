import type { SessionAccessModeCapability } from "../../../src/domain/sessionAccessMode";

const readOnly: SessionAccessModeCapability = {
  id: "readOnly",
  label: "Read-only",
  description: "Read workspace files; no writes or network.",
  fallback: true,
};
const ask: SessionAccessModeCapability = {
  id: "ask",
  label: "Ask each time",
  description: "Review each permission request; no filesystem sandbox.",
  fallback: true,
};
const workspaceWrite: SessionAccessModeCapability = {
  id: "workspaceWrite",
  label: "Workspace edits",
  description:
    "Edits and commands run without asking inside the workspace and temp folders; no network access.",
};

export const popoverProfiles = [
  {
    name: "Codex",
    subagents: "children" as const,
    access: [readOnly, workspaceWrite],
    modes: [],
    effort: {
      id: "effort",
      levels: ["minimal", "low", "medium", "high", "xhigh"],
      currentValue: "high",
      defaultValue: "medium",
    },
  },
  {
    name: "Claude Code",
    subagents: "tasks" as const,
    access: [ask],
    modes: [
      { id: "default", name: "Default" },
      { id: "plan", name: "Plan" },
    ],
    effort: {
      id: "effort",
      levels: ["low", "medium", "high"],
      currentValue: "high",
    },
  },
  {
    name: "Pi",
    access: [
      {
        ...readOnly,
        description: "Read-only tools only; workspace editing is not admitted.",
      },
    ],
    modes: [],
    effort: {
      id: "pi.thinking",
      levels: ["off", "medium", "high"],
      currentValue: "medium",
    },
  },
  {
    name: "OpenCode",
    access: [readOnly, ask, workspaceWrite],
    modes: [
      { id: "build", name: "Build" },
      { id: "plan", name: "Plan" },
    ],
    effort: null,
  },
  { name: "Oh My Pi", access: [], modes: [], effort: null },
];
