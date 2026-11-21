import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { MAX_CHAT_COMPOSER_BYTES } from "../domain/chatOwnership";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { resetBrowserDesktopForTests } from "../services/browserDesktop";
import { useWorkbenchStore } from "./store";
import { desktop } from "../services/desktop";
import { resetHarnessModelCache } from "./harnessModelCache";
import { useChatEffortDraft } from "./chatEffortDraft";
import { effectiveDraftChoice } from "./newChatDraftNavigation";
import type { HarnessDefinition, ModelOption } from "../domain/workbench";
import {
  chatTitles,
  durableChatIds,
  messagesByChat,
} from "../services/browserChatState";
import { browserSnapshot } from "../services/browserWorkspaceFixture";
import { emptyChatOwnershipEntry } from "./chatOwnershipProjection";

const initial = useWorkbenchStore.getState();
const ports = getWorkbenchPorts();
beforeEach(async () => {
  resetBrowserDesktopForTests();
  useWorkbenchStore.setState(initial, true);
  await useWorkbenchStore.getState().initialize();
  await useWorkbenchStore.getState().openNewChatDraft();
});
afterEach(() => {
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initial, true);
  resetBrowserDesktopForTests();
});

async function readyProjectDraft(
  text: string,
  projectId = useWorkbenchStore.getState().project!.id,
) {
  const state = useWorkbenchStore.getState();
  await state.openNewChatDraft(projectId);
  useWorkbenchStore.setState({
    harnesses: [
      {
        id: "sage-simulator",
        name: "Fixture ACP",
        description: "Draft handoff fixture",
        source: "builtIn",
        transport: "acp",
        installState: "available",
        bindingId: "fixture-binding",
        accountModelDiscovery: false,
      },
    ],
  });
  await state.selectDraftHarness("sage-simulator");
  state.updateNewChatDraft({ text });
}

it("hands a first prompt to a reusable chat in another project", async () => {
  const state = useWorkbenchStore.getState();
  const sourceId = state.project!.id;
  await state.createProject({ name: "Target", sourceTokens: [] });
  const targetId = useWorkbenchStore.getState().project!.id;
  await state.createChat(targetId);
  const reusable = useWorkbenchStore.getState().activeTabId;
  await state.switchProject(sourceId);
  await readyProjectDraft("First prompt in target", targetId);
  await state.sendNewChatDraft();
  expect(useWorkbenchStore.getState()).toMatchObject({
    project: { id: targetId },
    activeTabId: reusable,
    newChatDraft: null,
    composerText: "First prompt in target",
    pendingFirstSend: { chatId: reusable, text: "First prompt in target" },
  });
  expect(durableChatIds.size).toBe(2);
});

it.each(["refused", "committed"])(
  "recovers a lost reuse response only when the selection %s",
  async (outcome) => {
    const state = useWorkbenchStore.getState();
    await state.createChat(state.project!.id);
    const reusable = useWorkbenchStore.getState().activeTabId;
    await readyProjectDraft("Preserve through lost response");
    const nativeCreate = ports.chat.createChat;
    vi.spyOn(ports.chat, "createChat").mockImplementationOnce(
      async (...args) => {
        if (outcome === "committed") await nativeCreate(...args);
        else
          browserSnapshot.tabs = browserSnapshot.tabs.map((tab) =>
            tab.id === reusable ? { ...tab, pinned: true } : tab,
          );
        throw new Error("Reuse reply lost");
      },
    );
    await state.sendNewChatDraft();
    const result = useWorkbenchStore.getState();
    if (outcome === "committed") {
      expect(result.newChatDraft).toBeNull();
      expect(result.composerText).toBe("Preserve through lost response");
      expect(result.pendingFirstSend).toMatchObject({ chatId: reusable });
    } else {
      expect(result.newChatDraft).toMatchObject({
        text: "Preserve through lost response",
        submission: { status: "released" },
      });
      expect(result.composerText).toBe("");
      expect(result.pendingFirstSend).toBeNull();
    }
    expect(durableChatIds.size).toBe(2);
  },
);

it("cancels without allocating when Editor is entered during the reusable-chat lookup", async () => {
  const state = useWorkbenchStore.getState();
  await readyProjectDraft("Keep this cancelled prompt");
  let finishLookup!: (id: string | null) => void;
  let began!: () => void;
  const started = new Promise<void>((resolve) => {
    began = resolve;
  });
  const lookup = new Promise<string | null>((resolve) => {
    finishLookup = resolve;
  });
  vi.spyOn(ports.chat, "getReusableChat").mockImplementationOnce(() => {
    began();
    return lookup;
  });
  const sending = state.sendNewChatDraft();
  await started;
  useWorkbenchStore.setState({ pendingFirstSend: null });
  finishLookup(null);
  await sending;
  expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
    text: "Keep this cancelled prompt",
    submission: { status: "released" },
  });
  expect(durableChatIds.size).toBe(1);
});

