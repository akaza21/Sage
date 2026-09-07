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
  editor: { mode: "editor", resource: "file" },
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
