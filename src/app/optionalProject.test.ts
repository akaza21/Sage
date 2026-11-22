import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { resetBrowserDesktopForTests } from "../services/browserDesktop";
import { currentProjectLayout } from "./projectProjection";
import { useWorkbenchStore } from "./store";
import { browserDirectories } from "../services/browserWorkspaceFixture";

const initial = useWorkbenchStore.getState();
beforeEach(async () => {
  resetBrowserDesktopForTests();
  useWorkbenchStore.setState(initial, true);
  await useWorkbenchStore.getState().initialize();
});
afterEach(() => {
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initial, true);
});

it.each(["chat-first", "resources-first", "empty"])(
  "archives the last private chat with native selection normalization: %s",
  async (order) => {
    const state = useWorkbenchStore.getState();
    await state.createChat(null);
    const privateState = useWorkbenchStore.getState();
    const chat = privateState.activeTabId!;
    const rootId = privateState.project!.roots[0].id;
    if (order !== "empty") {
      await state.openFile({ rootId, relativePath: "first.txt" });
      await state.openFile({ rootId, relativePath: "second.txt" });
    }
    const files = useWorkbenchStore
      .getState()
      .tabs.filter((tab) => tab.kind === "file");
    if (order === "resources-first") {
      // Array order need not be persisted position order.
      useWorkbenchStore.setState((current) => ({
        tabs: current.tabs.map((tab) => ({
          ...tab,
          position: tab.id === chat ? 2 : tab.id === files[0].id ? 1 : 0,
        })),
      }));
    }
    await state.activateTab(chat);
    const ports = getWorkbenchPorts();
    const archive = ports.chat.archiveChat.bind(ports.chat);
    vi.spyOn(ports.chat, "archiveChat").mockImplementation(async (...args) => {
      const result = await archive(...args);
      const snapshot = result.snapshot!;
      // Native restore_layout selects the first tab by persisted position
      // when the configured active tab is absent (unlike browserDesktop).
      snapshot.tabs.sort((left, right) => left.position - right.position);
      snapshot.activeTabId ??= snapshot.tabs[0]?.id ?? null;
      return result;
    });
    expect(await state.archiveChat(chat)).toBe(true);
    expect(useWorkbenchStore.getState()).toMatchObject({
      phase: "ready",
      error: null,
      projectTransitioning: false,
      activeTabId:
        order === "empty" ? null : files[order === "chat-first" ? 0 : 1].id,
      archivedChats: [{ id: chat }],
    });
    expect(
      useWorkbenchStore
        .getState()
        .tabs.map((tab) => tab.id)
        .sort(),
    ).toEqual(files.map((tab) => tab.id).sort());
  },
);

it.each(["delete", "move"])(
  "releases private directory listings after %s",
  async (action) => {
    const state = useWorkbenchStore.getState();
    const target = state.project!.id;
    const initialListings = browserDirectories.size;
    for (let cycle = 0; cycle < 2; cycle++) {
      await state.createChat(null);
      expect(browserDirectories.size).toBe(initialListings + 1);
      if (action === "move") await state.setChatProject(target);
      else {
        const chat = useWorkbenchStore.getState().activeTabId!;
        expect(await state.archiveChat(chat)).toBe(true);
        expect(await state.deleteChat(chat)).toBe(true);
      }
      expect(browserDirectories.size).toBe(initialListings);
    }
  },
);

it("re-parents an empty chat without losing its prompt, captured context or harness choices", async () => {
  const state = useWorkbenchStore.getState();
  const project = state.project!.id;
  await state.createChat();
  const chat = useWorkbenchStore.getState().activeTabId!;
  const owner = useWorkbenchStore.getState().captureChatOwnership(chat)!;
  state.setChatComposer(owner, "Keep my first prompt");
  state.setChatHarnessSelection(owner, {
    harnessId: "sage-simulator",
    modelId: "sim-fast",
  });
  const context = {
    kind: "browserPage" as const,
    tabId: "browser-1",
    url: "https://example.test/",
    title: "Reference",
    text: "Captured page text",
  };
  expect(state.attachChatContext(owner, context)).toBe(true);
  await state.setChatProject(null);
  expect(useWorkbenchStore.getState()).toMatchObject({
    activeTabId: chat,
    composerText: "Keep my first prompt",
    project: { isInternal: true },
  });
  expect(useWorkbenchStore.getState().chatUiById[chat]).toMatchObject({
    harnessId: "sage-simulator",
    modelId: "sim-fast",
    attachedContext: [context],
  });
  await state.setChatProject(project);
  expect(useWorkbenchStore.getState()).toMatchObject({
    activeTabId: chat,
    composerText: "Keep my first prompt",
    project: { id: project },
  });
});