it.each(["none", "typed", "queued"])(
  "sends an upgraded project draft at capacity, preserving %s input",
  async (input) => {
    const state = useWorkbenchStore.getState();
    messagesByChat.set(state.activeTabId!, []);
    const ids: string[] = [state.activeTabId!];
    for (let index = 1; index < 64; index += 1) {
      const result = await state.createChat(state.project!.id);
      expect(result.status).toBe("confirmed");
      ids.push(useWorkbenchStore.getState().activeTabId!);
    }
    // Model the native upgrade projection: all 64 durable defaults are hidden.
    browserSnapshot.tabs = browserSnapshot.tabs.map((tab) => ({
      ...tab,
      title: "Chat: Chat",
      pinned: false,
      chatListed: false,
    }));
    for (const id of ids) chatTitles.set(id, "Chat: Chat");
    useWorkbenchStore.setState({
      tabs: structuredClone(browserSnapshot.tabs),
      activeTabId: null,
    });
    if (input !== "none")
      useWorkbenchStore.setState({
        chatUiById: {
          ...useWorkbenchStore.getState().chatUiById,
          [ids[0]]: {
            ...emptyChatOwnershipEntry(),
            composerText: input === "typed" ? "Keep my unsent prompt" : "",
            queuedPrompts:
              input === "queued"
                ? [
                    {
                      id: "queued",
                      text: "Keep my queue",
                      attachedContext: [],
                      bindingId: "fixture-binding",
                      modelId: null,
                      status: "queued",
                    },
                  ]
                : [],
          },
        },
      });
    await state.openNewChatDraft(state.project!.id);
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "sage-simulator",
          name: "Fixture ACP",
          description: "Draft handoff fixture",
          source: "builtIn",
          transport: "acp",
          installState: "available",
          bindingId: "fixture-binding",
          accountModelDiscovery: false,
        },
      ],
    });
    await state.selectDraftHarness("sage-simulator");
    state.updateNewChatDraft({ text: "First prompt after upgrade" });
    await state.sendNewChatDraft();
    const sent = useWorkbenchStore.getState();
    expect(sent.error).toBeNull();
    expect(sent.newChatDraft).toBeNull();
    expect(sent.activeTabId).toBe(ids[input === "none" ? 0 : 1]);
    expect(sent.composerText).toBe("First prompt after upgrade");
    expect(sent.tabs.filter((tab) => tab.kind === "chat")).toHaveLength(64);
    expect(durableChatIds.size).toBe(64);
    if (input === "typed")
      expect(sent.chatUiById[ids[0]].composerText).toBe(
        "Keep my unsent prompt",
      );
    if (input === "queued")
      expect(sent.chatUiById[ids[0]].queuedPrompts?.[0].text).toBe(
        "Keep my queue",
      );
  },
);

it.each(["current", "foreign"])(
  "preserves an unsent draft when New chat explicitly targets a different %s project",
  async (target) => {
    const state = useWorkbenchStore.getState();
    state.updateNewChatDraft({ text: "Keep this prompt", effort: "high" });
    const draft = useWorkbenchStore.getState().newChatDraft;
    await state.activateTab(state.activeTabId!);
    await state.openNewChatDraft(
      target === "current" ? state.project!.id : "another-project",
    );
    expect(useWorkbenchStore.getState()).toMatchObject({
      agentCenter: "draft",
      project: { id: state.project!.id },
      newChatDraft: draft,
    });
  },
);

it("starts a fresh draft for an explicit different target when the draft is empty", async () => {
  const state = useWorkbenchStore.getState();
  const draft = state.newChatDraft;
  await state.openNewChatDraft(state.project!.id);
  expect(useWorkbenchStore.getState().newChatDraft).not.toBe(draft);
  expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
    text: "",
    targetProjectId: state.project!.id,
  });
});

it("bounds draft UTF-8 text and keeps the previous chat's text independently", async () => {
  const state = useWorkbenchStore.getState();
  state.setComposerText("Previous chat text");
  const text = "é".repeat(MAX_CHAT_COMPOSER_BYTES / 2);
  state.updateNewChatDraft({ text });
  expect(useWorkbenchStore.getState().newChatDraft?.text).toBe(text);
  state.updateNewChatDraft({ text: text + "é" });
  expect(useWorkbenchStore.getState().newChatDraft?.text).toBe(text);
  expect(useWorkbenchStore.getState().newChatDraft?.error).toMatch(
    /limit|bytes|exceed/i,
  );
  await state.openNewChatDraft();
  await state.activateTab(state.activeTabId!);
  expect(useWorkbenchStore.getState().composerText).toBe("Previous chat text");
  expect(useWorkbenchStore.getState().agentCenter).toBe("chat");
});

