import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { BootstrapSnapshot, WorkbenchTab } from "../domain/workbench";
import {
  MAX_ARCHIVED_CHAT_OWNERSHIP_ENTRIES,
  MAX_ATTACHED_CHAT_OWNERSHIP_ENTRIES,
  MAX_CHAT_COMPOSER_BYTES,
  MAX_CHAT_ERROR_BYTES,
  MAX_CHAT_OWNERSHIP_ENTRIES,
} from "../domain/chatOwnership";
import { desktop } from "../services/desktop";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { useWorkbenchStore } from "./store";

const projectPort = getWorkbenchPorts().project;

const chatA: WorkbenchTab = {
  id: "chat-a",
  kind: "chat",
  title: "Chat: A",
  position: 0,
  pinned: true,
  dirty: false,
  status: "idle",
};
const chatB: WorkbenchTab = {
  id: "chat-b",
  kind: "chat",
  title: "Chat: B",
  position: 1,
  pinned: false,
  dirty: false,
  status: "idle",
};

const initialState = useWorkbenchStore.getState();

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((accept, decline) => {
    resolve = accept;
    reject = decline;
  });
  return { promise, resolve, reject };
}

function projection(
  tabs: WorkbenchTab[],
  activeTabId: string,
  transitionEpoch: number,
  archivedChats: BootstrapSnapshot["archivedChats"] = [],
): BootstrapSnapshot {
  return {
    transitionEpoch,
    projects: [{ id: "project-a", name: "A", rootCount: 0, lastOpenedAt: 1 }],
    project: { id: "project-a", name: "A", roots: [] },
    tabs,
    archivedChats,
    activeTabId,
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
}

beforeEach(() => {
  useWorkbenchStore.setState({
    phase: "ready",
    error: null,
    projects: [{ id: "project-a", name: "A", rootCount: 0, lastOpenedAt: 1 }],
    project: { id: "project-a", name: "A", roots: [] },
    tabs: [chatA, chatB],
    archivedChats: [],
    activeTabId: chatA.id,
    selectedRootId: null,
    selectedResource: null,
    harnesses: [],
    selectedHarnessId: null,
    selectedModelId: null,
    models: [],
    sessionId: null,
    messages: [],
    composerText: "",
    sending: false,
    projectGeneration: 0,
    nativeTransitionEpoch: 0,
    projectTransitioning: false,
  });
  vi.spyOn(desktop, "activateChat").mockImplementation(
    async (layout, expectedTransitionEpoch) => ({
      committed: true,
      transitionEpoch: expectedTransitionEpoch + 1,
      snapshot: projection(
        layout.tabs,
        layout.activeTabId!,
        expectedTransitionEpoch + 1,
      ),
      recoveryError: null,
    }),
  );
});

afterEach(() => {
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initialState, true);
});

