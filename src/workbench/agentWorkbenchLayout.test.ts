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
    [479, 46, 433],
    [760, 46, 714],
    [799, 46, 753],
    [800, 302, 490],
    [1280, 302, 970],
    [1888, 302, 1578],
  ])(
    "docks the 256px panel from 800px without reserving a closed resource edge at %i",
    (viewportWidth, navigation, chat) => {
      const layout = agentWorkbenchLayout({
        viewportWidth,
        artifactExpanded: false,
      });
      expect(layout.leftRailWidth).toBe(navigation);
      expect(layout.chatWidth).toBe(chat);
      expect(layout.navigationAutoHidden).toBe(viewportWidth < 800);
      expect(layout.navigationCollapsed).toBe(viewportWidth < 800);
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
      ).toBe(viewportWidth < 800);
    },
  );

  it.each([
    [1279, 302, 576, 385],
    [1280, 302, 576, 386],
    [1600, 302, 720, 562],
    [1888, 302, 850, 720],
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
    ["artifact", 302, 602],
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

  it("auto-hides the panel under 800px whatever width was dragged", () => {
    const layout = agentWorkbenchLayout({
      viewportWidth: 480,
      artifactExpanded: false,
      requestedWidths: { leftRailWidth: 460, artifactWidth: 1200 },
    });
    expect(layout.navigationAutoHidden).toBe(true);
    expect(layout.navigationCollapsed).toBe(true);
    expect(layout.navigationOverlay).toBe(false);
    expect(layout.navigationResizable).toBe(false);
    expect(layout.leftRailWidth).toBe(46);
    expect(layout.chatWidth).toBe(434);
  });

  it.each([
    [480, 460, 460],
    [420, undefined, 302],
    [300, undefined, 284],
  ])(
    "brings the panel back over the chat at %i without squeezing it",
    (viewportWidth, leftRailWidth, overlayWidth) => {
      const layout = agentWorkbenchLayout({
        viewportWidth,
        artifactExpanded: false,
        navigationOverlayOpen: true,
        requestedWidths: { leftRailWidth },
      });
      expect(layout.navigationOverlay).toBe(true);
      expect(layout.navigationOverlayWidth).toBe(overlayWidth);
      expect(layout.leftRailWidth).toBe(46);
      expect(layout.chatWidth).toBe(viewportWidth - 46);
      expect(layout.gridTemplateColumns).toBe("46px minmax(0, 1fr)");
    },
  );

  it("restores the docked panel from 800px and ignores a stale overlay", () => {
    const layout = agentWorkbenchLayout({
      viewportWidth: 800,
      artifactExpanded: false,
      navigationOverlayOpen: true,
    });
    expect(layout.navigationAutoHidden).toBe(false);
    expect(layout.navigationOverlay).toBe(false);
    expect(layout.navigationCollapsed).toBe(false);
    expect(layout.leftRailWidth).toBe(302);
    expect(layout.leftRailBounds).toMatchObject({
      minWidth: 302,
      maxWidth: 432,
    });
  });

  it.each([
    [958, 280, 360],
    [1100, 300, 482],
  ])(
    "admits inline resources at %i when resized navigation leaves room",
    (viewportWidth, artifactWidth, chatWidth) => {
      const layout = agentWorkbenchLayout({
        viewportWidth,
        artifactExpanded: true,
        requestedWidths: { leftRailWidth: 302, artifactWidth },
      });
      expect(layout.artifactOverlay).toBe(false);
      expect(layout.leftRailWidth).toBe(302);
      expect(layout.artifactWidth).toBe(artifactWidth);
      expect(layout.chatWidth).toBe(chatWidth);
    },
  );
});
