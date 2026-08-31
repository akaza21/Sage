import { describe, expect, it } from "vitest";

import { buildVisualScenario, VISUAL_SCENARIOS } from "./visualScenarios";

describe("visual scenario contract", () => {
  it("keeps the Wave 2 scenario matrix explicit and reviewable", () => {
    expect(VISUAL_SCENARIOS).toEqual([
      { name: "agent-single", mode: "agent", resource: "empty" },
      { name: "agent-dual", mode: "agent", resource: "empty" },
      { name: "agent-long", mode: "agent", resource: "empty" },
      { name: "editor", mode: "editor", resource: "file" },
      { name: "diff", mode: "agent", resource: "diff" },
      { name: "preview", mode: "agent", resource: "preview" },
      { name: "error", mode: "agent", resource: "empty" },
      { name: "waiting", mode: "agent", resource: "empty" },
    ]);
  });

  it("builds byte-for-byte deterministic fixture state", () => {
    for (const scenario of VISUAL_SCENARIOS) {
      expect(buildVisualScenario(scenario.name)).toEqual(
        buildVisualScenario(scenario.name),
      );
    }
  });

  it("bounds the long transcript while preserving a running final turn", () => {
    const scenario = buildVisualScenario("agent-long");
    const page = scenario.pages[0];

    expect(page.turns).toHaveLength(14);
    expect(page.turns.at(-1)).toMatchObject({
      state: "running",
      terminalAt: null,
    });
  });
});
