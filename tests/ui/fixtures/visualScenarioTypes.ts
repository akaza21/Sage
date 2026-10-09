import type { AcpChatPage } from "../../../src/domain/acp";

export type VisualScenarioName =
  | "agent-single"
  | "agent-dual"
  | "agent-long"
  | "agent-activity-family"
  | "agent-thinking-live"
  | "rich"
  | "agent-declared-test"
  | "agent-command-rollup"
  | "agent-image"
  | "agent-child-activity"
  | "agent-picker-harness-model"
  | "agent-picker-handoff"
  | "agent-composer-narrow"
  | "agent-composer-context-unknown"
  | "agent-composer-context-known"
  | "agent-composer-goal"
  | "agent-composer-context-over-capacity"
  | "agent-session-mode-closed"
  | "agent-session-mode-open"
  | "agent-session-mode-narrow"
  | "agent-session-mode-forced-colors"
  | "editor"
  | "editor-picker-repository"
  | "editor-picker-repository-filtered"
  | "editor-picker-narrow"
  | "editor-picker-zoom-200"
  | "file"
  | "diff"
  | "preview"
  | "agent-preview-starting"
  | "agent-preview-health-failed"
  | "agent-preview-exited"
  | "error"
  | "waiting";

export type VisualScenarioMode = "agent" | "editor";
type VisualScenarioResource = "empty" | "file" | "diff" | "preview";
export type VisualAgentResourceState =
  "diff-error" | "diff-loading" | "file-error" | "file-loading";

export type VisualSurface = "agent" | "editor" | "resource" | "recovery";

export interface VisualScenarioDefinition {
  name: VisualScenarioName;
  mode: VisualScenarioMode;
  resource: VisualScenarioResource;
}

export interface BuiltVisualScenario extends VisualScenarioDefinition {
  pages: AcpChatPage[];
}

// Slice truth lives here (not in `surfaces/*`) so Playwright spec files can
// import the lists statically: surface fixture modules pull in the workbench
// store chain, which has no `window` in the Playwright runner process.
// `visualScenarioOwnership.test.ts` proves each surface page builder claims
// exactly its slice.
export const AGENT_VISUAL_SCENARIOS: readonly VisualScenarioName[] = [
  "agent-single",
  "agent-dual",
  "agent-long",
  "agent-activity-family",
  "agent-thinking-live",
  "rich",
  "agent-declared-test",
  "agent-command-rollup",
  "agent-image",
  "agent-child-activity",
  "agent-picker-harness-model",
  "agent-picker-handoff",
  "agent-composer-narrow",
  "agent-composer-context-unknown",
  "agent-composer-context-known",
  "agent-composer-goal",
  "agent-composer-context-over-capacity",
  "agent-session-mode-closed",
  "agent-session-mode-open",
  "agent-session-mode-narrow",
  "agent-session-mode-forced-colors",
];

export const EDITOR_VISUAL_SCENARIOS: readonly VisualScenarioName[] = [
  "editor",
  "editor-picker-repository",
  "editor-picker-repository-filtered",
  "editor-picker-narrow",
  "editor-picker-zoom-200",
];

export const RESOURCE_VISUAL_SCENARIOS: readonly VisualScenarioName[] = [
  "file",
  "diff",
  "preview",
];

// These are deterministic browser projection-fixture companions only. They
// deliberately stay out of canonical screenshot coverage: native lifecycle
// truth requires the packaged Tauri journey rather than a seeded projection.
export const PREVIEW_PROJECTION_FIXTURE_SCENARIOS: readonly VisualScenarioName[] =
  [
    "agent-preview-starting",
    "agent-preview-health-failed",
    "agent-preview-exited",
  ];

export const RECOVERY_VISUAL_SCENARIOS: readonly VisualScenarioName[] = [
  "error",
  "waiting",
];
