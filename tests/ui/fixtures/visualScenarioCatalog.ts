import {
  AGENT_VISUAL_SCENARIOS,
  EDITOR_VISUAL_SCENARIOS,
  PREVIEW_PROJECTION_FIXTURE_SCENARIOS,
  RECOVERY_VISUAL_SCENARIOS,
  RESOURCE_VISUAL_SCENARIOS,
} from "./visualScenarioTypes";
import type {
  VisualScenarioDefinition,
  VisualScenarioName,
  VisualSurface,
} from "./visualScenarioTypes";

const SCENARIO_META: Record<
  VisualScenarioName,
  Omit<VisualScenarioDefinition, "name">
> = {
  "agent-single": { mode: "agent", resource: "empty" },
  "agent-dual": { mode: "agent", resource: "empty" },
  "agent-long": { mode: "agent", resource: "empty" },
  "agent-activity-family": { mode: "agent", resource: "empty" },
  "agent-image": { mode: "agent", resource: "empty" },
  "agent-child-activity": { mode: "agent", resource: "empty" },
  "agent-picker-harness-model": { mode: "agent", resource: "empty" },
  "agent-picker-handoff": { mode: "agent", resource: "empty" },
  "agent-composer-narrow": { mode: "agent", resource: "empty" },
  "agent-composer-context-unknown": { mode: "agent", resource: "empty" },
  "agent-composer-context-known": { mode: "agent", resource: "empty" },
  "agent-composer-context-over-capacity": {
    mode: "agent",
    resource: "empty",
  },
  "agent-session-mode-closed": { mode: "agent", resource: "empty" },
  "agent-session-mode-open": { mode: "agent", resource: "empty" },
  "agent-session-mode-narrow": { mode: "agent", resource: "empty" },
  "agent-session-mode-forced-colors": { mode: "agent", resource: "empty" },
  editor: { mode: "editor", resource: "file" },
  "editor-picker-repository": { mode: "editor", resource: "file" },
  "editor-picker-repository-filtered": { mode: "editor", resource: "file" },
  "editor-picker-narrow": { mode: "editor", resource: "file" },
  "editor-picker-zoom-200": { mode: "editor", resource: "file" },
  file: { mode: "agent", resource: "file" },
  diff: { mode: "agent", resource: "diff" },
  preview: { mode: "agent", resource: "preview" },
  "agent-preview-starting": { mode: "agent", resource: "preview" },
  "agent-preview-health-failed": { mode: "agent", resource: "preview" },
  "agent-preview-exited": { mode: "agent", resource: "preview" },
  error: { mode: "agent", resource: "empty" },
  waiting: { mode: "agent", resource: "empty" },
};

function defineScenario(name: VisualScenarioName): VisualScenarioDefinition {
  return { name, ...SCENARIO_META[name] };
}

const VISUAL_SURFACE_SCENARIOS: Record<
  VisualSurface,
  readonly VisualScenarioName[]
> = {
  agent: AGENT_VISUAL_SCENARIOS,
  editor: EDITOR_VISUAL_SCENARIOS,
  resource: [
    ...RESOURCE_VISUAL_SCENARIOS,
    ...PREVIEW_PROJECTION_FIXTURE_SCENARIOS,
  ],
  recovery: RECOVERY_VISUAL_SCENARIOS,
};

export const VISUAL_SCENARIOS: readonly VisualScenarioDefinition[] = [
  ...AGENT_VISUAL_SCENARIOS,
  ...EDITOR_VISUAL_SCENARIOS,
  ...RESOURCE_VISUAL_SCENARIOS,
  ...PREVIEW_PROJECTION_FIXTURE_SCENARIOS,
  ...RECOVERY_VISUAL_SCENARIOS,
].map(defineScenario);

export function scenarioSurface(name: VisualScenarioName): VisualSurface {
  for (const surface of Object.keys(
    VISUAL_SURFACE_SCENARIOS,
  ) as VisualSurface[]) {
    if (VISUAL_SURFACE_SCENARIOS[surface].includes(name)) return surface;
  }
  throw new Error(`Unknown visual scenario: ${name}`);
}