it.each([
  { phase: "error" as const, modelId: null, empty: true },
  { phase: "checking" as const, modelId: null, empty: true },
])(
  "retains the draft without creating for $phase discovery and model $modelId (empty: $empty)",
  async ({ phase, modelId, empty }) => {
    const state = useWorkbenchStore.getState();
    const before = await ports.project.getBootstrapSnapshot();
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "draft-harness",
          name: "Draft harness",
          description: "Fixture ACP harness",
          source: "builtIn",
          transport: "acp",
          installState: "available",
          bindingId: "draft-binding",
          accountModelDiscovery: false,
        },
      ],
      selectedHarnessId: "draft-harness",
      models: empty ? [] : [{ id: "model-a", name: "Model A" }],
      harnessDiscoveryPhase: phase,
      newChatDraft: {
        ...state.newChatDraft!,
        harnessId: "draft-harness",
        modelId,
        text: "Keep this until discovery permits sending",
      },
    });
    await state.sendNewChatDraft();
    expect((await ports.project.getBootstrapSnapshot()).tabs).toEqual(
      before.tabs,
    );
    expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
      text: "Keep this until discovery permits sending",
      modelId,
      submission: { status: "released" },
      error: "Choose an available harness and model before sending.",
    });
    expect(useWorkbenchStore.getState().pendingFirstSend).toBeNull();
  },
);

it("updates draft choices synchronously and freezes edits as soon as sending starts", async () => {
  const state = useWorkbenchStore.getState();
  useWorkbenchStore.setState({
    harnesses: [],
    models: [
      {
        id: "thinking",
        name: "Thinking",
        isDefault: true,
        effortLevels: ["high"],
      },
      { id: "plain", name: "Plain", isDefault: false },
    ],
  });
  state.updateNewChatDraft({ text: "First prompt", effort: "high" });
  state.selectDraftModel("thinking");
  expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
    text: "First prompt",
    modelId: "thinking",
    effort: "high",
  });
  state.selectDraftModel("plain");
  expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
    modelId: "plain",
    effort: null,
  });

  const sending = state.sendNewChatDraft();
  state.updateNewChatDraft({ text: "Too late" });
  state.selectDraftModel("thinking");
  expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
    text: "First prompt",
    modelId: "plain",
    submission: { status: "submitting" },
  });
  await sending;
  expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
    text: "First prompt",
    submission: { status: "released" },
    error: "Choose an available harness and model before sending.",
  });
});

it("keeps the draft through same-project refresh and failed switching, then discards it on successful switching", async () => {
  const state = useWorkbenchStore.getState();
  state.updateNewChatDraft({ text: "Project draft" });
  await state.createChat();
  expect(useWorkbenchStore.getState().newChatDraft?.text).toBe("Project draft");
  const snapshot = await ports.project.getBootstrapSnapshot();
  vi.spyOn(ports.project, "switchProject").mockResolvedValueOnce({
    committed: false,
    snapshot: null,
    transitionEpoch: snapshot.transitionEpoch,
    recoveryError: "Switch rejected",
  });
  await state.switchProject("missing-project");
  expect(useWorkbenchStore.getState().newChatDraft?.text).toBe("Project draft");
  const changed = {
    ...snapshot,
    project: { id: "other-project", name: "Other", roots: [] },
    tabs: [],
    archivedChats: [],
    activeTabId: null,
    transitionEpoch: snapshot.transitionEpoch + 1,
  };
  vi.mocked(ports.project.switchProject).mockResolvedValueOnce({
    committed: true,
    snapshot: changed,
    transitionEpoch: changed.transitionEpoch,
    recoveryError: null,
  });
  await state.switchProject("other-project");
  expect(useWorkbenchStore.getState().project?.id).toBe("other-project");
  expect(useWorkbenchStore.getState().newChatDraft).toBeNull();
  expect(useWorkbenchStore.getState().agentCenter).toBe("chat");
});

it("refuses delayed creation after the project ownership lifetime changes", async () => {
  const before = await ports.project.getBootstrapSnapshot();
  const state = useWorkbenchStore.getState();
  const creation = state.createChat();
  useWorkbenchStore.setState({
    chatOwnershipEpoch: state.chatOwnershipEpoch + 1,
  });
  expect(await creation).toEqual({ status: "not-created" });
  expect((await ports.project.getBootstrapSnapshot()).tabs).toEqual(
    before.tabs,
  );
});

