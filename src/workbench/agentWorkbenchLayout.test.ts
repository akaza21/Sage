import { describe, expect, it } from "vitest";

import { agentWorkbenchLayout } from "./agentWorkbenchLayout";

describe("Agent navigation allocation", () => {
  it.each([1041, 1280, 1888])(
    "preserves the chat and Files floors at splitter extremes at %i",
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
    [624, true, 204, 420],
    [720, true, 300, 420],
    [761, true, 341, 420],
  ])(
    "bounds modal overlays with background navigation inert at %i",
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
    [800, 326, 466],
    [936, 326, 602],
    [981, 326, 647],
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
    [982, 326, 280, 360],
    [1040, 326, 338, 360],
    [1041, 326, 339, 360],
    [1280, 326, 538, 400],
    [1888, 326, 793, 753],
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

  it.each([
    [836, 280, 648, true],
    [1000, 300, 504, false],
  ])(
    "keeps resources modal below the inline threshold at %i even with resized navigation",
    (viewportWidth, artifactWidth, chatWidth, overlay) => {
      const layout = agentWorkbenchLayout({
        viewportWidth,
        artifactExpanded: true,
        requestedWidths: { leftRailWidth: 180, artifactWidth },
      });
      expect(layout.artifactOverlay).toBe(overlay);
      expect(layout.leftRailWidth).toBe(180);
      expect(layout.artifactWidth).toBe(artifactWidth);
      expect(layout.chatWidth).toBe(chatWidth);
    },
  );
});
