import { expect, it } from "vitest";
import { currentProjectLayout } from "./projectProjection";
import { useWorkbenchStore } from "./store";

it("compacts stale tab positions before persisting layout for a chat send", () => {
  const state = useWorkbenchStore.getState();
  const tabs = [
    {
      id: "settings",
      kind: "settings" as const,
      title: "Settings",
      position: 0,
      pinned: false,
      dirty: false,
    },
    {
      id: "browser-1",
      kind: "browser" as const,
      title: "Browser",
      position: 2,
      pinned: false,
      dirty: false,
    },
    {
      id: "browser-2",
      kind: "browser" as const,
      title: "Browser",
      position: 2,
      pinned: false,
      dirty: false,
    },
  ];
  const layout = currentProjectLayout({
    ...state,
    project: { id: "project", name: "Project", roots: [] },
    tabs,
  });
  expect(layout?.tabs.map((tab) => [tab.id, tab.position])).toEqual([
    ["settings", 0],
    ["browser-1", 1],
    ["browser-2", 2],
  ]);
  expect(tabs.map((tab) => tab.position)).toEqual([0, 2, 2]);
});

it("preserves a valid gapped layout for authoritative transition comparisons", () => {
  const state = useWorkbenchStore.getState();
  const tabs = [
    {
      id: "chat",
      kind: "chat" as const,
      title: "Chat",
      position: 2,
      pinned: false,
      dirty: false,
    },
  ];
  const layout = currentProjectLayout({
    ...state,
    project: { id: "project", name: "Project", roots: [] },
    tabs,
  });
  expect(layout?.tabs).toBe(tabs);
});