describe("per-Chat disposable ownership", () => {
  it("restores each Chat's independent composer across authoritative switches", async () => {
    useWorkbenchStore.getState().setComposerText("Draft A");

    await useWorkbenchStore.getState().activateChat(chatB.id);
    expect(useWorkbenchStore.getState().composerText).toBe("");
    useWorkbenchStore.getState().setComposerText("Draft B");

    await useWorkbenchStore.getState().activateChat(chatA.id);
    expect(useWorkbenchStore.getState().composerText).toBe("Draft A");
    await useWorkbenchStore.getState().activateChat(chatB.id);
    expect(useWorkbenchStore.getState().composerText).toBe("Draft B");
  });

  it("accepts the exact UTF-8 draft bound and reports plus one without replacing project errors", () => {
    const exact = "é".repeat(MAX_CHAT_COMPOSER_BYTES / 2);
    useWorkbenchStore.setState({ error: "Project warning" });

    useWorkbenchStore.getState().setComposerText(exact);
    expect(useWorkbenchStore.getState().composerText).toBe(exact);
    useWorkbenchStore.getState().setComposerText(`${exact}é`);

    expect(useWorkbenchStore.getState()).toMatchObject({
      composerText: exact,
      chatError: `Chat composer bytes exceed ${MAX_CHAT_COMPOSER_BYTES}`,
      error: "Project warning",
    });
  });

  it("clears all disposable Chat ownership on authoritative initialization", async () => {
    useWorkbenchStore.getState().setComposerText("Do not survive restart");
    const priorEpoch = useWorkbenchStore.getState().chatOwnershipEpoch;
    vi.spyOn(projectPort, "getBootstrapSnapshot").mockResolvedValue(
      projection([chatA, chatB], chatA.id, 2),
    );

    await useWorkbenchStore.getState().initialize();

    expect(useWorkbenchStore.getState()).toMatchObject({
      composerText: "",
      chatError: null,
      chatOwnershipEpoch: priorEpoch + 1,
      chatUiById: {
        [chatA.id]: { composerText: "" },
        [chatB.id]: { composerText: "" },
      },
    });
  });

  it("preserves Chat ownership through a same-Project root projection", async () => {
    useWorkbenchStore.getState().setComposerText("Keep through root change");
    vi.spyOn(projectPort, "persistProjectLayout").mockResolvedValue();
    vi.spyOn(projectPort, "addProjectRootFromPicker").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: {
        ...projection([chatA, chatB], chatA.id, 1),
        project: {
          id: "project-a",
          name: "A",
          roots: [
            {
              id: "root-new",
              name: "New",
              ordinal: 0,
              availability: "unavailable",
              message: "offline",
            },
          ],
        },
      },
      recoveryError: null,
    });

    await useWorkbenchStore.getState().addProjectRoot();

    expect(useWorkbenchStore.getState()).toMatchObject({
      phase: "ready",
      composerText: "Keep through root change",
      chatError: null,
    });
  });

  it("preserves ownership through archive/restore and lets Handoff replace only the target draft", async () => {
    useWorkbenchStore.getState().setComposerText("Draft A");
    await useWorkbenchStore.getState().activateChat(chatB.id);
    useWorkbenchStore.getState().setComposerText("Draft B");
    await useWorkbenchStore.getState().activateChat(chatA.id);

    vi.spyOn(desktop, "archiveChat").mockImplementation(
      async (_request, expectedTransitionEpoch) => ({
        committed: true,
        transitionEpoch: expectedTransitionEpoch + 1,
        snapshot: projection([chatA], chatA.id, expectedTransitionEpoch + 1, [
          { id: chatB.id, title: chatB.title },
        ]),
        recoveryError: null,
      }),
    );
    expect(await useWorkbenchStore.getState().archiveChat(chatB.id)).toBe(true);
    expect(useWorkbenchStore.getState().composerText).toBe("Draft A");

    vi.spyOn(desktop, "restoreChat").mockImplementation(
      async (_request, expectedTransitionEpoch) => ({
        committed: true,
        transitionEpoch: expectedTransitionEpoch + 1,
        snapshot: projection(
          [chatA, chatB],
          chatB.id,
          expectedTransitionEpoch + 1,
        ),
        recoveryError: null,
      }),
    );
    expect(await useWorkbenchStore.getState().restoreChat(chatB.id)).toBe(true);
    expect(useWorkbenchStore.getState().composerText).toBe("Draft B");

    await useWorkbenchStore.getState().activateChat(chatA.id);
    const state = useWorkbenchStore.getState();
    expect(
      await state.handoffChat({
        projectId: "project-a",
        projectGeneration: state.projectGeneration,
        transitionEpoch: state.nativeTransitionEpoch,
        sourceChatId: chatA.id,
        targetChatId: chatB.id,
        draft: "Handoff replaces B",
      }),
    ).toBe(true);
    expect(useWorkbenchStore.getState().composerText).toBe(
      "Handoff replaces B",
    );
    expect(useWorkbenchStore.getState().chatUiById[chatB.id].composerText).toBe(
      "Handoff replaces B",
    );

    await useWorkbenchStore.getState().activateChat(chatA.id);
    expect(useWorkbenchStore.getState().composerText).toBe("Draft A");
    await useWorkbenchStore.getState().activateChat(chatB.id);
    expect(useWorkbenchStore.getState().composerText).toBe(
      "Handoff replaces B",
    );
  });

  it("keeps a background send and its failure on the captured Chat only", async () => {
    const pending = deferred<Awaited<ReturnType<typeof desktop.sendPrompt>>>();
    vi.spyOn(desktop, "sendPrompt").mockReturnValue(pending.promise);
    useWorkbenchStore.setState({
      selectedHarnessId: "sage-simulator",
      selectedModelId: "sim-fast",
    });
    useWorkbenchStore.getState().setComposerText("Retry A");

    const sendingA = useWorkbenchStore.getState().sendPrompt();
    expect(useWorkbenchStore.getState()).toMatchObject({
      composerText: "",
      sending: true,
      chatError: null,
    });
    await useWorkbenchStore.getState().activateChat(chatB.id);
    expect(useWorkbenchStore.getState()).toMatchObject({
      composerText: "",
      sending: false,
      chatError: null,
    });
    useWorkbenchStore.getState().setComposerText("Draft B");

    pending.reject(new Error("A failed in background"));
    await sendingA;
    expect(useWorkbenchStore.getState()).toMatchObject({
      activeTabId: chatB.id,
      composerText: "Draft B",
      sending: false,
      chatError: null,
    });

    await useWorkbenchStore.getState().activateChat(chatA.id);
    expect(useWorkbenchStore.getState()).toMatchObject({
      composerText: "Retry A",
      sending: false,
      chatError: "A failed in background",
    });
  });

  it("applies ACP presentation updates only to the captured ownership epoch", async () => {
    useWorkbenchStore.getState().setComposerText("ACP A");
    const owner = useWorkbenchStore.getState().captureChatOwnership(chatA.id);
    expect(owner).not.toBeNull();
    expect(
      useWorkbenchStore
        .getState()
        .setChatActionState(owner!, { sending: true, error: null }),
    ).toBe(true);
    expect(
      useWorkbenchStore.getState().setChatActionState(owner!, {
        sending: false,
        error: "x".repeat(MAX_CHAT_ERROR_BYTES + 1),
      }),
    ).toBe(true);
    expect(useWorkbenchStore.getState().chatUiById[chatA.id].error).toBe(
      "x".repeat(MAX_CHAT_ERROR_BYTES),
    );
    useWorkbenchStore.getState().setChatActionState(owner!, {
      sending: true,
      error: null,
    });
    expect(
      useWorkbenchStore.getState().clearChatComposer(owner!, "ACP A"),
    ).toBe(true);

    await useWorkbenchStore.getState().activateChat(chatB.id);
    useWorkbenchStore.getState().setComposerText("ACP B");
    expect(
      useWorkbenchStore.getState().restoreChatComposer(owner!, "Retry ACP A"),
    ).toBe(true);
    expect(
      useWorkbenchStore.getState().setChatActionState(owner!, {
        sending: false,
        error: "ACP A failed in background",
      }),
    ).toBe(true);
    expect(useWorkbenchStore.getState()).toMatchObject({
      composerText: "ACP B",
      sending: false,
      chatError: null,
    });

    await useWorkbenchStore.getState().activateChat(chatA.id);
    expect(useWorkbenchStore.getState()).toMatchObject({
      composerText: "Retry ACP A",
      sending: false,
      chatError: "ACP A failed in background",
    });

    vi.spyOn(projectPort, "getBootstrapSnapshot").mockResolvedValue(
      projection([chatA, chatB], chatA.id, 9),
    );
    await useWorkbenchStore.getState().initialize();
    expect(
      useWorkbenchStore.getState().setChatActionState(owner!, {
        sending: false,
        error: "stale failure",
      }),
    ).toBe(false);
    expect(useWorkbenchStore.getState()).toMatchObject({
      composerText: "",
      sending: false,
      chatError: null,
    });
  });

  it("accepts exact attached/archive ownership ceilings and rejects either plus one", async () => {
    const attached = Array.from(
      { length: MAX_ATTACHED_CHAT_OWNERSHIP_ENTRIES },
      (_, index): WorkbenchTab => ({
        ...chatA,
        id: `attached-${index}`,
        position: index,
      }),
    );
    const archived = Array.from(
      { length: MAX_ARCHIVED_CHAT_OWNERSHIP_ENTRIES },
      (_, index) => ({ id: `archived-${index}`, title: `Archived ${index}` }),
    );
    const bootstrap = vi.spyOn(projectPort, "getBootstrapSnapshot");
    bootstrap.mockResolvedValueOnce(
      projection(attached, attached[0].id, 10, archived),
    );

    await useWorkbenchStore.getState().initialize();
    expect(Object.keys(useWorkbenchStore.getState().chatUiById)).toHaveLength(
      MAX_CHAT_OWNERSHIP_ENTRIES,
    );

    bootstrap.mockResolvedValueOnce(
      projection(
        [...attached, { ...chatA, id: "attached-overflow", position: 64 }],
        attached[0].id,
        11,
      ),
    );
    await useWorkbenchStore.getState().initialize();
    expect(useWorkbenchStore.getState()).toMatchObject({
      phase: "error",
      error: expect.stringContaining("attached Chat ownership entries"),
    });

    bootstrap.mockResolvedValueOnce(
      projection([chatA], chatA.id, 12, [
        ...archived,
        { id: "archived-overflow", title: "Archived overflow" },
      ]),
    );
    await useWorkbenchStore.getState().initialize();
    expect(useWorkbenchStore.getState()).toMatchObject({
      phase: "error",
      error: expect.stringContaining("archived Chat ownership entries"),
    });
  });
});
