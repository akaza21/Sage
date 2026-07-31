import { describe, expect, it } from "vitest";

import type { WorkbenchTab } from "../domain/workbench";
import { closeTab, nextActiveTabId, orderedTabs } from "./tabState";

const tabs: WorkbenchTab[] = [
  {
    id: "chat",
    kind: "chat",
    title: "Chat",
    position: 1,
    pinned: false,
    dirty: false,
  },
  {
    id: "diff",
    kind: "diff",
    title: "store.ts",
    position: 0,
    pinned: true,
    dirty: false,
    resource: { rootId: "root-a", relativePath: "src/app/store.ts" },
  },
  {
    id: "file",
    kind: "file",
    title: "main.rs",
    position: 2,
    pinned: false,
    dirty: false,
  },
];

describe("tab state", () => {
  it("orders tabs by their durable position", () => {
    expect(orderedTabs(tabs).map((tab) => tab.id)).toEqual([
      "diff",
      "chat",
      "file",
    ]);
  });

  it("activates the next tab when the current tab closes", () => {
    expect(nextActiveTabId(tabs, "chat", "chat")).toBe("file");
  });

  it("keeps the active tab when another tab closes", () => {
    expect(nextActiveTabId(tabs, "chat", "file")).toBe("chat");
  });

  it("does not silently close a dirty file", () => {
    const dirtyTabs = tabs.map((tab) =>
      tab.id === "file" ? { ...tab, dirty: true } : tab,
    );
    expect(closeTab(dirtyTabs, "file", "file")).toEqual({
      tabs: dirtyTabs,
      activeTabId: "file",
    });
  });
});
