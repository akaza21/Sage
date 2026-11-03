import { describe, expect, it } from "vitest";

import {
  AGENT_ARTIFACT_WIDTH,
  AGENT_CHAT_MIN_WIDTH,
  AGENT_LEFT_RAIL_WIDTH,
  EDITOR_TOOL_RAIL_WIDTH,
  fitAgentDesktopSidebarWidths,
  initialAgentArtifactWidth,
  initialAgentDesktopSidebarWidths,
  initialAgentLeftRailWidth,
  resizeWorkbenchSidebar,
} from "./workbenchSidebarResize";

describe("resizeWorkbenchSidebar", () => {
  // The owner's Codex desktop capture (1872 px wide, 100% scale): the icon
  // rail plus the chat list end at x = 326.
  it("opens the icon rail and project sidebar at the captured Codex width", () => {
    expect(initialAgentLeftRailWidth(1936)).toBe(326);
    expect(initialAgentLeftRailWidth(1872)).toBe(326);
    expect(initialAgentArtifactWidth(1936)).toBe(871);
    expect(initialAgentDesktopSidebarWidths(1380)).toEqual({
      leftRailWidth: 326,
      artifactWidth: 621,
    });
  });
  // The artifact pane opens at 45% of the window. Beside the 326 px sidebar
  // at 1888 that leaves Chat about 45% of the remaining space.
  it("opens the desktop Agent rail and artifact pane at the reference proportions", () => {
    expect(initialAgentLeftRailWidth(1672)).toBe(326);
    expect(initialAgentArtifactWidth(1672)).toBe(752);
    expect(initialAgentLeftRailWidth(1380)).toBe(326);
    expect(initialAgentArtifactWidth(1380)).toBe(621);
    const pane = initialAgentArtifactWidth(1888);
    const chat = 1888 - initialAgentLeftRailWidth(1888) - 16 - pane;
    expect(pane / (pane + chat)).toBeCloseTo(0.55, 2);
  });

  it("keeps a usable Chat column when either desktop splitter reaches its end", () => {
    const artifactFirst = fitAgentDesktopSidebarWidths({
      viewportWidth: 900,
      leftRailWidth: AGENT_LEFT_RAIL_WIDTH.minWidth,
      artifactWidth: AGENT_ARTIFACT_WIDTH.maxWidth,
      prioritize: "artifact",
    });
    const railFirst = fitAgentDesktopSidebarWidths({
      viewportWidth: 900,
      leftRailWidth: AGENT_LEFT_RAIL_WIDTH.maxWidth,
      artifactWidth: AGENT_ARTIFACT_WIDTH.minWidth,
      prioritize: "left",
    });

    for (const widths of [artifactFirst, railFirst]) {
      expect(
        900 - widths.leftRailWidth - widths.artifactWidth - 16,
      ).toBeGreaterThanOrEqual(AGENT_CHAT_MIN_WIDTH);
    }
  });

  it("caps initial Agent proportions without resetting a dragged width on resize", () => {
    expect(initialAgentDesktopSidebarWidths(800)).toEqual({
      leftRailWidth: 180,
      artifactWidth: 244,
    });
    expect(
      fitAgentDesktopSidebarWidths({
        viewportWidth: 1500,
        leftRailWidth: 320,
        artifactWidth: 500,
        prioritize: "artifact",
      }),
    ).toEqual({ leftRailWidth: 320, artifactWidth: 500 });
  });

  it("keeps each resizable workbench column within its usable width bounds", () => {
    expect(
      resizeWorkbenchSidebar(AGENT_LEFT_RAIL_WIDTH, { pointerOffset: 90 }),
    ).toBe(180);
    expect(
      resizeWorkbenchSidebar(EDITOR_TOOL_RAIL_WIDTH, { pointerOffset: 314 }),
    ).toBe(314);
    expect(
      resizeWorkbenchSidebar(AGENT_ARTIFACT_WIDTH, { pointerOffset: 800 }),
    ).toBe(800);
  });

  it("uses keyboard steps and Home or End within the same bounds", () => {
    expect(
      resizeWorkbenchSidebar(AGENT_LEFT_RAIL_WIDTH, { key: "ArrowRight" }),
    ).toBe(334);
    expect(
      resizeWorkbenchSidebar(EDITOR_TOOL_RAIL_WIDTH, {
        key: "ArrowLeft",
        shiftKey: true,
      }),
    ).toBe(252);
    expect(resizeWorkbenchSidebar(AGENT_ARTIFACT_WIDTH, { key: "Home" })).toBe(
      280,
    );
    expect(resizeWorkbenchSidebar(AGENT_ARTIFACT_WIDTH, { key: "End" })).toBe(
      1200,
    );
  });
});
