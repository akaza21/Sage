import { describe, expect, it } from "vitest";

import { agentWorkbenchLayout } from "./agentWorkbenchLayout";

describe("Agent navigation allocation", () => {
  it.each([
    [480, true, 60, 420],
    [623, true, 203, 420],
    [624, false, 304, 320],
    [720, false, 332, 388],
    [761, false, 352, 409],
  ])(
    "bounds overlays without overlapping interactive navigation at %i",
    (viewportWidth, modal, left, width) => {
      const layout = agentWorkbenchLayout({
        viewportWidth,
        artifactExpanded: true,
      });
      expect(layout.artifactModal).toBe(modal);
      expect(layout.artifactOverlayLeft).toBe(left);
      expect(layout.artifactOverlayWidth).toBe(width);
    },
  );

  it.each([
    [479, 262, 217],
    [480, 262, 218],
    [719, 332, 387],
    [720, 332, 388],
    [760, 344, 416],
    [761, 344, 409],
    [1280, 385, 887],
    [1888, 385, 1495],
  ])(
    "retains navigation at %i without reserving a closed resource edge",
    (viewportWidth, navigation, chat) => {
      const layout = agentWorkbenchLayout({
        viewportWidth,
        artifactExpanded: false,
      });
      expect(layout.leftRailWidth).toBe(navigation);
      expect(layout.chatWidth).toBe(chat);
      expect(layout.artifactOverlay).toBe(false);
    },
  );

  it.each([
    [479, 262, 217],
    [480, 262, 218],
    [719, 332, 387],
    [720, 332, 388],
    [760, 344, 416],
    [761, 344, 409],
    [1040, 385, 647],
  ])(
    "opens resources over the same chat allocation at %i",
    (viewportWidth, navigation, chat) => {
      const layout = agentWorkbenchLayout({
        viewportWidth,
        artifactExpanded: true,
      });
      expect(layout.leftRailWidth).toBe(navigation);
      expect(layout.chatWidth).toBe(chat);
      expect(layout.artifactOverlay).toBe(true);
    },
  );

  it.each([
    [1041, 385, 280, 360],
    [1280, 385, 519, 360],
    [1888, 385, 1020, 467],
  ])(
    "admits inline resources with a usable chat at %i",
    (viewportWidth, navigation, resource, chat) => {
      const layout = agentWorkbenchLayout({
        viewportWidth,
        artifactExpanded: true,
      });
      expect(layout.leftRailWidth).toBe(navigation);
      expect(layout.artifactWidth).toBe(resource);
      expect(layout.chatWidth).toBe(chat);
      expect(layout.artifactOverlay).toBe(false);
    },
  );

  it.each([
    ["left", 460, 444],
    ["artifact", 180, 724],
  ] as const)(
    "bounds %s splitter extremes without consuming the chat floor",
    (prioritize, navigation, resource) => {
      const layout = agentWorkbenchLayout({
        viewportWidth: 1280,
        artifactExpanded: true,
        requestedWidths: { leftRailWidth: 460, artifactWidth: 1200 },
        prioritize,
      });
      expect(layout.leftRailWidth).toBe(navigation);
      expect(layout.artifactWidth).toBe(resource);
      expect(layout.chatWidth).toBe(360);
    },
  );

  it("keeps narrow navigation reference-sized even with wide splitter overrides", () => {
    const layout = agentWorkbenchLayout({
      viewportWidth: 480,
      artifactExpanded: true,
      requestedWidths: { leftRailWidth: 460, artifactWidth: 1200 },
    });
    expect(layout.leftRailWidth).toBe(262);
    expect(layout.chatWidth).toBe(218);
    expect(layout.navigationResizable).toBe(false);
    expect(layout.leftRailBounds).toMatchObject({
      minWidth: 262,
      maxWidth: 262,
    });
  });

  it("admits inline resources below the default threshold when resized navigation leaves room", () => {
    const layout = agentWorkbenchLayout({
      viewportWidth: 1000,
      artifactExpanded: true,
      requestedWidths: { leftRailWidth: 180, artifactWidth: 300 },
    });
    expect(layout.artifactOverlay).toBe(false);
    expect(layout.leftRailWidth).toBe(180);
    expect(layout.artifactWidth).toBe(300);
    expect(layout.chatWidth).toBe(504);
  });
});
