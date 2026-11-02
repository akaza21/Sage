import { describe, expect, it } from "vitest";

import { agentWorkbenchLayout } from "./agentWorkbenchLayout";

describe("Agent navigation allocation", () => {
  it.each([800, 936, 1024])(
    "preserves a usable chat and an inline Files pane at %i",
    (viewportWidth) => {
      const layout = agentWorkbenchLayout({
        viewportWidth,
        artifactExpanded: true,
        requestedWidths: { leftRailWidth: 385, artifactWidth: 1200 },
      });
      expect(layout.artifactOverlay).toBe(false);
      expect(layout.chatWidth).toBeGreaterThanOrEqual(360);
      expect(layout.artifactWidth).toBeGreaterThanOrEqual(280);
      expect(
        layout.leftRailWidth + layout.chatWidth + layout.artifactWidth,
      ).toBeLessThan(viewportWidth);
    },
  );
  it.each([480, 760, 1000, 1280, 1888])(
    "keeps only the 54px icon rail when navigation is collapsed at %i",
    (viewportWidth) => {
      for (const artifactExpanded of [false, true]) {
        const layout = agentWorkbenchLayout({
          viewportWidth,
          artifactExpanded,
          navigationCollapsed: true,
        });
        expect(layout.leftRailWidth).toBe(54);
        expect(layout.navigationResizable).toBe(false);
        expect(layout.gridTemplateColumns).toMatch(/^54px minmax/);
        expect(layout.chatWidth).toBe(
          viewportWidth -
            54 -
            (artifactExpanded && !layout.artifactOverlay
              ? layout.artifactWidth + 8
              : 0),
        );
        expect(layout.chatWidth).toBeGreaterThanOrEqual(360);
      }
    },
  );
  it.each([
    [480, true, 60, 420],
    [623, true, 203, 420],
    [624, false, 304, 320],
    [700, false, 326, 374],
    [709, false, 329, 380],
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
    [479, 262, 217, true],
    [480, 262, 218, true],
    [719, 54, 377, false],
    [720, 54, 378, false],
    [760, 54, 418, false],
    [761, 54, 419, false],
    [1040, 384, 360, false],
  ])(
    "fits resources beside the chat before using an overlay at %i",
    (viewportWidth, navigation, chat, overlay) => {
      const layout = agentWorkbenchLayout({
        viewportWidth,
        artifactExpanded: true,
      });
      expect(layout.leftRailWidth).toBe(navigation);
      expect(layout.chatWidth).toBe(chat);
      expect(layout.artifactOverlay).toBe(overlay);
    },
  );

  it.each([
    [1041, 385, 280, 360],
    [1280, 385, 519, 360],
    [1888, 385, 850, 637],
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
