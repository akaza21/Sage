import { expect, it } from "vitest";
import type { BootstrapSnapshot, HarnessDefinition } from "../domain/workbench";
import { currentProjectLayout, snapshotProjection } from "./projectProjection";
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

it("shows the remembered listing only for the harness the snapshot restored", () => {
  const harness = (id: string, installState: HarnessDefinition["installState"]) =>
    ({
      id,
      name: id,
      description: "",
      source: "builtIn",
      accountModelDiscovery: false,
      transport: "acp",
      installState,
      bindingId: installState === "available" ? `${id}-binding` : null,
      versionChecking: installState !== "available",
    }) as HarnessDefinition;
  const snapshot = {
    transitionEpoch: 0,
    projects: [],
    project: null,
    tabs: [],
    archivedChats: [],
    activeTabId: null,
    selectedRootId: null,
    selectedResource: null,
    harnesses: [harness("fast", "available"), harness("slow", "unavailable")],
    sessionId: null,
    selectedHarnessId: "slow",
    selectedModelId: "sonnet",
    rememberedModels: [{ id: "sonnet", name: "Sonnet" }],
    messages: [],
    repositories: { roots: [], truncated: false, elapsedMs: 0 },
    turnChanges: [],
    recoveredBuffers: [],
    recoveryNotice: null,
  } as BootstrapSnapshot;
  // Still being checked, the restored harness keeps its place and listing.
  const restored = snapshotProjection(snapshot);
  expect(restored.selectedHarnessId).toBe("slow");
  expect(restored.selectedModelId).toBe("sonnet");
  expect(restored.models).toEqual(snapshot.rememberedModels);
  // Once it failed its check, another harness's choice shows no listing.
  const fallback = snapshotProjection({
    ...snapshot,
    harnesses: [harness("fast", "available"), {
      ...harness("slow", "unavailable"),
      versionChecking: false,
    }],
  });
  expect(fallback.selectedHarnessId).toBe("fast");
  expect(fallback.models).toEqual([]);
});
