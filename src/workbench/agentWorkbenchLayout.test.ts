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
    "keeps only the 48px icon rail when navigation is collapsed at %i",
    (viewportWidth) => {
      for (const artifactExpanded of [false, true]) {
        const layout = agentWorkbenchLayout({
          viewportWidth,
          artifactExpanded,
          navigationCollapsed: true,
        });
        expect(layout.leftRailWidth).toBe(48);
        expect(layout.navigationResizable).toBe(false);
        expect(layout.gridTemplateColumns).toMatch(/^48px minmax/);
        expect(layout.chatWidth).toBe(
          viewportWidth -
            48 -
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
    [367, true, 16, 351],
    [368, false, 48, 320],
    [480, false, 48, 432],
    [701, false, 48, 653],
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
    [479, 48, 431],
    [760, 48, 712],
    [799, 48, 751],
    [800, 304, 488],
    [1280, 304, 968],
    [1888, 304, 1576],
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
    [704, 288, 360],
    [720, 304, 360],
    [800, 360, 384],
    [900, 405, 439],
    [1024, 461, 507],
  ])(
    "folds navigation so resources dock beside the chat at %i",
    (viewportWidth, resource, chat) => {
      const layout = agentWorkbenchLayout({
        viewportWidth,
        artifactExpanded: true,
      });
      expect(layout.navigationCollapsed).toBe(true);
      expect(layout.leftRailWidth).toBe(48);
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
    [1279, 304, 576, 383],
    [1280, 304, 576, 384],
    [1600, 304, 720, 560],
    [1888, 304, 850, 718],
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
    ["artifact", 304, 600],
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
    expect(layout.leftRailWidth).toBe(48);
    expect(layout.chatWidth).toBe(432);
  });

  it.each([
    [480, 460, 460],
    [420, undefined, 304],
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
      expect(layout.leftRailWidth).toBe(48);
      expect(layout.chatWidth).toBe(viewportWidth - 48);
      expect(layout.gridTemplateColumns).toBe("48px minmax(0, 1fr)");
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
    expect(layout.leftRailWidth).toBe(304);
    expect(layout.leftRailBounds).toMatchObject({
      minWidth: 304,
      maxWidth: 432,
    });
  });

  it.each([
    [960, 280, 360],
    [1100, 300, 480],
  ])(
    "admits inline resources at %i when resized navigation leaves room",
    (viewportWidth, artifactWidth, chatWidth) => {
      const layout = agentWorkbenchLayout({
        viewportWidth,
        artifactExpanded: true,
        requestedWidths: { leftRailWidth: 304, artifactWidth },
      });
      expect(layout.artifactOverlay).toBe(false);
      expect(layout.leftRailWidth).toBe(304);
      expect(layout.artifactWidth).toBe(artifactWidth);
      expect(layout.chatWidth).toBe(chatWidth);
    },
  );
});
