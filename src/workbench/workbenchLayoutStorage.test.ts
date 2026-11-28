import { afterEach, describe, expect, it } from "vitest";

import {
  WORKBENCH_LAYOUT_STORAGE_KEY,
  readWorkbenchLayout,
  writeWorkbenchLayout,
} from "./workbenchLayoutStorage";

afterEach(() => {
  localStorage.clear();
});

describe("workbench layout storage", () => {
  it("restores the bounded window mode and single-splitter layout values from WebView storage", () => {
    localStorage.setItem(
      WORKBENCH_LAYOUT_STORAGE_KEY,
      JSON.stringify({
        agentLeftRailWidth: 999,
        agentArtifactWidth: 12,
        editorToolRailWidth: 333,
        agentChatSplitPercent: 61,
        windowMode: "agent",
      }),
    );

    expect(readWorkbenchLayout()).toEqual({
      editorToolRailWidth: 333,
      windowMode: "agent",
    });
  });

  it("falls back to defaults when stored layout is absent or malformed", () => {
    localStorage.setItem(WORKBENCH_LAYOUT_STORAGE_KEY, "{");

    expect(readWorkbenchLayout()).toEqual({
      editorToolRailWidth: 272,
      windowMode: "agent",
    });
  });

  it("falls back to Agent when persisted window mode is invalid", () => {
    localStorage.setItem(
      WORKBENCH_LAYOUT_STORAGE_KEY,
      JSON.stringify({
        editorToolRailWidth: 300,
        agentChatSplitPercent: 55,
        windowMode: "split",
      }),
    );

    expect(readWorkbenchLayout()).toEqual({
      editorToolRailWidth: 300,
      windowMode: "agent",
    });
  });

  it("defaults a fresh profile to Agent and preserves an explicit Editor preference", () => {
    expect(readWorkbenchLayout().windowMode).toBe("agent");
    writeWorkbenchLayout({ windowMode: "editor" });
    expect(readWorkbenchLayout().windowMode).toBe("editor");
  });

  it.each([
    [100, 256],
    [300, 300],
    [999, 456],
  ])(
    "clamps a stored Agent panel width of %i to the 256-456px range",
    (stored, restored) => {
      localStorage.setItem(
        WORKBENCH_LAYOUT_STORAGE_KEY,
        JSON.stringify({ editorToolRailWidth: 300, agentSidebarWidth: stored }),
      );
      expect(readWorkbenchLayout().agentSidebarWidth).toBe(restored);
    },
  );

  it("keeps a dragged Agent panel width beside later editor writes", () => {
    writeWorkbenchLayout({ agentSidebarWidth: 380 });
    writeWorkbenchLayout({ editorToolRailWidth: 310, windowMode: "editor" });
    expect(readWorkbenchLayout()).toEqual({
      editorToolRailWidth: 310,
      windowMode: "editor",
      agentSidebarWidth: 380,
    });
  });

  it("writes only the bounded layout record to WebView storage", () => {
    writeWorkbenchLayout({
      editorToolRailWidth: 300,
      windowMode: "agent",
    });

    expect(
      JSON.parse(localStorage.getItem(WORKBENCH_LAYOUT_STORAGE_KEY)!),
    ).toEqual({
      editorToolRailWidth: 300,
      windowMode: "agent",
    });
  });
});
