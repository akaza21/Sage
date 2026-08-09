import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { resourceKey } from "../domain/resourceIdentity";
import type {
  BootstrapSnapshot,
  BufferSnapshotAck,
  ChatMessage,
  DirectoryListing,
  EditorDocument,
  FileDiff,
  ModelOption,
  ProjectRepositoryStatus,
  ResourceRef,
  WorkbenchTab,
} from "../domain/workbench";
import { desktop } from "../services/desktop";
import { useWorkbenchStore } from "./store";

const resource = (relativePath: string, rootId = "root-a"): ResourceRef => ({
  rootId,
  relativePath,
});
const chatTab: WorkbenchTab = {
  id: "chat:a",
  kind: "chat",
  title: "Chat: A",
  position: 0,
  pinned: true,
  dirty: false,
};
const chatTabB: WorkbenchTab = {
  id: "chat:b",
  kind: "chat",
  title: "Chat: B",
  position: 1,
  pinned: false,
  dirty: false,
  status: "idle",
};

function document(
  target: ResourceRef,
  content: string,
  revision: number,
  dirty: boolean,
): EditorDocument {
  const version = {
    fingerprint: dirty ? "base" : `saved-${revision}`,
    byteLen: content.length,
    modifiedAt: null,
  };
  return {
    resource: target,
    disk: version,
    base: version,
    buffer: {
      fingerprint: dirty ? `buffer-${revision}` : version.fingerprint,
      byteLen: content.length,
      revision,
      content,
    },
    lineEnding: "lf",
    dirty,
    conflict: null,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function snapshotB(): BootstrapSnapshot {
  return {
    transitionEpoch: 1,
    projects: [
      { id: "project-a", name: "A", rootCount: 1, lastOpenedAt: 1 },
      { id: "project-b", name: "B", rootCount: 1, lastOpenedAt: 2 },
    ],
    project: {
      id: "project-b",
      name: "B",
      roots: [
        {
          id: "root-b",
          name: "B",
          ordinal: 0,
          availability: "available",
          message: null,
        },
      ],
    },
    tabs: [
      {
        id: "chat:b",
        kind: "chat",
        title: "Chat: B",
        position: 0,
        pinned: true,
        dirty: false,
      },
    ],
    archivedChats: [],
    activeTabId: "chat:b",
    selectedRootId: "root-b",
    selectedResource: null,
    harnesses: [],
    sessionId: "session-b",
    selectedHarnessId: null,
    selectedModelId: null,
    messages: [],
    repositories: { roots: [], truncated: false, elapsedMs: 0 },
    turnChanges: [],
    recoveredBuffers: [],
    recoveryNotice: null,
  };
}

function chatProjection(
  tabs: WorkbenchTab[],
  activeTabId: string,
  transitionEpoch: number,
  sessionId: string | null,
  messages: ChatMessage[],
  selectedRootId = "root-a",
  selectedResource: ResourceRef | null = null,
  archivedChats: BootstrapSnapshot["archivedChats"] = [],
): BootstrapSnapshot {
  const state = useWorkbenchStore.getState();
  return {
    transitionEpoch,
    projects: state.projects,
    project: state.project,
    tabs,
    archivedChats,
    activeTabId,
    selectedRootId,
    selectedResource,
    harnesses: [],
    sessionId,
    selectedHarnessId: null,
    selectedModelId: null,
    messages,
    repositories: { roots: [], truncated: false, elapsedMs: 0 },
    turnChanges: [],
    recoveredBuffers: [],
    recoveryNotice: null,
  };
}

describe("workbench resource and project navigation", () => {
  beforeEach(() => {
    useWorkbenchStore.setState({
      phase: "ready",
      nativeTransitionEpoch: 0,
      projects: [{ id: "project-a", name: "A", rootCount: 1, lastOpenedAt: 1 }],
      project: {
        id: "project-a",
        name: "A",
        roots: [
          {
            id: "root-a",
            name: "A",
            ordinal: 0,
            availability: "available",
            message: null,
          },
          {
            id: "root-b",
            name: "B",
            ordinal: 1,
            availability: "available",
            message: null,
          },
        ],
      },
      tabs: [chatTab],
      archivedChats: [],
      activeTabId: chatTab.id,
      selectedRootId: "root-a",
      selectedResource: null,
      sidebarOpen: false,
      sidebarView: "sourceControl",
      error: null,
      projectGeneration: 0,
      projectTransitioning: false,
      directoryListings: {},
      expandedDirectories: {},
      activeEditor: null,
      fileNavigationTarget: null,
      pendingCloseTabId: null,
      diffs: {},
      requestGenerations: {},
      requestIntentSequence: 0,
      repositories: { roots: [], truncated: false, elapsedMs: 0 },
      repositoryRefreshing: false,
      harnesses: [],
      selectedHarnessId: null,
      selectedModelId: null,
      models: [],
      sendRequestGeneration: 0,
      chatOwnershipEpoch: 0,
      chatUiById: {},
      chatError: null,
      composerText: "",
      sending: false,
      sessionId: "session-a",
      messages: [],
      recoveryNotice: null,
    });
    vi.spyOn(desktop, "activateChat").mockImplementation(
      async (currentLayout, expectedTransitionEpoch) => {
        const state = useWorkbenchStore.getState();
        return {
          committed: true,
          transitionEpoch: expectedTransitionEpoch + 1,
          snapshot: {
            transitionEpoch: expectedTransitionEpoch + 1,
            projects: state.projects,
            project: state.project,
            tabs: currentLayout.tabs,
            archivedChats: state.archivedChats,
            activeTabId: currentLayout.activeTabId,
            selectedRootId: currentLayout.selectedRootId,
            selectedResource: currentLayout.selectedResource,
            harnesses: state.harnesses,
            sessionId: null,
            selectedHarnessId: null,
            selectedModelId: null,
            messages: [],
            repositories: state.repositories,
            turnChanges: state.turnChanges,
            recoveredBuffers: [],
            recoveryNotice: null,
          },
          recoveryError: null,
        };
      },
    );
    vi.spyOn(desktop, "renameChat").mockImplementation(
      async (_request, expectedTransitionEpoch) => ({
        committed: false,
        transitionEpoch: expectedTransitionEpoch,
        snapshot: null,
        recoveryError: "rename not configured",
      }),
    );
  });

  afterEach(() => vi.restoreAllMocks());

  it("addresses simulator prompts by Chat and adopts the durable session head", async () => {
    const messages = [
      {
        id: "message-new",
        sessionId: "session-new",
        sequence: 2,
        role: "assistant" as const,
        text: "ack",
        createdAt: 2,
      },
    ];
    const send = vi.spyOn(desktop, "sendPrompt").mockResolvedValue({
      sessionId: "session-new",
      messages,
    });
    useWorkbenchStore.setState({
      selectedHarnessId: "sage-simulator",
      selectedModelId: "sim-fast",
      sessionId: null,
      composerText: "continue",
    });

    await useWorkbenchStore.getState().sendPrompt();

    expect(send).toHaveBeenCalledWith({
      chatId: chatTab.id,
      harnessId: "sage-simulator",
      modelId: "sim-fast",
      text: "continue",
    });
    expect(useWorkbenchStore.getState()).toMatchObject({
      sessionId: "session-new",
      messages,
      sending: false,
    });
  });

  it("hands an edited ordinary prompt to an authoritatively activated target without sending or copying history", async () => {
    const send = vi.spyOn(desktop, "sendPrompt");
    const targetMessages: ChatMessage[] = [
      {
        id: "target-only",
        sessionId: "session-b",
        sequence: 1,
        role: "assistant",
        text: "Target history",
        createdAt: 1,
      },
    ];
    const targetProjection = chatProjection(
      [chatTab, chatTabB],
      chatTabB.id,
      1,
      "session-b",
      targetMessages,
    );
    vi.spyOn(desktop, "activateChat").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: targetProjection,
      recoveryError: null,
    });
    useWorkbenchStore.setState({
      tabs: [chatTab, chatTabB],
      activeTabId: chatTab.id,
      composerText: "source draft stays source-only",
      messages: [
        {
          id: "source-only",
          sessionId: "session-a",
          sequence: 1,
          role: "user",
          text: "Source history",
          createdAt: 1,
        },
      ],
    });

    const committed = await useWorkbenchStore.getState().handoffChat({
      projectId: "project-a",
      projectGeneration: 0,
      transitionEpoch: 0,
      sourceChatId: chatTab.id,
      targetChatId: chatTabB.id,
      draft: "Edited handoff prompt",
    });

    expect(committed).toBe(true);
    expect(send).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState()).toMatchObject({
      activeTabId: chatTabB.id,
      messages: targetMessages,
      composerText: "Edited handoff prompt",
      nativeTransitionEpoch: 1,
      projectGeneration: 1,
    });
    expect(useWorkbenchStore.getState().chatUiById).toMatchObject({
      [chatTab.id]: { composerText: "source draft stays source-only" },
      [chatTabB.id]: { composerText: "Edited handoff prompt" },
    });

    vi.spyOn(desktop, "getBootstrapSnapshot").mockResolvedValue(
      targetProjection,
    );
    vi.spyOn(desktop, "listDirectory").mockImplementation(async (target) => ({
      resource: target,
      entries: [],
      truncated: false,
    }));
    await useWorkbenchStore.getState().initialize();
    expect(useWorkbenchStore.getState()).toMatchObject({
      activeTabId: chatTabB.id,
      messages: targetMessages,
      composerText: "",
    });
    expect(useWorkbenchStore.getState().chatUiById).toMatchObject({
      [chatTab.id]: { composerText: "" },
      [chatTabB.id]: { composerText: "" },
    });
  });

  it("rejects stale empty foreign or changed-source handoff intents without navigation", async () => {
    const activate = vi.spyOn(desktop, "activateChat");
    useWorkbenchStore.setState({
      tabs: [chatTab, chatTabB],
      activeTabId: chatTab.id,
      composerText: "source draft",
    });
    const valid = {
      projectId: "project-a",
      projectGeneration: 0,
      transitionEpoch: 0,
      sourceChatId: chatTab.id,
      targetChatId: chatTabB.id,
      draft: "Handoff prompt",
    };

    for (const intent of [
      { ...valid, projectGeneration: 1 },
      { ...valid, transitionEpoch: 1 },
      { ...valid, sourceChatId: chatTabB.id },
      { ...valid, targetChatId: "chat:foreign" },
      { ...valid, targetChatId: chatTab.id },
      { ...valid, draft: " \n " },
    ]) {
      expect(await useWorkbenchStore.getState().handoffChat(intent)).toBe(
        false,
      );
    }
    expect(activate).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState()).toMatchObject({
      activeTabId: chatTab.id,
      composerText: "source draft",
      messages: [],
    });
  });

  it("keeps source state isolated when handoff editor flush or navigation fails", async () => {
    const selectedResource = resource("src/dirty.ts");
    vi.spyOn(desktop, "snapshotEditorBuffer").mockRejectedValueOnce(
      new Error("snapshot unavailable"),
    );
    const activate = vi.spyOn(desktop, "activateChat");
    useWorkbenchStore.setState({
      tabs: [chatTab, chatTabB],
      activeTabId: chatTab.id,
      composerText: "source draft",
      activeEditor: {
        phase: "ready",
        resource: selectedResource,
        document: document(selectedResource, "base", 0, false),
        content: "dirty",
        revision: 1,
        persistedRevision: 0,
        persisting: false,
        saving: false,
        message: null,
      },
    });
    const intent = {
      projectId: "project-a",
      projectGeneration: 0,
      transitionEpoch: 0,
      sourceChatId: chatTab.id,
      targetChatId: chatTabB.id,
      draft: "Handoff prompt",
    };

    expect(await useWorkbenchStore.getState().handoffChat(intent)).toBe(false);
    expect(activate).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState().composerText).toBe("source draft");

    useWorkbenchStore.setState({ activeEditor: null, error: null });
    activate.mockResolvedValueOnce({
      committed: false,
      transitionEpoch: 0,
      snapshot: null,
      recoveryError: "activation rejected",
    });
    expect(await useWorkbenchStore.getState().handoffChat(intent)).toBe(false);
    expect(useWorkbenchStore.getState()).toMatchObject({
      activeTabId: chatTab.id,
      composerText: "source draft",
      messages: [],
      projectTransitioning: false,
    });
  });

  it("creates a Chat from the current layout and adopts its authoritative projection", async () => {
    const newChat: WorkbenchTab = {
      id: "chat:new",
      kind: "chat",
      title: "Chat: New chat 2",
      position: 1,
      pinned: false,
      dirty: false,
      status: "idle",
    };
    const createdSnapshot: BootstrapSnapshot = {
      transitionEpoch: 1,
      projects: useWorkbenchStore.getState().projects,
      project: useWorkbenchStore.getState().project,
      tabs: [chatTab, newChat],
      archivedChats: [],
      activeTabId: newChat.id,
      selectedRootId: "root-a",
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
    const create = vi.spyOn(desktop, "createChat").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: createdSnapshot,
      recoveryError: null,
    });
    vi.spyOn(desktop, "listDirectory").mockImplementation(async (target) => ({
      resource: target,
      entries: [],
      truncated: false,
    }));

    await useWorkbenchStore.getState().createChat();

    expect(create).toHaveBeenCalledWith(
      {
        projectId: "project-a",
        tabs: [chatTab],
        activeTabId: chatTab.id,
        selectedRootId: "root-a",
        selectedResource: null,
      },
      0,
    );
    expect(useWorkbenchStore.getState()).toMatchObject({
      nativeTransitionEpoch: 1,
      activeTabId: newChat.id,
      tabs: [chatTab, newChat],
      sessionId: null,
      messages: [],
      projectTransitioning: false,
    });
  });

  it("renames an inactive Chat from its authoritative projection", async () => {
    const renamedTab = { ...chatTabB, title: "Chat: Focus" };
    const selectedResource = resource("src/current.ts", "root-b");
    const fileTab: WorkbenchTab = {
      id: "file:current",
      kind: "file",
      title: "current.ts",
      position: 2,
      pinned: false,
      dirty: true,
      status: "idle",
      resource: selectedResource,
    };
    const snapshot = vi
      .spyOn(desktop, "snapshotEditorBuffer")
      .mockResolvedValue({ revision: 1, persistedAt: 1, dirty: true });
    const pendingRename =
      deferred<Awaited<ReturnType<typeof desktop.renameChat>>>();
    const rename = vi
      .spyOn(desktop, "renameChat")
      .mockReturnValue(pendingRename.promise);
    useWorkbenchStore.setState({
      tabs: [chatTab, chatTabB, fileTab],
      activeTabId: fileTab.id,
      selectedRootId: "root-b",
      selectedResource,
      activeEditor: {
        phase: "ready",
        resource: selectedResource,
        document: document(selectedResource, "before rename", 0, false),
        content: "latest unsnapshotted bytes",
        revision: 1,
        persistedRevision: 0,
        persisting: false,
        saving: false,
        message: null,
      },
    });

    const renaming = useWorkbenchStore
      .getState()
      .renameChat(chatTabB.id, "  Focus  ");

    await vi.waitFor(() => expect(rename).toHaveBeenCalledOnce());
    useWorkbenchStore
      .getState()
      .updateEditorContent("post-flush bytes must be rejected");
    expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
      phase: "ready",
      content: "latest unsnapshotted bytes",
      revision: 1,
      persistedRevision: 1,
    });
    pendingRename.resolve({
      committed: true,
      transitionEpoch: 1,
      snapshot: chatProjection(
        [chatTab, renamedTab, fileTab],
        fileTab.id,
        1,
        null,
        [],
        "root-b",
        selectedResource,
      ),
      recoveryError: null,
    });
    const committed = await renaming;

    expect(committed).toBe(true);
    expect(snapshot).toHaveBeenCalledWith(
      expect.objectContaining({
        resource: selectedResource,
        content: "latest unsnapshotted bytes",
        revision: 1,
      }),
    );
    expect(snapshot.mock.invocationCallOrder[0]).toBeLessThan(
      rename.mock.invocationCallOrder[0],
    );
    expect(rename).toHaveBeenCalledWith(
      {
        projectId: "project-a",
        chatId: chatTabB.id,
        title: "Focus",
        currentLayout: {
          projectId: "project-a",
          tabs: [chatTab, chatTabB, fileTab],
          activeTabId: fileTab.id,
          selectedRootId: "root-b",
          selectedResource,
        },
      },
      0,
    );
    expect(useWorkbenchStore.getState()).toMatchObject({
      activeTabId: fileTab.id,
      tabs: [chatTab, renamedTab, fileTab],
      selectedRootId: "root-b",
      selectedResource,
      sessionId: null,
      messages: [],
      projectTransitioning: false,
    });
  });

  it("blocks Chat rename when the newest editor snapshot fails", async () => {
    const selectedResource = resource("src/unsnapshotted.ts");
    const fileTab: WorkbenchTab = {
      id: "file:unsnapshotted",
      kind: "file",
      title: "unsnapshotted.ts",
      position: 2,
      pinned: false,
      dirty: true,
      resource: selectedResource,
    };
    vi.spyOn(desktop, "snapshotEditorBuffer").mockRejectedValue(
      new Error("snapshot unavailable"),
    );
    const rename = vi.spyOn(desktop, "renameChat");
    useWorkbenchStore.setState({
      tabs: [chatTab, chatTabB, fileTab],
      activeTabId: fileTab.id,
      selectedResource,
      activeEditor: {
        phase: "ready",
        resource: selectedResource,
        document: document(selectedResource, "before rename", 0, false),
        content: "must survive",
        revision: 1,
        persistedRevision: 0,
        persisting: false,
        saving: false,
        message: null,
      },
    });

    const committed = await useWorkbenchStore
      .getState()
      .renameChat(chatTabB.id, "Blocked");

    expect(committed).toBe(false);
    expect(rename).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState()).toMatchObject({
      projectTransitioning: false,
      error: expect.stringContaining("snapshot unavailable"),
      activeEditor: {
        phase: "ready",
        content: "must survive",
        revision: 1,
        persistedRevision: 0,
      },
    });
  });

  it("archives the active Chat through an editor-safe authoritative transition", async () => {
    const archive = vi.spyOn(desktop, "archiveChat").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: chatProjection(
        [chatTabB],
        chatTabB.id,
        1,
        null,
        [],
        "root-a",
        null,
        [{ id: chatTab.id, title: chatTab.title }],
      ),
      recoveryError: null,
    });
    useWorkbenchStore.setState({ tabs: [chatTab, chatTabB] });

    const committed = await useWorkbenchStore
      .getState()
      .archiveChat(chatTab.id);

    expect(committed).toBe(true);
    expect(archive).toHaveBeenCalledWith(
      {
        projectId: "project-a",
        chatId: chatTab.id,
        currentLayout: expect.objectContaining({
          tabs: [chatTab, chatTabB],
          activeTabId: chatTab.id,
        }),
      },
      0,
    );
    expect(useWorkbenchStore.getState()).toMatchObject({
      tabs: [chatTabB],
      activeTabId: chatTabB.id,
      archivedChats: [{ id: chatTab.id, title: chatTab.title }],
      projectTransitioning: false,
    });
  });

  it("flushes and freezes the active File editor before archiving an inactive Chat", async () => {
    const selectedResource = resource("src/current.ts");
    const fileTab: WorkbenchTab = {
      id: "file:current",
      kind: "file",
      title: "current.ts",
      position: 2,
      pinned: false,
      dirty: true,
      status: "idle",
      resource: selectedResource,
    };
    const snapshot = vi
      .spyOn(desktop, "snapshotEditorBuffer")
      .mockResolvedValue({ revision: 1, persistedAt: 1, dirty: true });
    const pendingArchive =
      deferred<Awaited<ReturnType<typeof desktop.archiveChat>>>();
    const archive = vi
      .spyOn(desktop, "archiveChat")
      .mockReturnValue(pendingArchive.promise);
    useWorkbenchStore.setState({
      tabs: [chatTab, chatTabB, fileTab],
      activeTabId: fileTab.id,
      selectedResource,
      activeEditor: {
        phase: "ready",
        resource: selectedResource,
        document: document(selectedResource, "before archive", 0, false),
        content: "latest unsnapshotted bytes",
        revision: 1,
        persistedRevision: 0,
        persisting: false,
        saving: false,
        message: null,
      },
    });

    const archiving = useWorkbenchStore.getState().archiveChat(chatTabB.id);
    await vi.waitFor(() => expect(archive).toHaveBeenCalledOnce());
    useWorkbenchStore
      .getState()
      .updateEditorContent("post-flush bytes must be rejected");
    expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
      phase: "ready",
      content: "latest unsnapshotted bytes",
      revision: 1,
      persistedRevision: 1,
    });
    pendingArchive.resolve({
      committed: true,
      transitionEpoch: 1,
      snapshot: chatProjection(
        [chatTab, fileTab],
        fileTab.id,
        1,
        null,
        [],
        "root-a",
        selectedResource,
        [{ id: chatTabB.id, title: chatTabB.title }],
      ),
      recoveryError: null,
    });

    expect(await archiving).toBe(true);
    expect(snapshot).toHaveBeenCalledWith(
      expect.objectContaining({
        resource: selectedResource,
        content: "latest unsnapshotted bytes",
        revision: 1,
      }),
    );
    expect(snapshot.mock.invocationCallOrder[0]).toBeLessThan(
      archive.mock.invocationCallOrder[0],
    );
    expect(useWorkbenchStore.getState()).toMatchObject({
      activeTabId: fileTab.id,
      archivedChats: [{ id: chatTabB.id, title: chatTabB.title }],
      projectTransitioning: false,
    });
  });

  it("restores and selects an archived Chat through the authoritative transition", async () => {
    const restoredTab = {
      ...chatTab,
      position: 2,
      pinned: false,
      status: "idle" as const,
    };
    const restore = vi.spyOn(desktop, "restoreChat").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: chatProjection(
        [chatTabB, restoredTab],
        restoredTab.id,
        1,
        "session-a",
        [],
      ),
      recoveryError: null,
    });
    useWorkbenchStore.setState({
      tabs: [chatTabB],
      activeTabId: chatTabB.id,
      archivedChats: [{ id: chatTab.id, title: chatTab.title }],
    });

    const committed = await useWorkbenchStore
      .getState()
      .restoreChat(chatTab.id);

    expect(committed).toBe(true);
    expect(restore).toHaveBeenCalledWith(
      {
        projectId: "project-a",
        chatId: chatTab.id,
        currentLayout: expect.objectContaining({
          tabs: [chatTabB],
          activeTabId: chatTabB.id,
        }),
      },
      0,
    );
    expect(useWorkbenchStore.getState()).toMatchObject({
      tabs: [chatTabB, restoredTab],
      activeTabId: restoredTab.id,
      archivedChats: [],
      projectTransitioning: false,
    });
  });

  it("preserves the detached Chat when restore is not committed", async () => {
    const restore = vi.spyOn(desktop, "restoreChat").mockResolvedValue({
      committed: false,
      transitionEpoch: 0,
      snapshot: null,
      recoveryError: "restore rejected",
    });
    useWorkbenchStore.setState({
      tabs: [chatTabB],
      activeTabId: chatTabB.id,
      archivedChats: [{ id: chatTab.id, title: chatTab.title }],
    });

    expect(await useWorkbenchStore.getState().restoreChat(chatTab.id)).toBe(
      false,
    );
    expect(restore).toHaveBeenCalledOnce();
    expect(useWorkbenchStore.getState()).toMatchObject({
      tabs: [chatTabB],
      activeTabId: chatTabB.id,
      archivedChats: [{ id: chatTab.id, title: chatTab.title }],
      projectTransitioning: false,
      error: "restore rejected",
    });
  });

  it("reloads the detached Chat authoritatively when restore throws", async () => {
    vi.spyOn(desktop, "restoreChat").mockRejectedValue(
      new Error("restore transport interrupted"),
    );
    const bootstrap = vi
      .spyOn(desktop, "getBootstrapSnapshot")
      .mockResolvedValue(
        chatProjection([chatTabB], chatTabB.id, 0, null, [], "root-a", null, [
          { id: chatTab.id, title: chatTab.title },
        ]),
      );
    useWorkbenchStore.setState({
      tabs: [chatTabB],
      activeTabId: chatTabB.id,
      archivedChats: [{ id: chatTab.id, title: chatTab.title }],
    });

    expect(await useWorkbenchStore.getState().restoreChat(chatTab.id)).toBe(
      false,
    );
    expect(bootstrap).toHaveBeenCalled();
    expect(useWorkbenchStore.getState()).toMatchObject({
      tabs: [chatTabB],
      activeTabId: chatTabB.id,
      archivedChats: [{ id: chatTab.id, title: chatTab.title }],
      projectTransitioning: false,
      error: "restore transport interrupted",
    });
  });

  it("adopts a newer divergent projection after stale restore completion", async () => {
    const restoredTab = {
      ...chatTab,
      position: 2,
      pinned: false,
      status: "idle" as const,
    };
    vi.spyOn(desktop, "restoreChat").mockResolvedValue({
      committed: false,
      transitionEpoch: 2,
      snapshot: null,
      recoveryError: "stale restore",
    });
    const bootstrap = vi
      .spyOn(desktop, "getBootstrapSnapshot")
      .mockResolvedValue(
        chatProjection([chatTabB, restoredTab], chatTabB.id, 2, null, []),
      );
    useWorkbenchStore.setState({
      tabs: [chatTabB],
      activeTabId: chatTabB.id,
      archivedChats: [{ id: chatTab.id, title: chatTab.title }],
    });

    expect(await useWorkbenchStore.getState().restoreChat(chatTab.id)).toBe(
      false,
    );
    expect(bootstrap).toHaveBeenCalled();
    expect(useWorkbenchStore.getState()).toMatchObject({
      nativeTransitionEpoch: 2,
      tabs: [chatTabB, restoredTab],
      activeTabId: chatTabB.id,
      archivedChats: [],
      projectTransitioning: false,
    });
  });

  it("blocks restore when the active File editor cannot be snapshotted", async () => {
    const selectedResource = resource("src/current.ts");
    const fileTab: WorkbenchTab = {
      id: "file:current",
      kind: "file",
      title: "current.ts",
      position: 2,
      pinned: false,
      dirty: true,
      status: "idle",
      resource: selectedResource,
    };
    vi.spyOn(desktop, "snapshotEditorBuffer").mockRejectedValue(
      new Error("snapshot unavailable"),
    );
    const restore = vi.spyOn(desktop, "restoreChat");
    useWorkbenchStore.setState({
      tabs: [chatTabB, fileTab],
      activeTabId: fileTab.id,
      selectedResource,
      archivedChats: [{ id: chatTab.id, title: chatTab.title }],
      activeEditor: {
        phase: "ready",
        resource: selectedResource,
        document: document(selectedResource, "before restore", 0, false),
        content: "must survive",
        revision: 1,
        persistedRevision: 0,
        persisting: false,
        saving: false,
        message: null,
      },
    });

    expect(await useWorkbenchStore.getState().restoreChat(chatTab.id)).toBe(
      false,
    );
    expect(restore).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState()).toMatchObject({
      projectTransitioning: false,
      error: expect.stringContaining("snapshot unavailable"),
      archivedChats: [{ id: chatTab.id, title: chatTab.title }],
      activeEditor: {
        phase: "ready",
        content: "must survive",
        revision: 1,
        persistedRevision: 0,
      },
    });
  });

  it("preserves Chat titles when rename is not committed", async () => {
    vi.spyOn(desktop, "renameChat").mockResolvedValue({
      committed: false,
      transitionEpoch: 0,
      snapshot: null,
      recoveryError: "rename rejected",
    });
    useWorkbenchStore.setState({ tabs: [chatTab, chatTabB] });

    const committed = await useWorkbenchStore
      .getState()
      .renameChat(chatTabB.id, "Focus");

    expect(committed).toBe(false);
    expect(useWorkbenchStore.getState()).toMatchObject({
      tabs: [chatTab, chatTabB],
      activeTabId: chatTab.id,
      error: "rename rejected",
      projectTransitioning: false,
    });
  });

  it("rejects a committed rename projection with mismatched selection", async () => {
    const renamedTab = { ...chatTabB, title: "Chat: Focus" };
    vi.spyOn(desktop, "renameChat").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: chatProjection(
        [chatTab, renamedTab],
        chatTab.id,
        1,
        "session-a",
        [],
        "root-b",
        resource("src/unexpected.ts", "root-b"),
      ),
      recoveryError: null,
    });
    useWorkbenchStore.setState({ tabs: [chatTab, chatTabB] });

    const committed = await useWorkbenchStore
      .getState()
      .renameChat(chatTabB.id, "Focus");

    expect(committed).toBe(false);
    expect(useWorkbenchStore.getState()).toMatchObject({
      phase: "error",
      error: expect.stringContaining("invalid projection"),
    });
  });

  it("reconciles ambiguous and stale rename completions authoritatively", async () => {
    const renamedTab = { ...chatTabB, title: "Chat: Recovered" };
    vi.spyOn(desktop, "renameChat")
      .mockRejectedValueOnce(new Error("transport interrupted"))
      .mockResolvedValueOnce({
        committed: false,
        transitionEpoch: 3,
        snapshot: null,
        recoveryError: "stale rename",
      });
    vi.spyOn(desktop, "getBootstrapSnapshot")
      .mockResolvedValueOnce(
        chatProjection([chatTab, renamedTab], chatTab.id, 1, "session-a", []),
      )
      .mockResolvedValueOnce(
        chatProjection(
          [chatTab, { ...chatTabB, title: "Chat: Newer" }],
          chatTab.id,
          3,
          "session-a",
          [],
        ),
      );
    useWorkbenchStore.setState({ tabs: [chatTab, chatTabB] });

    const recovered = await useWorkbenchStore
      .getState()
      .renameChat(chatTabB.id, "Recovered");
    expect(recovered).toBe(true);
    expect(useWorkbenchStore.getState().tabs).toEqual([chatTab, renamedTab]);

    const stale = await useWorkbenchStore
      .getState()
      .renameChat(chatTabB.id, "Stale");
    expect(stale).toBe(false);
    expect(useWorkbenchStore.getState()).toMatchObject({
      nativeTransitionEpoch: 3,
      tabs: [chatTab, { ...chatTabB, title: "Chat: Newer" }],
      activeTabId: chatTab.id,
      sessionId: "session-a",
    });
  });

  it("rejects invalid rename input before invoking the desktop", async () => {
    const rename = vi.spyOn(desktop, "renameChat");
    useWorkbenchStore.setState({ tabs: [chatTab, chatTabB] });

    const committed = await useWorkbenchStore
      .getState()
      .renameChat(chatTabB.id, " \t ");

    expect(committed).toBe(false);
    expect(rename).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState().error).toContain("not be empty");
  });

  it("does not project a Chat completion while a File tab is active", async () => {
    const pending = deferred<Awaited<ReturnType<typeof desktop.sendPrompt>>>();
    vi.spyOn(desktop, "sendPrompt").mockReturnValue(pending.promise);
    const fileTab: WorkbenchTab = {
      id: "file:a",
      kind: "file",
      title: "main.rs",
      position: 1,
      pinned: false,
      dirty: false,
      resource: resource("src/main.rs"),
    };
    useWorkbenchStore.setState({
      tabs: [chatTab, fileTab],
      selectedHarnessId: "sage-simulator",
      selectedModelId: "sim-fast",
      composerText: "continue",
    });

    const sending = useWorkbenchStore.getState().sendPrompt();
    useWorkbenchStore.setState({ activeTabId: fileTab.id });
    pending.resolve({
      sessionId: "session-new",
      messages: [
        {
          id: "message-new",
          sessionId: "session-new",
          sequence: 2,
          role: "assistant",
          text: "ack",
          createdAt: 2,
        },
      ],
    });
    await sending;

    expect(useWorkbenchStore.getState()).toMatchObject({
      activeTabId: fileTab.id,
      sessionId: "session-a",
      sending: false,
    });
  });

  it("does not let an archived Chat send overwrite the next Chat", async () => {
    const pending = deferred<Awaited<ReturnType<typeof desktop.sendPrompt>>>();
    vi.spyOn(desktop, "sendPrompt").mockReturnValue(pending.promise);
    vi.spyOn(desktop, "archiveChat").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: chatProjection(
        [chatTabB],
        chatTabB.id,
        1,
        null,
        [],
        "root-a",
        null,
        [{ id: chatTab.id, title: chatTab.title }],
      ),
      recoveryError: null,
    });
    useWorkbenchStore.setState({
      tabs: [chatTab, chatTabB],
      selectedHarnessId: "sage-simulator",
      selectedModelId: "sim-fast",
      composerText: "continue",
    });

    const sending = useWorkbenchStore.getState().sendPrompt();
    useWorkbenchStore.getState().closeTab(chatTab.id);
    await vi.waitFor(() =>
      expect(useWorkbenchStore.getState().activeTabId).toBe(chatTabB.id),
    );
    pending.resolve({ sessionId: "detached-session", messages: [] });
    await sending;

    expect(useWorkbenchStore.getState()).toMatchObject({
      tabs: [chatTabB],
      sending: false,
      sessionId: null,
      messages: [],
    });
  });

  it("never lets Chat A completion overwrite Chat B and reloads A authoritatively", async () => {
    const chatB: WorkbenchTab = {
      id: "chat:b",
      kind: "chat",
      title: "Chat: B",
      position: 1,
      pinned: false,
      dirty: false,
      status: "idle",
    };
    const message = (id: string, sessionId: string, text: string) => ({
      id,
      sessionId,
      sequence: 1,
      role: "assistant" as const,
      text,
      createdAt: 1,
    });
    const projection = (
      active: WorkbenchTab,
      transitionEpoch: number,
      messages: ChatMessage[],
      sessionId: string,
    ): BootstrapSnapshot => ({
      transitionEpoch,
      projects: useWorkbenchStore.getState().projects,
      project: useWorkbenchStore.getState().project,
      tabs: [chatTab, chatB],
      archivedChats: [],
      activeTabId: active.id,
      selectedRootId: "root-a",
      selectedResource: null,
      harnesses: [],
      sessionId,
      selectedHarnessId: null,
      selectedModelId: null,
      messages,
      repositories: { roots: [], truncated: false, elapsedMs: 0 },
      turnChanges: [],
      recoveredBuffers: [],
      recoveryNotice: null,
    });
    const pending = deferred<Awaited<ReturnType<typeof desktop.sendPrompt>>>();
    vi.spyOn(desktop, "sendPrompt").mockReturnValue(pending.promise);
    const chatBMessages = [message("b-message", "session-b", "B only")];
    const chatAMessages = [message("a-message", "session-a2", "A complete")];
    const activate = vi
      .spyOn(desktop, "activateChat")
      .mockResolvedValueOnce({
        committed: true,
        transitionEpoch: 1,
        snapshot: projection(chatB, 1, chatBMessages, "session-b"),
        recoveryError: null,
      })
      .mockResolvedValueOnce({
        committed: true,
        transitionEpoch: 2,
        snapshot: projection(chatTab, 2, chatAMessages, "session-a2"),
        recoveryError: null,
      });
    vi.spyOn(desktop, "listDirectory").mockImplementation(async (target) => ({
      resource: target,
      entries: [],
      truncated: false,
    }));
    useWorkbenchStore.setState({
      tabs: [chatTab, chatB],
      selectedHarnessId: "sage-simulator",
      selectedModelId: "sim-fast",
      composerText: "run A",
    });

    const sendingA = useWorkbenchStore.getState().sendPrompt();
    await useWorkbenchStore.getState().activateTab(chatB.id);
    pending.resolve({ sessionId: "session-a2", messages: chatAMessages });
    await sendingA;

    expect(activate).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({ activeTabId: chatB.id }),
      0,
    );
    expect(useWorkbenchStore.getState()).toMatchObject({
      activeTabId: chatB.id,
      sessionId: "session-b",
      messages: chatBMessages,
    });

    await useWorkbenchStore.getState().activateTab(chatTab.id);
    expect(activate).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({ activeTabId: chatTab.id }),
      1,
    );
    expect(useWorkbenchStore.getState()).toMatchObject({
      activeTabId: chatTab.id,
      sessionId: "session-a2",
      messages: chatAMessages,
    });
  });

  it("reloads the next Chat authoritatively when the active Chat closes", async () => {
    const chatB: WorkbenchTab = {
      id: "chat:b",
      kind: "chat",
      title: "Chat: B",
      position: 1,
      pinned: false,
      dirty: false,
      status: "idle",
    };
    const messages: ChatMessage[] = [
      {
        id: "b-message",
        sessionId: "session-b",
        sequence: 1,
        role: "assistant",
        text: "B only",
        createdAt: 1,
      },
    ];
    const archive = vi.spyOn(desktop, "archiveChat").mockResolvedValueOnce({
      committed: true,
      transitionEpoch: 1,
      snapshot: {
        transitionEpoch: 1,
        projects: useWorkbenchStore.getState().projects,
        project: useWorkbenchStore.getState().project,
        tabs: [chatB],
        archivedChats: [{ id: chatTab.id, title: chatTab.title }],
        activeTabId: chatB.id,
        selectedRootId: "root-a",
        selectedResource: null,
        harnesses: [],
        sessionId: "session-b",
        selectedHarnessId: null,
        selectedModelId: null,
        messages,
        repositories: { roots: [], truncated: false, elapsedMs: 0 },
        turnChanges: [],
        recoveredBuffers: [],
        recoveryNotice: null,
      },
      recoveryError: null,
    });
    useWorkbenchStore.setState({ tabs: [chatTab, chatB] });

    useWorkbenchStore.getState().closeTab(chatTab.id);

    await vi.waitFor(() => {
      expect(useWorkbenchStore.getState()).toMatchObject({
        activeTabId: chatB.id,
        sessionId: "session-b",
        messages,
        projectTransitioning: false,
      });
    });
    expect(archive).toHaveBeenCalledWith(
      {
        projectId: "project-a",
        chatId: chatTab.id,
        currentLayout: expect.objectContaining({
          tabs: [chatTab, chatB],
          activeTabId: chatTab.id,
        }),
      },
      0,
    );
  });

  it("preserves the active Chat when transactional close is not committed", async () => {
    const bootstrap = vi.spyOn(desktop, "getBootstrapSnapshot");
    const archive = vi.spyOn(desktop, "archiveChat").mockResolvedValue({
      committed: false,
      transitionEpoch: 0,
      snapshot: null,
      recoveryError: "close rejected",
    });
    useWorkbenchStore.setState({ tabs: [chatTab, chatTabB] });

    useWorkbenchStore.getState().closeTab(chatTab.id);

    await vi.waitFor(() => {
      expect(useWorkbenchStore.getState().projectTransitioning).toBe(false);
    });
    expect(archive).toHaveBeenCalledWith(
      {
        projectId: "project-a",
        chatId: chatTab.id,
        currentLayout: expect.objectContaining({
          tabs: [chatTab, chatTabB],
          activeTabId: chatTab.id,
        }),
      },
      0,
    );
    expect(bootstrap).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState()).toMatchObject({
      tabs: [chatTab, chatTabB],
      activeTabId: chatTab.id,
      sessionId: "session-a",
      messages: [],
      error: "close rejected",
    });
  });

  it("restores the authoritative Chat when transactional close throws", async () => {
    vi.spyOn(desktop, "archiveChat").mockRejectedValue(
      new Error("transport interrupted"),
    );
    const bootstrap = vi
      .spyOn(desktop, "getBootstrapSnapshot")
      .mockResolvedValue(
        chatProjection([chatTab, chatTabB], chatTab.id, 0, "session-a", []),
      );
    useWorkbenchStore.setState({ tabs: [chatTab, chatTabB] });

    useWorkbenchStore.getState().closeTab(chatTab.id);

    await vi.waitFor(() => {
      expect(useWorkbenchStore.getState().projectTransitioning).toBe(false);
    });
    expect(bootstrap).toHaveBeenCalled();
    expect(useWorkbenchStore.getState()).toMatchObject({
      tabs: [chatTab, chatTabB],
      activeTabId: chatTab.id,
      sessionId: "session-a",
      messages: [],
      error: "transport interrupted",
    });
  });

  it("adopts divergent authoritative state after a stale close completion", async () => {
    const messages: ChatMessage[] = [
      {
        id: "b-message",
        sessionId: "session-b",
        sequence: 1,
        role: "assistant",
        text: "B won",
        createdAt: 1,
      },
    ];
    vi.spyOn(desktop, "archiveChat").mockResolvedValue({
      committed: false,
      transitionEpoch: 2,
      snapshot: null,
      recoveryError: "stale close",
    });
    const bootstrap = vi
      .spyOn(desktop, "getBootstrapSnapshot")
      .mockResolvedValue(
        chatProjection(
          [chatTab, chatTabB],
          chatTabB.id,
          2,
          "session-b",
          messages,
        ),
      );
    useWorkbenchStore.setState({ tabs: [chatTab, chatTabB] });

    useWorkbenchStore.getState().closeTab(chatTab.id);

    await vi.waitFor(() => {
      expect(useWorkbenchStore.getState().projectTransitioning).toBe(false);
    });
    expect(bootstrap).toHaveBeenCalled();
    expect(useWorkbenchStore.getState()).toMatchObject({
      nativeTransitionEpoch: 2,
      tabs: [chatTab, chatTabB],
      activeTabId: chatTabB.id,
      sessionId: "session-b",
      messages,
    });
  });

  it("keeps the same relative path in two roots as distinct tabs and caches", async () => {
    const left = resource("src/main.rs", "root-a");
    const right = resource("src/main.rs", "root-b");
    await useWorkbenchStore.getState().openFile(left);
    await useWorkbenchStore.getState().openFile(right);

    const files = useWorkbenchStore
      .getState()
      .tabs.filter((tab) => tab.kind === "file");
    expect(files).toHaveLength(2);
    expect(files.map((tab) => tab.resource?.rootId)).toEqual([
      "root-a",
      "root-b",
    ]);
    expect(resourceKey(left)).not.toBe(resourceKey(right));
  });

  it("opens the exact root-qualified Search match without losing a dirty editor", async () => {
    const left = resource("src/main.rs", "root-a");
    const right = resource("src/main.rs", "root-b");
    const leftTab: WorkbenchTab = {
      id: "file:left-main",
      kind: "file",
      title: "main.rs",
      position: 1,
      pinned: false,
      dirty: true,
      resource: left,
    };
    const rightTab: WorkbenchTab = {
      ...leftTab,
      id: "file:right-main",
      position: 2,
      dirty: false,
      resource: right,
    };
    const snapshot = vi
      .spyOn(desktop, "snapshotEditorBuffer")
      .mockResolvedValue({ revision: 2, persistedAt: 1, dirty: true });
    vi.spyOn(desktop, "openEditorDocument").mockResolvedValue(
      document(right, "fn needle() {}\n", 0, false),
    );
    useWorkbenchStore.setState({
      tabs: [chatTab, leftTab, rightTab],
      activeTabId: leftTab.id,
      selectedRootId: left.rootId,
      selectedResource: left,
      activeEditor: {
        phase: "ready",
        resource: left,
        document: document(left, "saved\n", 0, false),
        content: "unsaved local change\n",
        revision: 2,
        persistedRevision: 1,
        persisting: false,
        saving: false,
        message: null,
      },
    });

    await useWorkbenchStore.getState().openSearchMatch({
      projectId: "project-a",
      transitionEpoch: 0,
      resource: right,
      lineNumber: 1,
      columnNumber: 4,
      query: "needle",
      caseSensitive: false,
    });

    expect(snapshot).toHaveBeenCalledWith(
      expect.objectContaining({
        resource: left,
        content: "unsaved local change\n",
        revision: 2,
      }),
    );
    expect(useWorkbenchStore.getState()).toMatchObject({
      activeTabId: rightTab.id,
      selectedRootId: "root-b",
      selectedResource: right,
      fileNavigationTarget: {
        resource: right,
        lineNumber: 1,
        columnNumber: 4,
        query: "needle",
        caseSensitive: false,
      },
    });
    expect(
      useWorkbenchStore.getState().tabs.find((tab) => tab.id === leftTab.id),
    ).toMatchObject({ dirty: true, resource: left });
  });

  it("retires Search navigation when its Project intent becomes stale", async () => {
    const target = resource("src/stale.ts");
    const pending = deferred<EditorDocument>();
    vi.spyOn(desktop, "openEditorDocument").mockReturnValue(pending.promise);

    const navigation = useWorkbenchStore.getState().openSearchMatch({
      projectId: "project-a",
      transitionEpoch: 0,
      resource: target,
      lineNumber: 2,
      columnNumber: 3,
      query: "needle",
      caseSensitive: true,
    });
    await vi.waitFor(() =>
      expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
        phase: "loading",
        resource: target,
      }),
    );

    useWorkbenchStore.setState({
      projectGeneration: 1,
      nativeTransitionEpoch: 1,
      fileNavigationTarget: null,
    });
    pending.resolve(document(target, "one\nneedle\n", 0, false));
    await navigation;

    expect(useWorkbenchStore.getState().fileNavigationTarget).toBeNull();
  });

  it("opens a changed resource as a diff and focuses Explorer", async () => {
    const target = resource("src/app/store.ts");
    await useWorkbenchStore.getState().openDiff(target);
    const state = useWorkbenchStore.getState();
    expect(state.tabs).toContainEqual(
      expect.objectContaining({ kind: "diff", resource: target }),
    );
    expect(state.selectedResource).toEqual(target);
    expect(state.sidebarOpen).toBe(true);
    expect(state.sidebarView).toBe("explorer");
  });

  it("never evicts dirty tabs at the resource-tab cap", async () => {
    const dirtyTabs: WorkbenchTab[] = Array.from(
      { length: 32 },
      (_, index) => ({
        id: `file:${index}`,
        kind: "file",
        title: `${index}.txt`,
        position: index + 1,
        pinned: false,
        dirty: true,
        resource: resource(`${index}.txt`),
      }),
    );
    useWorkbenchStore.setState({ tabs: [chatTab, ...dirtyTabs] });

    await useWorkbenchStore.getState().openFile(resource("overflow.txt"));

    expect(
      useWorkbenchStore.getState().tabs.filter((tab) => tab.kind === "file"),
    ).toHaveLength(32);
    expect(useWorkbenchStore.getState().error).toContain(
      "32 resource-tab limit",
    );
  });

  it("requires an explicit decision before closing a dirty tab", async () => {
    await useWorkbenchStore
      .getState()
      .openFile(resource("src-tauri/src/main.rs"));
    useWorkbenchStore.getState().updateEditorContent("dirty");
    const tabId = useWorkbenchStore.getState().activeTabId!;
    useWorkbenchStore.getState().closeTab(tabId);
    expect(useWorkbenchStore.getState().pendingCloseTabId).toBe(tabId);
    expect(
      useWorkbenchStore.getState().tabs.some((tab) => tab.id === tabId),
    ).toBe(true);
  });

  it("serializes overlapping snapshots and saves the latest revision", async () => {
    const target = resource("race.txt");
    const tab: WorkbenchTab = {
      id: "file:race",
      kind: "file",
      title: "race.txt",
      position: 1,
      pinned: false,
      dirty: false,
      resource: target,
    };
    const first = deferred<BufferSnapshotAck>();
    const requests: string[] = [];
    vi.spyOn(desktop, "snapshotEditorBuffer").mockImplementation(
      async (request) => {
        requests.push(request.content);
        return requests.length === 1
          ? first.promise
          : {
              revision: request.revision,
              persistedAt: 2,
              dirty: true,
            };
      },
    );
    vi.spyOn(desktop, "saveEditorDocument").mockResolvedValue({
      outcome: "saved",
      document: document(target, "latest\n", 2, false),
    });
    vi.spyOn(desktop, "getRepositoryStatus").mockResolvedValue({
      roots: [],
      truncated: false,
      elapsedMs: 0,
    });
    useWorkbenchStore.setState({
      tabs: [chatTab, tab],
      activeTabId: tab.id,
      activeEditor: {
        phase: "ready",
        resource: target,
        document: document(target, "base\n", 0, false),
        content: "base\n",
        revision: 0,
        persistedRevision: 0,
        persisting: false,
        saving: false,
        message: null,
      },
    });

    useWorkbenchStore.getState().updateEditorContent("first\n");
    const timerFlush = useWorkbenchStore.getState().flushActiveEditor();
    await vi.waitFor(() => expect(requests).toEqual(["first\n"]));
    useWorkbenchStore.getState().updateEditorContent("latest\n");
    const save = useWorkbenchStore.getState().saveActiveEditor();
    first.resolve({ revision: 1, persistedAt: 1, dirty: true });
    await Promise.all([timerFlush, save]);
    expect(requests).toEqual(["first\n", "latest\n"]);
  });

  it("blocks project switching when the newest snapshot fails", async () => {
    const target = resource("dirty.txt");
    vi.spyOn(desktop, "snapshotEditorBuffer").mockRejectedValue(
      new Error("disk unavailable"),
    );
    const switchNative = vi.spyOn(desktop, "switchProject");
    useWorkbenchStore.setState({
      tabs: [
        chatTab,
        {
          id: "file:dirty",
          kind: "file",
          title: "dirty.txt",
          position: 1,
          pinned: false,
          dirty: true,
          resource: target,
        },
      ],
      activeEditor: {
        phase: "ready",
        resource: target,
        document: document(target, "base", 0, false),
        content: "dirty",
        revision: 1,
        persistedRevision: 0,
        persisting: false,
        saving: false,
        message: null,
      },
    });

    await useWorkbenchStore.getState().switchProject("project-b");

    expect(switchNative).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState().project?.id).toBe("project-a");
  });

  it("clears prior project caches only after a successful switch", async () => {
    const oldRoot = resource("");
    useWorkbenchStore.setState({
      directoryListings: {
        [resourceKey(oldRoot)]: {
          phase: "ready",
          value: { resource: oldRoot, entries: [], truncated: false },
        },
      },
    });
    vi.spyOn(desktop, "switchProject").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: null,
      recoveryError: null,
    });
    vi.spyOn(desktop, "getBootstrapSnapshot").mockResolvedValue(snapshotB());
    vi.spyOn(desktop, "listDirectory").mockImplementation(async (target) => ({
      resource: target,
      entries: [],
      truncated: false,
    }));

    await useWorkbenchStore.getState().switchProject("project-b");

    const state = useWorkbenchStore.getState();
    expect(state.project?.id).toBe("project-b");
    expect(state.directoryListings[resourceKey(oldRoot)]).toBeUndefined();
    expect(
      state.directoryListings[resourceKey(resource("", "root-b"))],
    ).toEqual(expect.objectContaining({ phase: "ready" }));
  });

  it("reuses an existing File tab for the same resource", async () => {
    const target = resource("src/reused.ts");
    await useWorkbenchStore.getState().openFile(target);
    const firstId = useWorkbenchStore.getState().activeTabId;
    await useWorkbenchStore.getState().openFile(target);
    expect(useWorkbenchStore.getState().activeTabId).toBe(firstId);
    expect(
      useWorkbenchStore
        .getState()
        .tabs.filter(
          (tab) =>
            tab.kind === "file" &&
            tab.resource?.rootId === target.rootId &&
            tab.resource.relativePath === target.relativePath,
        ),
    ).toHaveLength(1);
  });

  it("reuses an existing Diff tab for the same resource", async () => {
    const target = resource("src/reused.ts");
    await useWorkbenchStore.getState().openDiff(target);
    const firstId = useWorkbenchStore.getState().activeTabId;
    await useWorkbenchStore.getState().openDiff(target);
    expect(useWorkbenchStore.getState().activeTabId).toBe(firstId);
    expect(
      useWorkbenchStore
        .getState()
        .tabs.filter(
          (tab) =>
            tab.kind === "diff" &&
            tab.resource?.rootId === target.rootId &&
            tab.resource.relativePath === target.relativePath,
        ),
    ).toHaveLength(1);
  });

  it("flushes the latest dirty editor revision before activating another tab", async () => {
    const target = resource("switch.txt");
    const fileTab: WorkbenchTab = {
      id: "file:switch",
      kind: "file",
      title: "switch.txt",
      position: 1,
      pinned: false,
      dirty: true,
      resource: target,
    };
    const pending = deferred<BufferSnapshotAck>();
    const snapshot = vi
      .spyOn(desktop, "snapshotEditorBuffer")
      .mockReturnValue(pending.promise);
    useWorkbenchStore.setState({
      tabs: [chatTab, fileTab],
      activeTabId: fileTab.id,
      activeEditor: {
        phase: "ready",
        resource: target,
        document: document(target, "base", 0, false),
        content: "dirty",
        revision: 1,
        persistedRevision: 0,
        persisting: false,
        saving: false,
        message: null,
      },
    });

    const activation = useWorkbenchStore.getState().activateTab(chatTab.id);
    await vi.waitFor(() => expect(snapshot).toHaveBeenCalledOnce());
    expect(useWorkbenchStore.getState().activeTabId).toBe(fileTab.id);
    pending.resolve({ revision: 1, persistedAt: 1, dirty: true });
    await activation;
    expect(useWorkbenchStore.getState().activeTabId).toBe(chatTab.id);
  });

  it("refreshes both Explorer and Source Control after a successful save", async () => {
    const target = resource("saved.txt");
    const fileTab: WorkbenchTab = {
      id: "file:saved",
      kind: "file",
      title: "saved.txt",
      position: 1,
      pinned: false,
      dirty: true,
      resource: target,
    };
    vi.spyOn(desktop, "snapshotEditorBuffer").mockResolvedValue({
      revision: 1,
      persistedAt: 1,
      dirty: true,
    });
    vi.spyOn(desktop, "saveEditorDocument").mockResolvedValue({
      outcome: "saved",
      document: document(target, "saved", 1, false),
    });
    const repository = vi
      .spyOn(desktop, "getRepositoryStatus")
      .mockResolvedValue({ roots: [], truncated: false, elapsedMs: 1 });
    const directory = vi.spyOn(desktop, "listDirectory").mockResolvedValue({
      resource: resource(""),
      entries: [],
      truncated: false,
    });
    useWorkbenchStore.setState({
      tabs: [chatTab, fileTab],
      activeTabId: fileTab.id,
      activeEditor: {
        phase: "ready",
        resource: target,
        document: document(target, "base", 0, false),
        content: "saved",
        revision: 1,
        persistedRevision: 0,
        persisting: false,
        saving: false,
        message: null,
      },
    });

    expect(await useWorkbenchStore.getState().saveActiveEditor()).toBe(true);
    expect(repository).toHaveBeenCalledOnce();
    expect(directory).toHaveBeenCalledWith(resource(""));
  });

  it.each(["success", "error"] as const)(
    "ignores a late old-project directory %s after switching",
    async (outcome) => {
      const target = resource("src");
      const pending = deferred<DirectoryListing>();
      vi.spyOn(desktop, "listDirectory").mockImplementation((requested) =>
        requested.rootId === "root-a"
          ? pending.promise
          : Promise.resolve({
              resource: requested,
              entries: [],
              truncated: false,
            }),
      );
      vi.spyOn(desktop, "switchProject").mockResolvedValue({
        committed: true,
        transitionEpoch: 1,
        snapshot: null,
        recoveryError: null,
      });
      vi.spyOn(desktop, "getBootstrapSnapshot").mockResolvedValue(snapshotB());

      const oldLoad = useWorkbenchStore.getState().loadDirectory(target);
      await vi.waitFor(() =>
        expect(
          useWorkbenchStore.getState().directoryListings[resourceKey(target)],
        ).toEqual({ phase: "loading" }),
      );
      await useWorkbenchStore.getState().switchProject("project-b");
      if (outcome === "success") {
        pending.resolve({ resource: target, entries: [], truncated: false });
      } else {
        pending.reject(new Error("old directory failed"));
      }
      await oldLoad;

      const state = useWorkbenchStore.getState();
      expect(state.project?.id).toBe("project-b");
      expect(state.directoryListings[resourceKey(target)]).toBeUndefined();
      expect(state.error).not.toBe("old directory failed");
    },
  );

  it.each(["success", "error"] as const)(
    "ignores a late old-project diff %s after switching",
    async (outcome) => {
      const target = resource("old.ts");
      const diffTab: WorkbenchTab = {
        id: "diff:old",
        kind: "diff",
        title: "old.ts",
        position: 1,
        pinned: false,
        dirty: false,
        resource: target,
      };
      const pending = deferred<FileDiff>();
      vi.spyOn(desktop, "getFileDiff").mockReturnValue(pending.promise);
      vi.spyOn(desktop, "listDirectory").mockResolvedValue({
        resource: resource("", "root-b"),
        entries: [],
        truncated: false,
      });
      vi.spyOn(desktop, "switchProject").mockResolvedValue({
        committed: true,
        transitionEpoch: 1,
        snapshot: null,
        recoveryError: null,
      });
      vi.spyOn(desktop, "getBootstrapSnapshot").mockResolvedValue(snapshotB());
      useWorkbenchStore.setState({
        tabs: [chatTab, diffTab],
        activeTabId: diffTab.id,
      });

      const oldLoad = useWorkbenchStore.getState().loadDiff(target);
      await vi.waitFor(() =>
        expect(useWorkbenchStore.getState().diffs[resourceKey(target)]).toEqual(
          { phase: "loading" },
        ),
      );
      await useWorkbenchStore.getState().switchProject("project-b");
      if (outcome === "success") {
        pending.resolve({
          resource: target,
          hunks: [],
          binary: false,
          truncated: false,
        });
      } else {
        pending.reject(new Error("old diff failed"));
      }
      await oldLoad;

      const state = useWorkbenchStore.getState();
      expect(state.project?.id).toBe("project-b");
      expect(state.diffs[resourceKey(target)]).toBeUndefined();
      expect(state.error).not.toBe("old diff failed");
    },
  );

  it.each(["success", "error"] as const)(
    "ignores a late old-project editor %s after switching",
    async (outcome) => {
      const target = resource("old.txt");
      const fileTab: WorkbenchTab = {
        id: "file:old",
        kind: "file",
        title: "old.txt",
        position: 1,
        pinned: false,
        dirty: false,
        resource: target,
      };
      const pending = deferred<EditorDocument>();
      vi.spyOn(desktop, "openEditorDocument").mockReturnValue(pending.promise);
      vi.spyOn(desktop, "listDirectory").mockResolvedValue({
        resource: resource("", "root-b"),
        entries: [],
        truncated: false,
      });
      vi.spyOn(desktop, "switchProject").mockResolvedValue({
        committed: true,
        transitionEpoch: 1,
        snapshot: null,
        recoveryError: null,
      });
      vi.spyOn(desktop, "getBootstrapSnapshot").mockResolvedValue(snapshotB());
      useWorkbenchStore.setState({
        tabs: [chatTab, fileTab],
        activeTabId: fileTab.id,
      });

      const oldLoad = useWorkbenchStore.getState().loadEditor(target);
      await vi.waitFor(() =>
        expect(useWorkbenchStore.getState().activeEditor).toEqual({
          phase: "loading",
          resource: target,
        }),
      );
      await useWorkbenchStore.getState().switchProject("project-b");
      if (outcome === "success") {
        pending.resolve(document(target, "old", 1, false));
      } else {
        pending.reject(new Error("old editor failed"));
      }
      await oldLoad;

      const state = useWorkbenchStore.getState();
      expect(state.project?.id).toBe("project-b");
      expect(state.activeEditor).toBeNull();
      expect(state.error).not.toBe("old editor failed");
    },
  );

  it.each(["success", "error"] as const)(
    "ignores an old repository refresh %s after switching",
    async (outcome) => {
      const pending = deferred<ProjectRepositoryStatus>();
      vi.spyOn(desktop, "getRepositoryStatus").mockReturnValue(pending.promise);
      vi.spyOn(desktop, "listDirectory").mockResolvedValue({
        resource: resource("", "root-b"),
        entries: [],
        truncated: false,
      });
      vi.spyOn(desktop, "switchProject").mockResolvedValue({
        committed: true,
        transitionEpoch: 1,
        snapshot: null,
        recoveryError: null,
      });
      vi.spyOn(desktop, "getBootstrapSnapshot").mockResolvedValue(snapshotB());

      const oldRefresh = useWorkbenchStore.getState().refreshRepository();
      await vi.waitFor(() =>
        expect(useWorkbenchStore.getState().repositoryRefreshing).toBe(true),
      );
      await useWorkbenchStore.getState().switchProject("project-b");
      if (outcome === "success") {
        pending.resolve({
          roots: [
            {
              rootId: "root-a",
              state: "repository",
              branch: "old",
              changes: [],
              truncated: false,
              elapsedMs: 1,
              message: null,
            },
          ],
          truncated: false,
          elapsedMs: 1,
        });
      } else {
        pending.reject(new Error("old repository failed"));
      }
      await oldRefresh;

      const state = useWorkbenchStore.getState();
      expect(state.project?.id).toBe("project-b");
      expect(state.repositories).toEqual(snapshotB().repositories);
      expect(state.repositoryRefreshing).toBe(false);
      expect(state.error).not.toBe("old repository failed");
    },
  );

  it("stages only the exact root-qualified Source Control change and adopts native status", async () => {
    const samePathA = resource("same.txt", "root-a");
    const samePathB = resource("same.txt", "root-b");
    const before: ProjectRepositoryStatus = {
      roots: [
        {
          rootId: "root-a",
          state: "repository",
          branch: "main",
          changes: [
            {
              resource: samePathA,
              indexStatus: "unchanged",
              worktreeStatus: "modified",
              additions: 1,
              deletions: 1,
            },
          ],
          truncated: false,
          elapsedMs: 1,
          message: null,
        },
        {
          rootId: "root-b",
          state: "repository",
          branch: "main",
          changes: [
            {
              resource: samePathB,
              indexStatus: "unchanged",
              worktreeStatus: "modified",
              additions: 2,
              deletions: 0,
            },
          ],
          truncated: false,
          elapsedMs: 1,
          message: null,
        },
      ],
      truncated: false,
      elapsedMs: 2,
    };
    const after: ProjectRepositoryStatus = {
      ...before,
      roots: [
        {
          ...before.roots[0],
          changes: [
            {
              ...before.roots[0].changes[0],
              indexStatus: "modified",
              worktreeStatus: "unchanged",
            },
          ],
        },
        before.roots[1],
      ],
    };
    const mutate = vi.spyOn(desktop, "mutateGitFile").mockResolvedValue({
      projectId: "project-a",
      transitionEpoch: 0,
      repositories: after,
    });
    useWorkbenchStore.setState({ repositories: before });

    await expect(
      useWorkbenchStore.getState().mutateRepositoryFile(samePathA, "stage"),
    ).resolves.toBe(true);

    expect(mutate).toHaveBeenCalledWith(
      {
        projectId: "project-a",
        resource: samePathA,
        action: "stage",
      },
      0,
    );
    expect(useWorkbenchStore.getState()).toMatchObject({
      repositories: after,
      repositoryMutation: null,
      error: null,
    });
  });

  it("preserves the prior Source Control projection and reports a Git mutation failure", async () => {
    const target = resource("failed.txt");
    const before: ProjectRepositoryStatus = {
      roots: [
        {
          rootId: "root-a",
          state: "repository",
          branch: "main",
          changes: [
            {
              resource: target,
              indexStatus: "unchanged",
              worktreeStatus: "modified",
              additions: 1,
              deletions: 0,
            },
          ],
          truncated: false,
          elapsedMs: 1,
          message: null,
        },
      ],
      truncated: false,
      elapsedMs: 1,
    };
    vi.spyOn(desktop, "mutateGitFile").mockRejectedValue(
      new Error("Git index is locked"),
    );
    useWorkbenchStore.setState({ repositories: before });

    await expect(
      useWorkbenchStore.getState().mutateRepositoryFile(target, "stage"),
    ).resolves.toBe(false);

    expect(useWorkbenchStore.getState().repositories).toBe(before);
    expect(useWorkbenchStore.getState()).toMatchObject({
      repositoryMutation: null,
      error: "Git index is locked",
    });
  });

  it("retires a Git mutation completion after its Project intent becomes stale", async () => {
    const target = resource("stale.txt");
    const before: ProjectRepositoryStatus = {
      roots: [
        {
          rootId: "root-a",
          state: "repository",
          branch: "main",
          changes: [
            {
              resource: target,
              indexStatus: "unchanged",
              worktreeStatus: "modified",
              additions: 1,
              deletions: 0,
            },
          ],
          truncated: false,
          elapsedMs: 1,
          message: null,
        },
      ],
      truncated: false,
      elapsedMs: 1,
    };
    const pending =
      deferred<Awaited<ReturnType<typeof desktop.mutateGitFile>>>();
    vi.spyOn(desktop, "mutateGitFile").mockReturnValue(pending.promise);
    useWorkbenchStore.setState({ repositories: before });

    const mutation = useWorkbenchStore
      .getState()
      .mutateRepositoryFile(target, "stage");
    await vi.waitFor(() =>
      expect(useWorkbenchStore.getState().repositoryMutation).toMatchObject({
        resource: target,
        action: "stage",
      }),
    );
    const next = snapshotB().repositories;
    useWorkbenchStore.setState({
      projectGeneration: 1,
      nativeTransitionEpoch: 1,
      repositories: next,
      repositoryMutation: null,
    });
    pending.resolve({
      projectId: "project-a",
      transitionEpoch: 0,
      repositories: before,
    });

    await expect(mutation).resolves.toBe(false);
    expect(useWorkbenchStore.getState().repositories).toBe(next);
    expect(useWorkbenchStore.getState().error).toBeNull();
  });

  it("keeps the newest harness/model intent when an older load finishes last", async () => {
    const oldModels = deferred<ModelOption[]>();
    vi.spyOn(desktop, "listHarnessModels").mockImplementation((harnessId) =>
      harnessId === "old"
        ? oldModels.promise
        : Promise.resolve([{ id: "new-model", name: "New" }]),
    );

    const oldSelection = useWorkbenchStore.getState().selectHarness("old");
    await useWorkbenchStore.getState().selectHarness("new");
    oldModels.resolve([{ id: "old-model", name: "Old" }]);
    await oldSelection;

    const state = useWorkbenchStore.getState();
    expect(state.selectedHarnessId).toBe("new");
    expect(state.models).toEqual([{ id: "new-model", name: "New" }]);
    expect(state.selectedModelId).toBe("new-model");
  });

  it.each(["success", "error"] as const)(
    "ignores late harness/model %s after switching projects",
    async (outcome) => {
      const pending = deferred<ModelOption[]>();
      vi.spyOn(desktop, "listHarnessModels").mockReturnValue(pending.promise);
      vi.spyOn(desktop, "switchProject").mockResolvedValue({
        committed: true,
        transitionEpoch: 1,
        snapshot: snapshotB(),
        recoveryError: null,
      });
      vi.spyOn(desktop, "listDirectory").mockResolvedValue({
        resource: resource("", "root-b"),
        entries: [],
        truncated: false,
      });

      const oldSelection = useWorkbenchStore.getState().selectHarness("old");
      await vi.waitFor(() =>
        expect(useWorkbenchStore.getState().selectedHarnessId).toBe("old"),
      );
      await useWorkbenchStore.getState().switchProject("project-b");
      if (outcome === "success") {
        pending.resolve([{ id: "stale", name: "Stale" }]);
      } else {
        pending.reject(new Error("old harness failed"));
      }
      await oldSelection;

      const state = useWorkbenchStore.getState();
      expect(state.project?.id).toBe("project-b");
      expect(state.models).toEqual([]);
      expect(state.error).not.toBe("old harness failed");
    },
  );

  it("ignores a rejected same-harness ABA completion", async () => {
    const old = deferred<ModelOption[]>();
    let oldCalls = 0;
    vi.spyOn(desktop, "listHarnessModels").mockImplementation((harnessId) => {
      if (harnessId === "old" && oldCalls++ === 0) return old.promise;
      return Promise.resolve([{ id: `${harnessId}-new`, name: "New" }]);
    });
    const first = useWorkbenchStore.getState().selectHarness("old");
    await useWorkbenchStore.getState().selectHarness("new");
    await useWorkbenchStore.getState().selectHarness("old");
    old.reject(new Error("stale ABA failure"));
    await first;
    expect(useWorkbenchStore.getState().selectedHarnessId).toBe("old");
    expect(useWorkbenchStore.getState().selectedModelId).toBe("old-new");
    expect(useWorkbenchStore.getState().error).not.toBe("stale ABA failure");
  });

  it("restores persisted harness models during initialization", async () => {
    const restored = snapshotB();
    restored.selectedHarnessId = "saved-harness";
    restored.selectedModelId = "saved-model";
    vi.spyOn(desktop, "getBootstrapSnapshot").mockResolvedValue(restored);
    vi.spyOn(desktop, "listDirectory").mockImplementation(async (target) => ({
      resource: target,
      entries: [],
      truncated: false,
    }));
    const models = vi.spyOn(desktop, "listHarnessModels").mockResolvedValue([
      { id: "fallback-model", name: "Fallback" },
      { id: "saved-model", name: "Saved" },
    ]);

    await useWorkbenchStore.getState().initialize();

    expect(models).toHaveBeenCalledWith("saved-harness");
    expect(useWorkbenchStore.getState()).toEqual(
      expect.objectContaining({
        projectTransitioning: false,
        selectedHarnessId: "saved-harness",
        selectedModelId: "saved-model",
      }),
    );
  });

  it("restores persisted harness models inside a project switch", async () => {
    const restored = snapshotB();
    restored.selectedHarnessId = "saved-harness";
    restored.selectedModelId = "saved-model";
    vi.spyOn(desktop, "switchProject").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: restored,
      recoveryError: null,
    });
    vi.spyOn(desktop, "listDirectory").mockImplementation(async (target) => ({
      resource: target,
      entries: [],
      truncated: false,
    }));
    const pending = deferred<ModelOption[]>();
    vi.spyOn(desktop, "listHarnessModels").mockReturnValue(pending.promise);

    const switching = useWorkbenchStore.getState().switchProject("project-b");
    await vi.waitFor(() =>
      expect(useWorkbenchStore.getState()).toEqual(
        expect.objectContaining({
          projectTransitioning: true,
          selectedHarnessId: "saved-harness",
          selectedModelId: null,
        }),
      ),
    );
    pending.resolve([{ id: "saved-model", name: "Saved" }]);
    await switching;

    expect(useWorkbenchStore.getState()).toEqual(
      expect.objectContaining({
        projectTransitioning: false,
        selectedHarnessId: "saved-harness",
        selectedModelId: "saved-model",
      }),
    );
  });

  it("blocks the workbench if projection recovery fails after a committed switch", async () => {
    vi.spyOn(desktop, "switchProject").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: null,
      recoveryError: null,
    });
    const bootstrap = vi
      .spyOn(desktop, "getBootstrapSnapshot")
      .mockRejectedValue(new Error("bootstrap unavailable"));

    await useWorkbenchStore.getState().switchProject("project-b");

    const state = useWorkbenchStore.getState();
    expect(bootstrap).toHaveBeenCalledTimes(2);
    expect(state.phase).toBe("error");
    expect(state.error).toContain("project change committed");
    expect(state.projectTransitioning).toBe(false);
  });

  it("blocks the workbench if projection recovery fails after project creation", async () => {
    vi.spyOn(desktop, "createProjectFromPicker").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: null,
      recoveryError: null,
    });
    const bootstrap = vi
      .spyOn(desktop, "getBootstrapSnapshot")
      .mockRejectedValue(new Error("bootstrap unavailable"));

    await useWorkbenchStore.getState().createProject();

    const state = useWorkbenchStore.getState();
    expect(bootstrap).toHaveBeenCalledTimes(2);
    expect(state.phase).toBe("error");
    expect(state.error).toContain("project change committed");
  });

  it("blocks the workbench if projection recovery fails after adding a root", async () => {
    vi.spyOn(desktop, "addProjectRootFromPicker").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: null,
      recoveryError: null,
    });
    const bootstrap = vi
      .spyOn(desktop, "getBootstrapSnapshot")
      .mockRejectedValue(new Error("bootstrap unavailable"));

    await useWorkbenchStore.getState().addProjectRoot();

    const state = useWorkbenchStore.getState();
    expect(bootstrap).toHaveBeenCalledTimes(2);
    expect(state.phase).toBe("error");
    expect(state.error).toContain("project change committed");
  });

  it("uses an authoritative snapshot returned by the switch command without a second bootstrap", async () => {
    vi.spyOn(desktop, "switchProject").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: snapshotB(),
      recoveryError: null,
    });
    const bootstrap = vi.spyOn(desktop, "getBootstrapSnapshot");
    vi.spyOn(desktop, "listDirectory").mockResolvedValue({
      resource: resource("", "root-b"),
      entries: [],
      truncated: false,
    });

    await useWorkbenchStore.getState().switchProject("project-b");

    expect(useWorkbenchStore.getState().project?.id).toBe("project-b");
    expect(bootstrap).not.toHaveBeenCalled();
  });

  it.each(["switch", "create", "addRoot"] as const)(
    "authoritatively recovers an unknown %s commit outcome",
    async (operation) => {
      const recovered = snapshotB();
      if (operation === "addRoot") {
        recovered.project = {
          id: "project-a",
          name: "A",
          roots: [
            ...useWorkbenchStore.getState().project!.roots,
            {
              id: "root-new",
              name: "New",
              ordinal: 2,
              availability: "available",
              message: null,
            },
          ],
        };
        recovered.tabs = [chatTab];
        recovered.activeTabId = chatTab.id;
        recovered.selectedRootId = "root-a";
        recovered.sessionId = "session-a";
      }
      vi.spyOn(desktop, "getBootstrapSnapshot").mockResolvedValue(recovered);
      vi.spyOn(desktop, "listDirectory").mockImplementation(async (target) => ({
        resource: target,
        entries: [],
        truncated: false,
      }));
      if (operation === "switch") {
        vi.spyOn(desktop, "switchProject").mockRejectedValue(
          new Error("response lost"),
        );
        await useWorkbenchStore.getState().switchProject("project-b");
      } else if (operation === "create") {
        vi.spyOn(desktop, "createProjectFromPicker").mockRejectedValue(
          new Error("response lost"),
        );
        await useWorkbenchStore.getState().createProject();
      } else {
        vi.spyOn(desktop, "addProjectRootFromPicker").mockRejectedValue(
          new Error("response lost"),
        );
        await useWorkbenchStore.getState().addProjectRoot();
      }

      expect(useWorkbenchStore.getState().phase).toBe("ready");
      expect(useWorkbenchStore.getState().project?.id).toBe(
        operation === "addRoot" ? "project-a" : "project-b",
      );
      if (operation === "addRoot") {
        expect(useWorkbenchStore.getState().project?.roots).toHaveLength(3);
      }
    },
  );

  it("rejects old project-scoped actions while a transition flush is pending", async () => {
    const target = resource("old-action.txt");
    const pending = deferred<BufferSnapshotAck>();
    vi.spyOn(desktop, "snapshotEditorBuffer").mockReturnValue(pending.promise);
    const open = vi.spyOn(desktop, "openEditorDocument");
    vi.spyOn(desktop, "switchProject").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: snapshotB(),
      recoveryError: null,
    });
    vi.spyOn(desktop, "listDirectory").mockResolvedValue({
      resource: resource("", "root-b"),
      entries: [],
      truncated: false,
    });
    useWorkbenchStore.setState({
      tabs: [
        chatTab,
        {
          id: "file:dirty-transition",
          kind: "file",
          title: "dirty.txt",
          position: 1,
          pinned: false,
          dirty: true,
          resource: target,
        },
      ],
      activeTabId: "file:dirty-transition",
      activeEditor: {
        phase: "ready",
        resource: target,
        document: document(target, "base", 0, false),
        content: "dirty",
        revision: 1,
        persistedRevision: 0,
        persisting: false,
        saving: false,
        message: null,
      },
    });

    const switching = useWorkbenchStore.getState().switchProject("project-b");
    await vi.waitFor(() =>
      expect(useWorkbenchStore.getState().projectTransitioning).toBe(true),
    );
    await useWorkbenchStore.getState().openFile(resource("another-old.txt"));
    expect(open).not.toHaveBeenCalled();
    pending.resolve({ revision: 1, persistedAt: 1, dirty: true });
    await switching;
    expect(useWorkbenchStore.getState().project?.id).toBe("project-b");
  });

  it("keeps a newer directory request when an older same-resource request finishes last", async () => {
    const target = resource("");
    const old = deferred<DirectoryListing>();
    let calls = 0;
    vi.spyOn(desktop, "listDirectory").mockImplementation(async (requested) => {
      calls += 1;
      if (calls === 1) return old.promise;
      return {
        resource: requested,
        entries: [
          {
            name: "new.txt",
            resource: resource("new.txt"),
            kind: "file",
            byteLen: 1,
          },
        ],
        truncated: false,
      };
    });
    const first = useWorkbenchStore.getState().loadDirectory(target);
    await vi.waitFor(() => expect(calls).toBe(1));
    useWorkbenchStore.setState({ directoryListings: {} });
    await useWorkbenchStore.getState().loadDirectory(target);
    old.resolve({ resource: target, entries: [], truncated: false });
    await first;
    const listing =
      useWorkbenchStore.getState().directoryListings[resourceKey(target)];
    expect(listing).toEqual(
      expect.objectContaining({
        phase: "ready",
        value: expect.objectContaining({
          entries: [expect.objectContaining({ name: "new.txt" })],
        }),
      }),
    );
  });

  it("bounds every ancestor inserted by Explorer reveal", async () => {
    vi.spyOn(desktop, "listDirectory").mockImplementation(async (target) => ({
      resource: target,
      entries: [],
      truncated: false,
    }));
    const path = `${Array.from({ length: 80 }, (_, index) => `d${index}`).join("/")}/file.txt`;
    await useWorkbenchStore.getState().revealFile(resource(path));
    expect(
      Object.keys(useWorkbenchStore.getState().expandedDirectories),
    ).toHaveLength(64);
  });

  it.each(["editor", "diff"] as const)(
    "keeps a newer same-resource %s selection when the older request finishes last",
    async (kind) => {
      const target = resource("aba.txt");
      const tab: WorkbenchTab = {
        id: `${kind}:${resourceKey(target)}`,
        kind: kind === "editor" ? "file" : "diff",
        title: "aba.txt",
        position: 1,
        pinned: false,
        dirty: false,
        resource: target,
      };
      useWorkbenchStore.setState({ tabs: [chatTab, tab], activeTabId: tab.id });
      if (kind === "editor") {
        const old = deferred<EditorDocument>();
        let calls = 0;
        vi.spyOn(desktop, "openEditorDocument").mockImplementation(() => {
          calls += 1;
          return calls === 1
            ? old.promise
            : Promise.resolve(document(target, "new", 2, false));
        });
        const first = useWorkbenchStore.getState().loadEditor(target);
        await vi.waitFor(() => expect(calls).toBe(1));
        await useWorkbenchStore.getState().activateTab(chatTab.id);
        await useWorkbenchStore.getState().activateTab(tab.id);
        old.resolve(document(target, "old", 1, false));
        await first;
        const editor = useWorkbenchStore.getState().activeEditor;
        expect(editor?.phase).toBe("ready");
        if (editor?.phase === "ready") expect(editor.content).toBe("new");
      } else {
        const old = deferred<FileDiff>();
        let calls = 0;
        vi.spyOn(desktop, "getFileDiff").mockImplementation(() => {
          calls += 1;
          return calls === 1
            ? old.promise
            : Promise.resolve({
                resource: target,
                hunks: [{ header: "new", lines: [] }],
                binary: false,
                truncated: false,
              });
        });
        const first = useWorkbenchStore.getState().loadDiff(target);
        await vi.waitFor(() => expect(calls).toBe(1));
        await useWorkbenchStore.getState().activateTab(chatTab.id);
        await useWorkbenchStore.getState().activateTab(tab.id);
        await vi.waitFor(() =>
          expect(
            useWorkbenchStore.getState().diffs[resourceKey(target)],
          ).toEqual(expect.objectContaining({ phase: "ready" })),
        );
        old.resolve({
          resource: target,
          hunks: [],
          binary: false,
          truncated: false,
        });
        await first;
        const diff = useWorkbenchStore.getState().diffs[resourceKey(target)];
        expect(diff).toEqual(
          expect.objectContaining({
            phase: "ready",
            value: expect.objectContaining({
              hunks: [expect.objectContaining({ header: "new" })],
            }),
          }),
        );
      }
    },
  );

  it("prevents harness ABA completions and clears pending prompt state on switch", async () => {
    const firstOld = deferred<ModelOption[]>();
    let oldCalls = 0;
    vi.spyOn(desktop, "listHarnessModels").mockImplementation((harnessId) => {
      if (harnessId === "old" && oldCalls++ === 0) return firstOld.promise;
      return Promise.resolve([{ id: `${harnessId}-latest`, name: "Latest" }]);
    });
    const first = useWorkbenchStore.getState().selectHarness("old");
    await useWorkbenchStore.getState().selectHarness("new");
    await useWorkbenchStore.getState().selectHarness("old");
    firstOld.resolve([{ id: "old-stale", name: "Stale" }]);
    await first;
    expect(useWorkbenchStore.getState().models).toEqual([
      { id: "old-latest", name: "Latest" },
    ]);

    useWorkbenchStore.setState({ sending: true, composerText: "old draft" });
    vi.spyOn(desktop, "switchProject").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: snapshotB(),
      recoveryError: null,
    });
    vi.spyOn(desktop, "listDirectory").mockResolvedValue({
      resource: resource("", "root-b"),
      entries: [],
      truncated: false,
    });
    await useWorkbenchStore.getState().switchProject("project-b");
    expect(useWorkbenchStore.getState().sending).toBe(false);
    expect(useWorkbenchStore.getState().composerText).toBe("");
    expect(useWorkbenchStore.getState().models).toEqual([]);
  });
});
