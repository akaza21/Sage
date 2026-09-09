import { describe, expect, it } from "vitest";

import { VISUAL_SCENARIOS } from "./visualScenarioCatalog";
import { buildVisualScenario } from "./visualScenarioShell";

describe("visual scenario contract", () => {
  it("keeps the Wave 2 scenario matrix explicit and reviewable", () => {
    expect(VISUAL_SCENARIOS).toEqual([
      { name: "agent-single", mode: "agent", resource: "empty" },
      { name: "agent-dual", mode: "agent", resource: "empty" },
      { name: "agent-long", mode: "agent", resource: "empty" },
      { name: "agent-activity-family", mode: "agent", resource: "empty" },
      { name: "agent-image", mode: "agent", resource: "empty" },
      { name: "agent-child-activity", mode: "agent", resource: "empty" },
      { name: "editor", mode: "editor", resource: "file" },
      {
        name: "editor-picker-repository",
        mode: "editor",
        resource: "file",
      },
      {
        name: "editor-picker-repository-filtered",
        mode: "editor",
        resource: "file",
      },
      { name: "editor-picker-narrow", mode: "editor", resource: "file" },
      { name: "editor-picker-zoom-200", mode: "editor", resource: "file" },
      { name: "file", mode: "agent", resource: "file" },
      { name: "diff", mode: "agent", resource: "diff" },
      { name: "preview", mode: "agent", resource: "preview" },
      {
        name: "agent-preview-starting",
        mode: "agent",
        resource: "preview",
      },
      {
        name: "agent-preview-health-failed",
        mode: "agent",
        resource: "preview",
      },
      { name: "agent-preview-exited", mode: "agent", resource: "preview" },
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

  it("builds one deterministic admitted image in a dual-width Chat state", () => {
    const scenario = buildVisualScenario("agent-image");
    const imageItems = scenario.pages.flatMap((page) =>
      page.turns.flatMap((turn) =>
        turn.items.filter((item) => item.kind === "image"),
      ),
    );

    expect(scenario.pages).toHaveLength(2);
    expect(imageItems).toHaveLength(1);
    expect(imageItems[0]).toMatchObject({
      payload: {
        mediaType: "image/png",
      },
    });
    expect(imageItems[0]?.payload.dataUrl).toMatch(
      /^data:image\/png;base64,iVBOR/,
    );
    expect(imageItems[0]?.payload.dataUrl.length).toBeLessThan(262_144);
  });

  it("builds one bounded read-only collaboration hierarchy across levels one to four", () => {
    const scenario = buildVisualScenario("agent-child-activity");
    const turn = scenario.pages[0]?.turns[0];
    const collaborationItems = turn?.items.filter(
      (item) => item.kind === "collaboration",
    );

    expect(scenario.pages).toHaveLength(1);
    expect(turn?.activities.map((activity) => activity.depth)).toEqual([
      1, 2, 3, 4,
    ]);
    expect(
      turn?.activities.map((activity) => activity.parentActivityId),
    ).toEqual([
      null,
      "visual-child-activity-1",
      "visual-child-activity-2",
      "visual-child-activity-3",
    ]);
    expect(collaborationItems).toHaveLength(4);
    expect(
      collaborationItems?.every((item) => item.origin === "harnessActivity"),
    ).toBe(true);
    expect(
      collaborationItems?.every((item) => item.payload.label.length > 60),
    ).toBe(true);
  });
});
