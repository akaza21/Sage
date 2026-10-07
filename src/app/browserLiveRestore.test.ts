import { afterEach, expect, it, vi } from "vitest";

import type { BootstrapSnapshot, WorkbenchTab } from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { useWorkbenchStore } from "./store";

const browserLiveTabs = vi.fn();

vi.mock("../ports/nativeBrowserPort", () => ({
  browserLiveTabs: () => browserLiveTabs(),
  closeBrowser: vi.fn(async () => undefined),
  openBrowser: vi.fn(),
}));

const original = useWorkbenchStore.getState();

const chat: WorkbenchTab = {
  id: "chat-live-restore",
  kind: "chat",
  title: "Chat",
  position: 0,
  pinned: false,
  dirty: false,
};
const browserTabs: WorkbenchTab[] = ["browser-1", "browser-2"].map(
  (id, index) => ({
    id,
    kind: "browser",
    title: `Browser ${index + 1}`,
    position: index + 1,
    pinned: false,
    dirty: false,
  }),
);
const restoredSnapshot: BootstrapSnapshot = {
  transitionEpoch: 1,
  projects: [
    {
      id: "project-live-restore",
      name: "Project",
      rootCount: 0,
      lastOpenedAt: 1,
    },
  ],
  project: { id: "project-live-restore", name: "Project", roots: [] },
  tabs: [
    chat,
    ...browserTabs.map((tab) => ({ ...tab, status: "closed" as const })),
  ],
  archivedChats: [],
  activeTabId: "browser-2",
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
};

afterEach(() => {
  vi.restoreAllMocks();
  browserLiveTabs.mockReset();
  useWorkbenchStore.setState(original);
});

it("keeps two native-live browser tabs open across an in-session Agent-to-Editor projection reload", async () => {
  useWorkbenchStore.setState({
    project: restoredSnapshot.project,
    tabs: [chat, ...browserTabs],
    activeTabId: "browser-2",
    projectTransitioning: false,
  });
  browserLiveTabs.mockResolvedValue(["browser-1", "browser-2"]);
  vi.spyOn(
    getWorkbenchPorts().project,
    "getBootstrapSnapshot",
  ).mockResolvedValue(restoredSnapshot);

  await useWorkbenchStore.getState().initialize();

  expect(
    useWorkbenchStore.getState().tabs.filter((tab) => tab.kind === "browser"),
  ).toEqual(browserTabs);
});

it("keeps restored browser tabs closed when no native handles exist", async () => {
  useWorkbenchStore.setState({ project: null, tabs: [], activeTabId: null });
  browserLiveTabs.mockResolvedValue([]);
  vi.spyOn(
    getWorkbenchPorts().project,
    "getBootstrapSnapshot",
  ).mockResolvedValue(restoredSnapshot);

  await useWorkbenchStore.getState().initialize();

  expect(
    useWorkbenchStore.getState().tabs.filter((tab) => tab.kind === "browser"),
  ).toEqual(restoredSnapshot.tabs.filter((tab) => tab.kind === "browser"));
});
