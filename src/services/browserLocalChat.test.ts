import { describe, expect, it } from "vitest";

import type { BootstrapSnapshot, WorkbenchTab } from "../domain/workbench";
import { pinBrowserChat } from "./browserLocalChat";

function snapshot(
  overrides: Partial<BootstrapSnapshot> = {},
): BootstrapSnapshot {
  const chat: WorkbenchTab = {
    id: "chat:sage",
    kind: "chat",
    title: "Sage",
    position: 0,
    pinned: false,
    dirty: false,
  };
  return {
    transitionEpoch: 0,
    projects: [
      { id: "sage", name: "Sage", rootCount: 1, lastOpenedAt: 1 },
      {
        id: "cross",
        name: "Cross",
        rootCount: 1,
        lastOpenedAt: 2,
        railChats: [
          {
            id: "chat:cross",
            title: "Research",
            pinned: false,
            position: 0,
          },
        ],
      },
    ],
    project: { id: "sage", name: "Sage", roots: [] },
    tabs: [chat],
    archivedChats: [],
    activeTabId: chat.id,
    selectedRootId: null,
    selectedResource: null,
    harnesses: [],
    sessionId: null,
    selectedHarnessId: null,
    selectedModelId: null,
    messages: [],
    repositories: { roots: [], truncated: false, elapsedMs: 0 },
    turnChanges: [],
    recoveredBuffers: [],
    recoveryNotice: null,
    ...overrides,
  };
}

describe("pinBrowserChat", () => {
  it("pins a foreign-folder rail chat without switching the active project", () => {
    const current = snapshot();
    const result = pinBrowserChat({
      snapshot: current,
      request: { projectId: "cross", chatId: "chat:cross" },
      expectedTransitionEpoch: 0,
      activeMessages: () => [],
    });
    expect(result.committed).toBe(true);
    expect(current.project?.id).toBe("sage");
    expect(current.tabs[0]?.pinned).toBe(false);
    expect(
      current.projects.find((item) => item.id === "cross")?.railChats?.[0]
        ?.pinned,
    ).toBe(true);
  });

  it("refuses an unknown chat pin", () => {
    expect(() =>
      pinBrowserChat({
        snapshot: snapshot(),
        request: { projectId: "cross", chatId: "missing" },
        expectedTransitionEpoch: 0,
        activeMessages: () => [],
      }),
    ).toThrow("Chat is not attached to that Project");
  });
});