it("keeps the latest draft model when A, B, A discovery completes out of order", async () => {
  resetHarnessModelCache();
  useWorkbenchStore.setState({
    harnesses: ["A", "B"].map((id): HarnessDefinition => ({
      id,
      name: id,
      description: "Fixture ACP harness",
      source: "builtIn",
      transport: "acp",
      installState: "available",
      bindingId: `binding-${id}`,
      accountModelDiscovery: false,
    })),
  });
  const models = [
    { id: "default", name: "Default", isDefault: true },
    { id: "chosen", name: "Chosen" },
  ];
  let finishFirst!: (value: typeof models) => void;
  let finishLatest!: (value: typeof models) => void;
  vi.spyOn(desktop, "listHarnessModels")
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishFirst = resolve;
        }),
    )
    .mockResolvedValueOnce(models)
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishLatest = resolve;
        }),
    );
  const state = useWorkbenchStore.getState();
  const first = state.selectDraftHarness("A");
  await vi.waitFor(() => expect(finishFirst).toBeDefined());
  await state.selectDraftHarness("B");
  const latest = state.selectDraftHarness("A");
  await vi.waitFor(() => expect(finishLatest).toBeDefined());
  finishLatest(models);
  await latest;
  state.selectDraftModel("chosen");
  finishFirst(models);
  await first;
  expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
    harnessId: "A",
    modelId: "chosen",
  });
});

it.each<{
  catalog: ModelOption[];
  modelId: string | null;
  effort: string | null;
}>([
  { catalog: [], modelId: null, effort: null },
  {
    catalog: [{ id: "thinking", name: "Thinking" }],
    modelId: "thinking",
    effort: null,
  },
  {
    catalog: [
      { id: "plain", name: "Plain" },
      { id: "new-default", name: "New default", isDefault: true },
    ],
    modelId: "new-default",
    effort: null,
  },
  {
    catalog: [
      { id: "new-default", name: "New default", isDefault: true },
      { id: "thinking", name: "Thinking", effortLevels: ["high", "low"] },
    ],
    modelId: "thinking",
    effort: "high",
  },
])(
  "sends with model $modelId, effort $effort after a catalog refresh, keeping the stored choice",
  async ({ catalog, modelId, effort }) => {
    resetHarnessModelCache();
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "refresh-harness",
          name: "Refresh harness",
          description: "Fixture ACP harness",
          source: "builtIn",
          transport: "acp",
          installState: "available",
          bindingId: "refresh-binding",
          accountModelDiscovery: false,
        },
      ],
    });
    vi.spyOn(desktop, "listHarnessModels").mockResolvedValue([
      { id: "old-default", name: "Old default", isDefault: true },
      { id: "thinking", name: "Thinking", effortLevels: ["high"] },
    ]);
    const state = useWorkbenchStore.getState();
    await state.selectDraftHarness("refresh-harness");
    state.selectDraftModel("thinking");
    state.updateNewChatDraft({ text: "Keep through refresh", effort: "high" });
    vi.mocked(desktop.listHarnessModels).mockResolvedValue(catalog);
    await state.retryHarnessDiscovery();
    await vi.waitFor(() =>
      expect(useWorkbenchStore.getState().models).toEqual(catalog),
    );
    const refreshed = useWorkbenchStore.getState();
    expect(refreshed.newChatDraft).toMatchObject({
      modelId: "thinking",
      effort: "high",
    });
    expect(
      effectiveDraftChoice(refreshed.newChatDraft!, refreshed),
    ).toMatchObject({ modelId, effort });
    await state.sendNewChatDraft();
    const sent = useWorkbenchStore.getState();
    expect(sent.newChatDraft).toBeNull();
    expect(sent.composerText).toBe("Keep through refresh");
    expect(sent.chatUiById[sent.activeTabId!]).toMatchObject({
      harnessId: "refresh-harness",
      modelId,
    });
    expect(useChatEffortDraft.getState().draft?.effort ?? null).toBe(effort);
  },
);

it("opens a foreign project's unsent draft without creating a chat or switching projects", async () => {
  const state = useWorkbenchStore.getState();
  const sourceId = state.project!.id;
  expect(await state.createProject({ name: "Target", sourceTokens: [] })).toBe(
    true,
  );
  const targetId = useWorkbenchStore.getState().project!.id;
  await state.switchProject(sourceId);
  const before = await ports.project.getBootstrapSnapshot();
  await state.openNewChatDraft(targetId);
  expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
    projectId: sourceId,
    targetProjectId: targetId,
    text: "",
  });
  expect(await ports.project.getBootstrapSnapshot()).toEqual(before);
});