it("restores and deletes an archived project-less chat while another project is open", async () => {
  const state = useWorkbenchStore.getState();
  const real = state.project!.id;
  const created = await state.createChat(null);
  expect(created.status).toBe("confirmed");
  const privateState = useWorkbenchStore.getState();
  const project = privateState.project!.id;
  const chat = privateState.activeTabId!;
  expect(await state.archiveChat(chat)).toBe(true);
  await state.switchProject(real);
  expect(useWorkbenchStore.getState().project?.id).toBe(real);
  expect(await state.restoreChat(chat, project)).toBe(true);
  expect(useWorkbenchStore.getState().unscopedChats[0].railChats?.[0].id).toBe(
    chat,
  );
  await state.openRailChat(project, chat);
  expect(useWorkbenchStore.getState().activeTabId).toBe(chat);
  expect(await state.archiveChat(chat)).toBe(true);
  await state.switchProject(real);
  expect(await state.deleteChat(chat, project)).toBe(true);
  expect(useWorkbenchStore.getState().unscopedChats).toEqual([]);
});

it.each([
  { requestFocus: true, center: "draft", source: "Home" },
  { requestFocus: false, center: "chat", source: "continuity" },
  {
    requestFocus: false,
    center: "draft",
    source: "Home during continuity",
    home: true,
  },
])(
  "honors $source intent while a project move binds its watcher",
  async ({ requestFocus, center, home }) => {
    const state = useWorkbenchStore.getState();
    const target = state.project!.id;
    await state.createChat(null);
    const chat = useWorkbenchStore.getState().activeTabId!;
    let bindingStarted!: () => void;
    let releaseBinding!: () => void;
    const started = new Promise<void>((resolve) => (bindingStarted = resolve));
    const held = new Promise<void>((resolve) => (releaseBinding = resolve));
    vi.spyOn(
      getWorkbenchPorts().project,
      "watchWorkspaceNotify",
    ).mockImplementationOnce(async () => {
      bindingStarted();
      await held;
      throw new Error("Fixture watcher unavailable");
    });
    const moving = state.setChatProject(target);
    await started;
    try {
      expect(useWorkbenchStore.getState().projectTransitioning).toBe(false);
      const opening = state.openNewChatDraft(undefined, { requestFocus });
      if (home) await state.openNewChatDraft();
      await opening;
    } finally {
      releaseBinding();
      await moving;
    }
    expect(useWorkbenchStore.getState()).toMatchObject({
      activeTabId: chat,
      project: { id: target },
      agentCenter: center,
    });
  },
);

it("new-chat entry points distinguish a project chat from a top-level chat", async () => {
  const state = useWorkbenchStore.getState();
  await state.openNewChatDraft(state.project!.id);
  expect(useWorkbenchStore.getState().newChatDraft?.targetProjectId).toBe(
    state.project!.id,
  );
  state.discardNewChatDraft();
  await state.openNewChatDraft();
  expect(useWorkbenchStore.getState().newChatDraft?.targetProjectId).toBeNull();
  expect(currentProjectLayout(useWorkbenchStore.getState())?.projectId).toBe(
    state.project!.id,
  );
});

it("creates a project with no durable chat rows", async () => {
  const state = useWorkbenchStore.getState();
  expect(
    await state.createProject({ name: "Fresh project", sourceTokens: [] }),
  ).toBe(true);
  const fresh = useWorkbenchStore.getState();
  expect(fresh.tabs.filter((tab) => tab.kind === "chat")).toEqual([]);
  expect(fresh.activeTabId).toBeNull();
  expect(
    (await getWorkbenchPorts().project.getBootstrapSnapshot()).tabs,
  ).toEqual([]);
});

it("leaves a fresh profile without a project or a durable private placeholder", async () => {
  const snapshot = await getWorkbenchPorts().project.getBootstrapSnapshot();
  vi.spyOn(
    getWorkbenchPorts().project,
    "getBootstrapSnapshot",
  ).mockResolvedValueOnce({
    ...snapshot,
    project: null,
    projects: [],
    tabs: [],
    archivedChats: [],
    activeTabId: null,
    selectedRootId: null,
    selectedResource: null,
  });
  await useWorkbenchStore.getState().initialize();
  expect(useWorkbenchStore.getState()).toMatchObject({
    project: null,
    tabs: [],
    activeTabId: null,
    newChatDraft: null,
  });
});
