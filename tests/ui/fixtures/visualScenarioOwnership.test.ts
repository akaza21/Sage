import { describe, expect, it } from "vitest";

import {
  AGENT_VISUAL_SCENARIOS,
  EDITOR_VISUAL_SCENARIOS,
  RECOVERY_VISUAL_SCENARIOS,
  RESOURCE_VISUAL_SCENARIOS,
  type VisualScenarioName,
} from "./visualScenarioTypes";
import { VISUAL_SCENARIOS, scenarioSurface } from "./visualScenarioCatalog";
import { buildAgentPages } from "./surfaces/agentVisualFixtures";
import { buildEditorPages } from "./surfaces/editorVisualFixtures";
import { buildRecoveryPages } from "./surfaces/recoveryVisualFixtures";
import { buildResourcePages } from "./surfaces/resourceVisualFixtures";
import {
  installVisualAgentArtifactFollowFixture,
  installVisualAgentResourceState,
} from "./surfaces/resourceVisualFixtures";
import {
  installVisualDirtyEditor,
  installVisualEmptyEditorCanvas,
  installVisualEmptyEditorTools,
  installVisualExplorerActionFixture,
  installVisualExplorerScrollFixture,
  installVisualExplorerUnavailableActionFixture,
  installVisualRepositoryError,
  installVisualRepositoryLoading,
  installVisualRepositoryPrimaryState,
} from "./surfaces/editorVisualFixtures";
import * as barrel from "./visualScenarios";
import { buildVisualScenario } from "./visualScenarioShell";

const SLICES = {
  agent: AGENT_VISUAL_SCENARIOS,
  editor: EDITOR_VISUAL_SCENARIOS,
  resource: RESOURCE_VISUAL_SCENARIOS,
  recovery: RECOVERY_VISUAL_SCENARIOS,
} as const;

const BUILDERS = {
  agent: buildAgentPages,
  editor: buildEditorPages,
  resource: buildResourcePages,
  recovery: buildRecoveryPages,
} as const;

describe("visual scenario surface ownership", () => {
  it("keeps the full catalogue explicit in its pre-split order", () => {
    expect(VISUAL_SCENARIOS.map((entry) => entry.name)).toEqual([
      "agent-single",
      "agent-dual",
      "agent-long",
      "agent-activity-family",
      "agent-image",
      "agent-child-activity",
      "editor",
      "file",
      "diff",
      "preview",
      "error",
      "waiting",
    ]);
  });

  it("assigns every scenario to exactly one surface with no duplicate names", () => {
    const owned = Object.values(SLICES).flat();
    expect(owned).toHaveLength(VISUAL_SCENARIOS.length);
    expect(new Set(owned).size).toBe(owned.length);
    expect([...owned].sort()).toEqual(
      VISUAL_SCENARIOS.map((entry) => entry.name).sort(),
    );
    const inOrder = [
      ...SLICES.agent,
      ...SLICES.editor,
      ...SLICES.resource,
      ...SLICES.recovery,
    ];
    expect(inOrder).toEqual(VISUAL_SCENARIOS.map((entry) => entry.name));
  });

  it("keeps the surface slices pairwise disjoint", () => {
    const slices = Object.values(SLICES);
    for (let outer = 0; outer < slices.length; outer += 1) {
      for (let inner = outer + 1; inner < slices.length; inner += 1) {
        expect(
          slices[outer].filter((name) => slices[inner].includes(name)),
        ).toEqual([]);
      }
    }
  });

  it("maps each scenario name to its owning surface", () => {
    const expected: Record<VisualScenarioName, keyof typeof SLICES> = {
      "agent-single": "agent",
      "agent-dual": "agent",
      "agent-long": "agent",
      "agent-activity-family": "agent",
      "agent-image": "agent",
      "agent-child-activity": "agent",
      editor: "editor",
      file: "resource",
      diff: "resource",
      preview: "resource",
      error: "recovery",
      waiting: "recovery",
    };
    for (const scenario of VISUAL_SCENARIOS) {
      expect(scenarioSurface(scenario.name)).toBe(expected[scenario.name]);
    }
    expect(() =>
      scenarioSurface("unknown-scenario" as VisualScenarioName),
    ).toThrow("Unknown visual scenario");
  });

  it("lets exactly one surface builder claim each scenario", () => {
    for (const scenario of VISUAL_SCENARIOS) {
      const claimed = (
        Object.entries(BUILDERS) as Array<
          [keyof typeof BUILDERS, (typeof BUILDERS)[keyof typeof BUILDERS]]
        >
      ).filter(([, build]) => build(scenario.name) !== null);
      expect(claimed.map(([surface]) => surface)).toEqual([
        scenarioSurface(scenario.name),
      ]);
    }
  });

  it("builds the same deterministic pages through the dispatcher and the owning builder", () => {
    for (const scenario of VISUAL_SCENARIOS) {
      const builder = BUILDERS[scenarioSurface(scenario.name)];
      expect(buildVisualScenario(scenario.name).pages).toEqual(
        builder(scenario.name),
      );
      expect(buildVisualScenario(scenario.name)).toEqual(
        buildVisualScenario(scenario.name),
      );
    }
  });

  it("keeps the compatibility barrel to its exact single-route allowlist", () => {
    expect(Object.keys(barrel).sort()).toEqual(["installVisualScenario"]);
    expect(typeof barrel.installVisualScenario).toBe("function");
  });

  it("keeps every surface installer resolving from its owning module", () => {
    for (const installer of [
      installVisualAgentResourceState,
      installVisualAgentArtifactFollowFixture,
      installVisualExplorerScrollFixture,
      installVisualExplorerActionFixture,
      installVisualExplorerUnavailableActionFixture,
      installVisualRepositoryError,
      installVisualRepositoryLoading,
      installVisualRepositoryPrimaryState,
      installVisualEmptyEditorTools,
      installVisualEmptyEditorCanvas,
      installVisualDirtyEditor,
    ]) {
      expect(typeof installer).toBe("function");
    }
  });
});
