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
    [360, true, 16, 344],
    [365, true, 16, 349],
    [366, false, 46, 320],
    [480, false, 46, 434],
    [701, false, 46, 655],
  ])(
    "covers the whole chat column beside the icon rail with overlays at %i",
    (viewportWidth, modal, left, width) => {
      const layout = agentWorkbenchLayout({
        viewportWidth,
        artifactExpanded: true,
      });
      expect(layout.navigationCollapsed).toBe(true);
      expect(layout.artifactOverlay).toBe(true);
      expect(layout.artifactModal).toBe(modal);
      expect(layout.artifactOverlayLeft).toBe(left);
      expect(layout.artifactOverlayWidth).toBe(width);
      if (!modal) expect(layout.artifactOverlayWidth).toBe(layout.chatWidth);
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
    [702, 288, 360],
    [720, 306, 360],
    [800, 360, 386],
    [900, 405, 441],
    [1024, 461, 509],
  ])(
    "folds navigation so resources dock beside the chat at %i",
    (viewportWidth, resource, chat) => {
      const layout = agentWorkbenchLayout({
        viewportWidth,
        artifactExpanded: true,
      });
      expect(layout.navigationCollapsed).toBe(true);
      expect(layout.leftRailWidth).toBe(46);
      expect(layout.artifactOverlay).toBe(false);
      expect(layout.artifactWidth).toBe(resource);
      expect(layout.chatWidth).toBe(chat);
      expect(
        agentWorkbenchLayout({ viewportWidth, artifactExpanded: false })
          .navigationCollapsed,
      ).toBe(false);
    },
  );

  it.each([
    [1279, 326, 576, 361],
    [1280, 326, 576, 362],
    [1600, 326, 720, 538],
    [1888, 326, 850, 696],
  ])(
    "keeps the full rail beside inline resources at %i",
    (viewportWidth, navigation, resource, chat) => {
      const layout = agentWorkbenchLayout({
        viewportWidth,
        artifactExpanded: true,
      });
      expect(layout.navigationCollapsed).toBe(false);
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

  it("keeps the rail while dragging it wider leaves Files their floor", () => {
    const layout = agentWorkbenchLayout({
      viewportWidth: 1280,
      artifactExpanded: true,
      requestedWidths: { leftRailWidth: 340 },
      prioritize: "left",
    });
    expect(layout.navigationCollapsed).toBe(false);
    expect(layout.navigationResizable).toBe(true);
    expect(layout.leftRailWidth).toBe(340);
    expect(layout.artifactWidth).toBe(564);
    expect(layout.chatWidth).toBe(360);
  });

  it("keeps narrow navigation reference-sized even with wide splitter overrides", () => {
    const layout = agentWorkbenchLayout({
      viewportWidth: 480,
      artifactExpanded: false,
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
    [836, 280, 360],
    [1000, 300, 504],
  ])(
    "admits inline resources at %i when resized navigation leaves room",
    (viewportWidth, artifactWidth, chatWidth) => {
      const layout = agentWorkbenchLayout({
        viewportWidth,
        artifactExpanded: true,
        requestedWidths: { leftRailWidth: 180, artifactWidth },
      });
      expect(layout.artifactOverlay).toBe(false);
      expect(layout.leftRailWidth).toBe(180);
      expect(layout.artifactWidth).toBe(artifactWidth);
      expect(layout.chatWidth).toBe(chatWidth);
    },
  );
});
