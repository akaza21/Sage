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
      agentChatSplitPercent: 61,
      windowMode: "agent",
    });
  });

  it("falls back to defaults when stored layout is absent or malformed", () => {
    localStorage.setItem(WORKBENCH_LAYOUT_STORAGE_KEY, "{");

    expect(readWorkbenchLayout()).toEqual({
      editorToolRailWidth: 272,
      agentChatSplitPercent: 50,
      windowMode: "editor",
    });
  });

  it("fails closed to Editor when persisted window mode is invalid", () => {
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
      agentChatSplitPercent: 55,
      windowMode: "editor",
    });
  });

  it("writes only the bounded layout record to WebView storage", () => {
    writeWorkbenchLayout({
      editorToolRailWidth: 300,
      agentChatSplitPercent: 55,
      windowMode: "agent",
    });

    expect(
      JSON.parse(localStorage.getItem(WORKBENCH_LAYOUT_STORAGE_KEY)!),
    ).toEqual({
      editorToolRailWidth: 300,
      agentChatSplitPercent: 55,
      windowMode: "agent",
    });
  });
});
