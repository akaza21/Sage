import { describe, expect, it } from "vitest";

import { agentWorkbenchLayout } from "./agentWorkbenchLayout";

describe("Agent navigation allocation", () => {
  it.each([480, 760, 1000, 1280, 1888])(
    "keeps only the 46px icon rail when navigation is collapsed at %i",
    (viewportWidth) => {
      for (const artifactExpanded of [false, true]) {
        const layout = agentWorkbenchLayout({
          viewportWidth,
          artifactExpanded,
          navigationCollapsed: true,
        });
        expect(layout.leftRailWidth).toBe(46);
        expect(layout.navigationResizable).toBe(false);
        expect(layout.gridTemplateColumns).toMatch(/^46px minmax/);
        expect(layout.chatWidth).toBe(
          viewportWidth -
            46 -
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
    [720, false, 326, 394],
    [761, false, 341, 420],
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
    [719, 326, 393],
    [720, 326, 394],
    [760, 326, 434],
    [761, 326, 427],
    [1280, 326, 946],
    [1888, 326, 1554],
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
    [719, 326, 393],
    [720, 326, 394],
    [760, 326, 434],
    [761, 326, 427],
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
    [1040, 326, 338, 360],
    [1041, 326, 339, 360],
    [1280, 326, 576, 362],
    [1888, 326, 850, 696],
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
