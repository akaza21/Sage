import type { AcpChatPage } from "../../../src/domain/acp";

export type VisualScenarioName =
  | "agent-single"
  | "agent-dual"
  | "agent-long"
  | "agent-activity-family"
  | "agent-image"
  | "agent-child-activity"
  | "editor"
  | "file"
  | "diff"
  | "preview"
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
  "agent-image",
  "agent-child-activity",
];

export const EDITOR_VISUAL_SCENARIOS: readonly VisualScenarioName[] = [
  "editor",
];

export const RESOURCE_VISUAL_SCENARIOS: readonly VisualScenarioName[] = [
  "file",
  "diff",
  "preview",
];

export const RECOVERY_VISUAL_SCENARIOS: readonly VisualScenarioName[] = [
  "error",
  "waiting",
];
