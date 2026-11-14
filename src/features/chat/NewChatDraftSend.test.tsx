import { EditorContinuityKeeper } from "../../workbench/WorkbenchContinuity";
import { StrictMode, useState } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../../app/store";
import * as workspaceNotify from "../../app/workspaceNotifyLifecycle";
import { clearAcpProjectionRegistry } from "../../app/acpProjectionRegistry";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import { useChatAccessDrafts } from "../../app/chatAccessDrafts";
import { useChatEffortDraft } from "../../app/chatEffortDraft";
import type {
  AcpChatPage,
  StartAcpSessionRequest,
  SendAcpTurnRequest,
} from "../../domain/acp";
import { agentSessionFixture } from "../../domain/acpProjection.fixture";
import type {
  BootstrapSnapshot,
  ProjectTransitionResult,
} from "../../domain/workbench";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { acpDesktop } from "../../services/acpDesktop";
import { AcpDesktopError } from "../../services/acpDesktopCommand";
import { desktop } from "../../services/desktop";
import { acpHarnessDesktop } from "../../services/acpHarnessDesktop";
import { resetBrowserDesktopForTests } from "../../services/browserDesktop";
import { settleLazyModules } from "../../test/settleLazyModules";
import { AgentChatRuntimeHarness } from "../../test/AgentChatRuntimeHarness";
import { registerWorkbenchSurfaces } from "../../workbench/registeredSurfaces";
import { AgentChatPanes } from "../../workbench/AgentChatPanes";
import { resetHarnessModelCache } from "../../app/harnessModelCache";
import { LeftSidebar } from "../../workbench/LeftSidebar";
import { openSideChat } from "../../workbench/sideChatActions";
import { useSideChatState } from "../../workbench/sideChatState";

const initialState = useWorkbenchStore.getState();
const ports = getWorkbenchPorts();
const models = [
  {
    id: "model-a",
    name: "Model A",
    isDefault: true,
    effortLevels: ["low", "high"],
  },
  { id: "model-b", name: "Model B", isDefault: false },
];
const harness = {
  id: "draft-harness",
  name: "Draft harness",
  description: "Fixture ACP harness",
  source: "builtIn" as const,
  transport: "acp" as const,
  accountModelDiscovery: false,
  installState: "available" as const,
  bindingId: "draft-binding",
  accessModes: [
    {
      id: "readOnly" as const,
      label: "Read only",
      description: "Read files",
      fallback: true,
    },
    {
      id: "ask" as const,
      label: "Ask each time",
      description: "Ask before acting",
    },
  ],
};
const otherHarness = {
  ...harness,
  id: "other-draft-harness",
  name: "Other harness",
  bindingId: "other-binding",
};
let starts: StartAcpSessionRequest[];
let prompts: SendAcpTurnRequest[];
let pages: Map<string, AcpChatPage>;

function withHarnessCatalog(result: ProjectTransitionResult) {
  if (result.snapshot) result.snapshot = withHarnessSnapshot(result.snapshot);
  return result;
}

function withHarnessSnapshot(snapshot: BootstrapSnapshot): BootstrapSnapshot {
  return {
    ...snapshot,
    harnesses: [harness, otherHarness],
    selectedHarnessId: harness.id,
    selectedModelId: "model-a",
  };
}

function emptyPage(projectId: string, chatId: string): AcpChatPage {
  return {
    projectId,
    chatId,
    title: "New chat",
    projectionRevision: "1",
    currentSessionId: null,
    sessions: [],
    turns: [],
    nextCursor: null,
  } as unknown as AcpChatPage;
}
function mount(strict = false, compact = false) {
  registerWorkbenchSurfaces();
  const content = (
    <AgentChatRuntimeHarness>
      <AgentChatPanes compact={compact} />
    </AgentChatRuntimeHarness>
  );
  return render(strict ? <StrictMode>{content}</StrictMode> : content);
}
const composer = () => screen.getByRole("combobox", { name: "Message agent" });

beforeEach(async () => {
  useSideChatState.setState({ sides: [], pairedId: null, pendingClose: null });
  resetBrowserDesktopForTests();
  useWorkbenchStore.setState(initialState, true);
  await useWorkbenchStore.getState().initialize();
  useSessionAccessModeStore.setState({ ready: true, modes: {}, error: null });
  useChatAccessDrafts.setState({ drafts: {} });
  useChatEffortDraft.setState({ draft: null });
  starts = [];
  prompts = [];
  pages = new Map();
  vi.spyOn(desktop, "listHarnessModels").mockResolvedValue(models);
  useWorkbenchStore.setState({
    harnesses: [harness, otherHarness],
    selectedHarnessId: harness.id,
    selectedModelId: "model-a",
    models,
    harnessDiscoveryPhase: "ready",
  });
  const create = ports.chat.createChat.bind(ports.chat);
  vi.spyOn(ports.chat, "createChat").mockImplementation(async (...args) =>
    withHarnessCatalog(await create(...args)),
  );
  const activate = ports.chat.activateChat.bind(ports.chat);
  vi.spyOn(ports.chat, "activateChat").mockImplementation(async (...args) =>
    withHarnessCatalog(await activate(...args)),
  );
  const restore = ports.chat.restoreChat.bind(ports.chat);
  vi.spyOn(ports.chat, "restoreChat").mockImplementation(async (...args) =>
    withHarnessCatalog(await restore(...args)),
  );
  vi.spyOn(acpDesktop, "getChatPage").mockImplementation(
    async (request) =>
      pages.get(request.chatId) ?? emptyPage(request.projectId, request.chatId),
  );
  vi.spyOn(acpDesktop, "startSession").mockImplementation(async (request) => {
    starts.push(request);
    const session = agentSessionFixture({
      bindingId: request.bindingId,
      modelId: request.modelId,
      runtimeEpoch: "1" as never,
      residency: "hot",
      state: "active",
    });
    const page = {
      ...emptyPage(request.projectId, request.chatId),
      projectionRevision: "2" as never,
      sessions: [session],
      currentSessionId: session.id,
    };
    pages.set(request.chatId, page);
    return { projection: page };
  });
  vi.spyOn(acpDesktop, "sendTurn").mockImplementation(async (request) => {
    prompts.push(request);
    const page = {
      ...pages.get(request.chatId)!,
      projectionRevision: "3" as never,
    };
    pages.set(request.chatId, page);
    return { projection: page };
  });
  await useWorkbenchStore.getState().openNewChatDraft();
});
afterEach(() => {
  cleanup();
  clearAcpProjectionRegistry();
  useSideChatState.setState({ sides: [], pairedId: null, pendingClose: null });
  vi.restoreAllMocks();
  useChatAccessDrafts.setState({ drafts: {} });
  useChatEffortDraft.setState({ draft: null });
  useWorkbenchStore.setState(initialState, true);
  resetBrowserDesktopForTests();
});

it("creates one durable chat and sends once through its real scoped page, including across remount", async () => {
  const before = await ports.project.getBootstrapSnapshot();
  const view = mount(true);
  await settleLazyModules();
  fireEvent.change(composer(), {
    target: { value: "Build a focused workbench" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Harness" }));
  fireEvent.click(await screen.findByRole("option", { name: /Other harness/ }));
  await settleLazyModules();
  fireEvent.click(screen.getByRole("button", { name: "Model" }));
  fireEvent.click(await screen.findByRole("option", { name: /Model B/ }));
  fireEvent.submit(screen.getByRole("form"));
  fireEvent.submit(screen.getByRole("form"));
  await waitFor(() => expect(prompts).toHaveLength(1));
  await settleLazyModules();
  const after = await ports.project.getBootstrapSnapshot();
  expect(after.tabs.length).toBe(before.tabs.length + 1);
  expect(after.activeTabId).not.toBe(before.activeTabId);
  expect(starts).toMatchObject([
    {
      chatId: after.activeTabId,
      bindingId: otherHarness.bindingId,
      modelId: "model-b",
    },
  ]);
  expect(prompts).toMatchObject([
    {
      chatId: after.activeTabId,
      projectId: before.project!.id,
      prompt: "Build a focused workbench",
    },
  ]);
  expect(composer()).toHaveValue("");
  expect(screen.queryByRole("region", { name: "New chat draft" })).toBeNull();
  expect(screen.getByRole("region", { name: /^Chat pane/ })).toBeVisible();
  view.unmount();
  mount();
  await settleLazyModules();
  expect(prompts).toHaveLength(1);
  expect((await ports.project.getBootstrapSnapshot()).tabs).toEqual(after.tabs);
});

it("notCreated keeps the draft and allows a retry that creates exactly one chat", async () => {
  const before = await ports.project.getBootstrapSnapshot();
  vi.mocked(ports.chat.createChat).mockResolvedValueOnce({
    committed: false,
    snapshot: null,
    transitionEpoch: before.transitionEpoch,
    recoveryError: "Rejected",
  });
  mount();
  await settleLazyModules();
  fireEvent.change(composer(), { target: { value: "Keep this request" } });
  fireEvent.click(screen.getByRole("button", { name: "Model" }));
  fireEvent.click(await screen.findByRole("option", { name: /Model B/ }));
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  expect(composer()).toBeDisabled();
  expect(screen.getByRole("button", { name: "Model" })).toBeDisabled();
  await waitFor(() =>
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Could not create the chat",
    ),
  );
  expect(composer()).toHaveValue("Keep this request");
  expect(composer()).toBeEnabled();
  expect(screen.getByRole("button", { name: "Send message" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "Model" })).toHaveAttribute(
    "data-selected-id",
    "model-b",
  );
  expect((await ports.project.getBootstrapSnapshot()).tabs).toEqual(
    before.tabs,
  );
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() => expect(prompts).toHaveLength(1));
  expect(starts[0]?.modelId).toBe("model-b");
  expect(prompts[0]?.prompt).toBe("Keep this request");
  expect((await ports.project.getBootstrapSnapshot()).tabs.length).toBe(
    before.tabs.length + 1,
  );
});

it("sends with Pi's default when discovery returns its empty pre-session catalog", async () => {
  // Native HarnessCatalog::models_for returns [] for Pi's ACP binding.
  const pi = {
    id: "pi",
    name: "Pi",
    description: "Local Pi RPC harness",
    source: "builtIn" as const,
    transport: "acp" as const,
    accountModelDiscovery: false,
    installState: "available" as const,
    bindingId: "pi-rpc",
    accessModes: [],
  };
  resetHarnessModelCache();
  vi.mocked(desktop.listHarnessModels).mockResolvedValue([]);
  useWorkbenchStore.setState({ harnesses: [harness, pi] });
  await useWorkbenchStore.getState().selectDraftHarness(pi.id);
  const create = vi.mocked(ports.chat.createChat).getMockImplementation()!;
  vi.mocked(ports.chat.createChat).mockImplementation(async (...args) => {
    const result = await create(...args);
    if (result.snapshot)
      result.snapshot = {
        ...result.snapshot,
        harnesses: [pi],
        selectedHarnessId: pi.id,
        selectedModelId: null,
      };
    return result;
  });
  const before = await ports.project.getBootstrapSnapshot();
  mount(true);
  await settleLazyModules();
  fireEvent.change(composer(), { target: { value: "Use Pi's default" } });
  expect(screen.getByRole("button", { name: "Model" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Send message" })).toBeEnabled();
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() => expect(prompts).toHaveLength(1));
  await settleLazyModules();
  const after = await ports.project.getBootstrapSnapshot();
  expect(after.tabs).toHaveLength(before.tabs.length + 1);
  expect(starts).toMatchObject([
    { chatId: after.activeTabId, bindingId: "pi-rpc", modelId: null },
  ]);
  expect(prompts).toMatchObject([
    { chatId: after.activeTabId, prompt: "Use Pi's default" },
  ]);
  expect(composer()).toHaveValue("");
});

it.each([
  { layout: "created", accessRefused: false, inactiveTarget: false },
  { layout: "created", accessRefused: true, inactiveTarget: false },
  { layout: "unchanged", accessRefused: true, inactiveTarget: false },
  { layout: "created", accessRefused: false, inactiveTarget: true },
])(
  "Reload chats reconciles without losing text or creating twice ($layout layout, access refused: $accessRefused, inactive target: $inactiveTarget)",
  async ({ layout, accessRefused, inactiveTarget }) => {
    const before = await ports.project.getBootstrapSnapshot();
    const state = useWorkbenchStore.getState();
    const priorId = state.activeTabId!;
    const owner = state.captureChatOwnership(priorId)!;
    state.setChatComposer(owner, "Existing chat's own text");
    state.setChatHarnessSelection(owner, {
      harnessId: otherHarness.id,
      modelId: "model-b",
    });
    const priorChat = useWorkbenchStore.getState().chatUiById[priorId];
    const create = vi.mocked(ports.chat.createChat).getMockImplementation()!;
    vi.mocked(ports.chat.createChat).mockImplementationOnce(async (...args) => {
      if (layout === "created")
        return { ...(await create(...args)), snapshot: null };
      throw new Error("Creation response unavailable");
    });
    const bootstrap = ports.project.getBootstrapSnapshot.bind(ports.project);
    const projection = vi
      .spyOn(ports.project, "getBootstrapSnapshot")
      .mockRejectedValue(new Error("Projection unavailable"));
    render(<LeftSidebar />);
    let view = mount();
    await settleLazyModules();
    fireEvent.change(composer(), {
      target: { value: "Keep my unconfirmed request" },
    });
    if (accessRefused) {
      fireEvent.click(screen.getByRole("button", { name: "Access options" }));
      fireEvent.click(
        await screen.findByRole("menuitemradio", { name: "Ask each time" }),
      );
    }
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    const message =
      "Chat creation is unconfirmed. Reload chats before trying again.";
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(message),
    );
    expect(composer()).toHaveValue("Keep my unconfirmed request");
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    fireEvent.submit(screen.getByRole("form"));
    await settleLazyModules();
    const after = await bootstrap();
    expect(after.tabs.length).toBe(
      before.tabs.length + (layout === "created" ? 1 : 0),
    );
    expect(prompts).toEqual([]);
    view.unmount();
    view = mount();
    await settleLazyModules();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Reload chats" }));
    await settleLazyModules();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    expect(screen.getByRole("alert")).toHaveTextContent(message);
    expect(composer()).toHaveValue("Keep my unconfirmed request");
    expect(screen.getByRole("button", { name: "Reload chats" })).toBeEnabled();
    projection.mockImplementation(async () => {
      const snapshot = await bootstrap();
      return {
        ...snapshot,
        activeTabId: inactiveTarget ? priorId : snapshot.activeTabId,
        harnesses: [harness, otherHarness],
        selectedHarnessId: harness.id,
        selectedModelId: "model-a",
      };
    });
    fireEvent.click(screen.getByRole("button", { name: "Reload chats" }));
    fireEvent.click(screen.getByRole("button", { name: "Reload chats" }));
    await settleLazyModules();
    if (layout === "unchanged") {
      expect(
        screen.getByRole("region", { name: "New chat draft" }),
      ).toBeVisible();
      expect(composer()).toHaveValue("Keep my unconfirmed request");
      expect(composer()).toBeEnabled();
      expect(
        screen.getByRole("button", { name: "Send message" }),
      ).toBeEnabled();
      expect(
        screen.getByRole("button", { name: "Access options" }),
      ).toHaveTextContent("Ask each time");
      expect(prompts).toEqual([]);
      expect((await bootstrap()).tabs).toEqual(before.tabs);
      fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    }
    if (accessRefused) {
      expect(
        await screen.findByText(/The chosen access was not given to this chat/),
      ).toBeVisible();
      expect(composer()).toHaveValue("Keep my unconfirmed request");
      expect(composer()).toBeEnabled();
      expect(prompts).toEqual([]);
      await waitFor(() =>
        expect(
          screen.getByRole("button", { name: "Send message" }),
        ).toBeEnabled(),
      );
      fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    }
    await waitFor(() => expect(prompts).toHaveLength(1));
    await settleLazyModules();
    const recovered = await bootstrap();
    expect(recovered.tabs).toHaveLength(before.tabs.length + 1);
    if (layout === "created") expect(recovered.tabs).toEqual(after.tabs);
    expect(
      screen.getAllByRole("button", { name: "Open chat Chat" }),
    ).toHaveLength(1);
    expect(screen.queryByRole("region", { name: "New chat draft" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Reload chats" })).toBeNull();
    expect(composer()).toBeEnabled();
    expect(composer()).toHaveValue("");
    expect(screen.queryByText(message)).toBeNull();
    expect(prompts).toMatchObject([
      { chatId: recovered.activeTabId, prompt: "Keep my unconfirmed request" },
    ]);
    expect(useWorkbenchStore.getState().chatUiById[priorId]).toEqual(priorChat);
    view.unmount();
    mount();
    await settleLazyModules();
    expect(prompts).toHaveLength(1);
    expect((await bootstrap()).tabs).toEqual(recovered.tabs);
  },
);

it("recovers after restoring an archived chat without losing either composer or creating twice", async () => {
  const state = useWorkbenchStore.getState();
  const archivedId = state.activeTabId!;
  const owner = state.captureChatOwnership(archivedId)!;
  state.setChatComposer(owner, "Archived chat's own text");
  state.setChatHarnessSelection(owner, {
    harnessId: otherHarness.id,
    modelId: "model-b",
  });
  await state.createChat();
  expect(await useWorkbenchStore.getState().archiveChat(archivedId)).toBe(true);
  useWorkbenchStore.setState({
    harnesses: [harness, otherHarness],
    selectedHarnessId: harness.id,
    selectedModelId: "model-a",
    models,
    harnessDiscoveryPhase: "ready",
  });
  const archived = useWorkbenchStore.getState().chatUiById[archivedId];
  const before = await ports.project.getBootstrapSnapshot();
  vi.mocked(ports.chat.createChat).mockRejectedValueOnce(
    new Error("Creation response unavailable"),
  );
  const bootstrap = ports.project.getBootstrapSnapshot.bind(ports.project);
  const projection = vi
    .spyOn(ports.project, "getBootstrapSnapshot")
    .mockRejectedValue(new Error("Projection unavailable"));
  mount();
  await settleLazyModules();
  fireEvent.change(composer(), {
    target: { value: "Keep this unsaved draft" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() =>
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Chat creation is unconfirmed. Reload chats before trying again.",
    ),
  );
  projection.mockImplementation(async () =>
    withHarnessSnapshot(await bootstrap()),
  );
  await act(async () => {
    expect(await useWorkbenchStore.getState().restoreChat(archivedId)).toBe(
      true,
    );
  });
  await settleLazyModules();
  expect(useWorkbenchStore.getState().tabs).toHaveLength(
    before.tabs.length + 1,
  );
  expect(useWorkbenchStore.getState().chatUiById[archivedId]).toEqual(archived);
  expect(screen.getByRole("region", { name: "New chat draft" })).toBeVisible();
  expect(composer()).toHaveValue("Keep this unsaved draft");
  expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Reload chats" }));
  await settleLazyModules();
  expect(useWorkbenchStore.getState().chatUiById[archivedId]).toEqual(archived);
  expect(composer()).toHaveValue("Keep this unsaved draft");
  expect(screen.getByRole("button", { name: "Send message" })).toBeEnabled();
  fireEvent.submit(screen.getByRole("form"));
  await waitFor(() => expect(prompts).toHaveLength(1));
  const after = await ports.project.getBootstrapSnapshot();
  expect(after.tabs).toHaveLength(before.tabs.length + 2);
  expect(prompts[0]).toMatchObject({
    chatId: after.activeTabId,
    prompt: "Keep this unsaved draft",
  });
  expect(after.activeTabId).not.toBe(archivedId);
  expect(useWorkbenchStore.getState().chatUiById[archivedId]).toEqual(archived);
});

it("explains non-account model discovery failure and retries without losing the draft", async () => {
  resetHarnessModelCache();
  vi.mocked(desktop.listHarnessModels).mockRejectedValueOnce(
    new Error("Model service unavailable"),
  );
  mount();
  await settleLazyModules();
  fireEvent.change(composer(), { target: { value: "Keep this model draft" } });
  fireEvent.click(screen.getByRole("button", { name: "Harness" }));
  fireEvent.click(await screen.findByRole("option", { name: /Other harness/ }));
  await settleLazyModules();
  expect(screen.getByRole("alert")).toHaveTextContent(
    "Model service unavailable",
  );
  expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Retry" }));
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Send message" })).toBeEnabled(),
  );
  expect(composer()).toHaveValue("Keep this model draft");
  expect(screen.queryByRole("alert")).toBeNull();
  expect(starts).toEqual([]);
});

it("keeps an intervening side chat's text and session while recovering the draft into its own chat", async () => {
  const before = await ports.project.getBootstrapSnapshot();
  vi.mocked(ports.chat.createChat).mockRejectedValueOnce(
    new Error("Creation response unavailable"),
  );
  const bootstrap = ports.project.getBootstrapSnapshot.bind(ports.project);
  const projection = vi
    .spyOn(ports.project, "getBootstrapSnapshot")
    .mockRejectedValue(new Error("Projection unavailable"));
  mount(false, true);
  await settleLazyModules();
  fireEvent.change(composer(), { target: { value: "Draft's own request" } });
  fireEvent.submit(screen.getByRole("form"));
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Reload chats" })).toBeEnabled(),
  );
  projection.mockImplementation(async () =>
    withHarnessSnapshot(await bootstrap()),
  );
  await act(async () => {
    await openSideChat(before.activeTabId!, {
      turnId: "turn",
      itemId: "item",
      text: "Quote",
      startOffset: 0,
      endOffset: 5,
    });
    const state = useWorkbenchStore.getState();
    const sideId = useSideChatState.getState().sides[0].id;
    state.setChatHarnessSelection(state.captureChatOwnership(sideId)!, {
      harnessId: otherHarness.id,
      modelId: "model-b",
    });
    await state.activateTab(sideId);
  });
  await settleLazyModules();
  const sideId = useSideChatState.getState().sides[0].id;
  fireEvent.change(composer(), { target: { value: "Side chat question" } });
  fireEvent.submit(screen.getByRole("form"));
  await waitFor(() => expect(prompts).toHaveLength(1));
  await waitFor(() =>
    expect(useWorkbenchStore.getState().chatUiById[sideId].sending).toBe(false),
  );
  fireEvent.change(composer(), {
    target: { value: "Side chat's next message" },
  });
  const sideUi = useWorkbenchStore.getState().chatUiById[sideId];
  const sidePage = structuredClone(pages.get(sideId));
  await act(async () => useWorkbenchStore.getState().openNewChatDraft());
  await settleLazyModules();
  expect(composer()).toHaveValue("Draft's own request");
  fireEvent.click(screen.getByRole("button", { name: "Reload chats" }));
  await settleLazyModules();
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Send message" })).toBeEnabled(),
  );
  expect(composer()).toHaveValue("Draft's own request");
  fireEvent.submit(screen.getByRole("form"));
  await waitFor(() => expect(prompts).toHaveLength(2));
  const after = await ports.project.getBootstrapSnapshot();
  expect(prompts[1]).toMatchObject({
    chatId: after.activeTabId,
    prompt: "Draft's own request",
  });
  expect(after.activeTabId).not.toBe(sideId);
  expect(after.tabs).toHaveLength(before.tabs.length + 2);
  expect(useWorkbenchStore.getState().chatUiById[sideId]).toEqual(sideUi);
  expect(pages.get(sideId)).toEqual(sidePage);
  expect(starts.map((request) => request.chatId)).toEqual([
    sideId,
    after.activeTabId,
  ]);
});

it.each(["open", "close"])(
  "recovers the created chat after an Editor tab %s without losing either composer",
  async (change) => {
    const state = useWorkbenchStore.getState();
    const priorId = state.activeTabId!;
    state.setChatComposer(
      state.captureChatOwnership(priorId)!,
      "Previous chat text",
    );
    const file = {
      id: "file:example",
      kind: "file" as const,
      title: "example.ts",
      position: 1,
      pinned: false,
      dirty: false,
      resource: {
        rootId: state.project!.roots[0].id,
        relativePath: "src/example.ts",
      },
    };
    if (change === "close") {
      await ports.project.persistProjectLayout(
        {
          projectId: state.project!.id,
          tabs: [...state.tabs, file],
          activeTabId: priorId,
          selectedRootId: state.selectedRootId,
          selectedResource: null,
        },
        state.nativeTransitionEpoch,
      );
      useWorkbenchStore.setState({ tabs: [...state.tabs, file] });
    }
    const create = vi.mocked(ports.chat.createChat).getMockImplementation()!;
    vi.mocked(ports.chat.createChat).mockImplementationOnce(
      async (...args) => ({ ...(await create(...args)), snapshot: null }),
    );
    const bootstrap = ports.project.getBootstrapSnapshot.bind(ports.project);
    const projection = vi
      .spyOn(ports.project, "getBootstrapSnapshot")
      .mockRejectedValue(new Error("Projection unavailable"));
    state.updateNewChatDraft({ text: "Recover my draft" });
    await state.sendNewChatDraft();
    const native = await bootstrap();
    const createdId = native.activeTabId!;
    expect(useWorkbenchStore.getState().newChatDraft?.submission.status).toBe(
      "unconfirmed",
    );
    projection.mockImplementation(async () =>
      withHarnessSnapshot(await bootstrap()),
    );
    await ports.project.persistProjectLayout(
      {
        projectId: native.project!.id,
        tabs:
          change === "open"
            ? [...native.tabs, { ...file, position: native.tabs.length }]
            : native.tabs.filter((tab) => tab.id !== file.id),
        activeTabId: change === "open" ? file.id : priorId,
        selectedRootId: native.selectedRootId,
        selectedResource: null,
      },
      native.transitionEpoch,
    );
    await state.reloadNewChatDraft();
    const recovered = useWorkbenchStore.getState();
    expect(recovered.newChatDraft).toBeNull();
    expect(recovered.activeTabId).toBe(createdId);
    expect(recovered.chatUiById[createdId].composerText).toBe(
      "Recover my draft",
    );
    expect(recovered.chatUiById[priorId].composerText).toBe(
      "Previous chat text",
    );
    const after = await bootstrap();
    expect(after.tabs.filter((tab) => tab.kind === "chat")).toHaveLength(2);
    expect(after.archivedChats).toEqual([]);
    expect(after.tabs.some((tab) => tab.kind === "file")).toBe(
      change === "open",
    );
  },
);

it("reattaches the draft's own archived creation instead of creating another chat", async () => {
  const state = useWorkbenchStore.getState();
  const priorId = state.activeTabId!;
  state.setChatComposer(
    state.captureChatOwnership(priorId)!,
    "Previous chat text",
  );
  state.updateNewChatDraft({ text: "Recover the same chat" });
  const create = vi.mocked(ports.chat.createChat).getMockImplementation()!;
  vi.mocked(ports.chat.createChat).mockImplementationOnce(async (...args) => ({
    ...(await create(...args)),
    snapshot: null,
  }));
  const bootstrap = ports.project.getBootstrapSnapshot.bind(ports.project);
  const projection = vi
    .spyOn(ports.project, "getBootstrapSnapshot")
    .mockRejectedValue(new Error("Projection unavailable"));
  await state.sendNewChatDraft();
  const native = await bootstrap();
  const createdId = native.activeTabId!;
  projection.mockImplementation(async () =>
    withHarnessSnapshot(await bootstrap()),
  );
  await ports.chat.archiveChat(
    {
      projectId: native.project!.id,
      chatId: createdId,
      currentLayout: {
        projectId: native.project!.id,
        tabs: native.tabs,
        activeTabId: createdId,
        selectedRootId: native.selectedRootId,
        selectedResource: native.selectedResource,
      },
    },
    native.transitionEpoch,
  );
  await state.reloadNewChatDraft();
  const recovered = useWorkbenchStore.getState();
  expect(recovered.newChatDraft).toBeNull();
  expect(recovered.activeTabId).toBe(createdId);
  expect(recovered.chatUiById[createdId].composerText).toBe(
    "Recover the same chat",
  );
  expect(recovered.chatUiById[priorId].composerText).toBe("Previous chat text");
  const after = await bootstrap();
  expect(after.tabs.filter((tab) => tab.kind === "chat")).toHaveLength(2);
  expect(after.archivedChats).toEqual([]);
});

it("lets an unconfirmed draft be edited and reloads its creation with the edited prompt", async () => {
  const state = useWorkbenchStore.getState();
  const before = await ports.project.getBootstrapSnapshot();
  const create = vi.mocked(ports.chat.createChat).getMockImplementation()!;
  vi.mocked(ports.chat.createChat).mockImplementationOnce(async (...args) => ({
    ...(await create(...args)),
    snapshot: null,
  }));
  const bootstrap = ports.project.getBootstrapSnapshot.bind(ports.project);
  const projection = vi
    .spyOn(ports.project, "getBootstrapSnapshot")
    .mockRejectedValue(new Error("Projection unavailable"));
  state.updateNewChatDraft({ text: "Original unconfirmed prompt" });
  await state.sendNewChatDraft();
  const created = await bootstrap();
  mount();
  await settleLazyModules();
  expect(composer()).toBeEnabled();
  fireEvent.change(composer(), {
    target: { value: "Edited while unconfirmed" },
  });
  expect(composer()).toHaveValue("Edited while unconfirmed");
  expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Reload chats" })).toBeEnabled();
  projection.mockImplementation(async () =>
    withHarnessSnapshot(await bootstrap()),
  );
  fireEvent.click(screen.getByRole("button", { name: "Reload chats" }));
  await waitFor(() => expect(prompts).toHaveLength(1));
  expect(prompts[0]).toMatchObject({
    chatId: created.activeTabId,
    prompt: "Edited while unconfirmed",
  });
  expect((await bootstrap()).tabs).toEqual(created.tabs);
  expect(created.tabs).toHaveLength(before.tabs.length + 1);
});

it("preserves the previous chat composer when Home reload overlaps draft reconciliation", async () => {
  const state = useWorkbenchStore.getState();
  const previousId = state.activeTabId!;
  state.setChatComposer(
    state.captureChatOwnership(previousId)!,
    "Previous chat's next prompt",
  );
  const create = vi.mocked(ports.chat.createChat).getMockImplementation()!;
  vi.mocked(ports.chat.createChat).mockImplementationOnce(async (...args) => ({
    ...(await create(...args)),
    snapshot: null,
  }));
  const bootstrap = ports.project.getBootstrapSnapshot.bind(ports.project);
  const projection = vi
    .spyOn(ports.project, "getBootstrapSnapshot")
    .mockRejectedValue(new Error("Projection unavailable"));
  state.updateNewChatDraft({ text: "Recover this draft" });
  await state.sendNewChatDraft();
  projection.mockImplementation(async () =>
    withHarnessSnapshot(await bootstrap()),
  );
  let release!: () => void;
  vi.spyOn(workspaceNotify, "bindWorkspaceNotifyFromState")
    .mockResolvedValue(undefined)
    .mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        }),
    );
  const reloading = state.reloadNewChatDraft();
  await waitFor(() => expect(release).toBeTypeOf("function"));
  await state.initialize();
  release();
  await reloading;
  expect(useWorkbenchStore.getState().chatUiById[previousId].composerText).toBe(
    "Previous chat's next prompt",
  );
  expect(useWorkbenchStore.getState().newChatDraft).toBeNull();
  expect(useWorkbenchStore.getState().composerText).toBe("Recover this draft");
});

it("keeps draft picker choices separate from the previous chat and its defaults", async () => {
  const state = useWorkbenchStore.getState();
  const priorId = state.activeTabId!;
  const selection = state.chatUiById[priorId];
  mount();
  await settleLazyModules();
  fireEvent.click(screen.getByRole("button", { name: "Access options" }));
  fireEvent.click(
    await screen.findByRole("menuitemradio", { name: "Ask each time" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: /Thinking level/ }),
  );
  fireEvent.change(
    await screen.findByRole("slider", { name: /Thinking level/ }),
    { target: { value: "2" } },
  );
  await settleLazyModules();
  expect(
    screen.getByRole("button", { name: "Access options" }),
  ).toHaveTextContent("Ask each time");
  expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
    access: "ask",
    effort: "high",
  });
  expect(useWorkbenchStore.getState().chatUiById[priorId]).toEqual(selection);
  expect(useSessionAccessModeStore.getState().modes).toEqual({});
  fireEvent.click(screen.getByRole("button", { name: "Model" }));
  fireEvent.click(await screen.findByRole("option", { name: /Model B/ }));
  expect(screen.queryByRole("button", { name: "Thinking level" })).toBeNull();
  expect(useWorkbenchStore.getState().newChatDraft?.effort).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Harness" }));
  fireEvent.click(await screen.findByRole("option", { name: /Other harness/ }));
  await settleLazyModules();
  expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
    harnessId: otherHarness.id,
    access: null,
    effort: null,
  });
  expect(useWorkbenchStore.getState().chatUiById[priorId]).toEqual(selection);
  expect(
    screen.getByRole("button", { name: "Access options" }),
  ).toHaveTextContent("Read only");
  expect(screen.queryByRole("button", { name: "Composer options" })).toBeNull();
  expect(screen.queryByRole("button", { name: /Attach/ })).toBeNull();
  fireEvent.change(composer(), { target: { value: "@file /command" } });
  await settleLazyModules();
  expect(screen.queryByRole("listbox")).toBeNull();
  await act(async () => state.activateTab(priorId));
  await settleLazyModules();
  expect(screen.getByRole("button", { name: "Model" })).toHaveAttribute(
    "data-selected-id",
    "model-a",
  );
});

it("keeps a confirmed chat recoverable after session-start failure and retries in that chat", async () => {
  const before = await ports.project.getBootstrapSnapshot();
  vi.mocked(acpDesktop.startSession).mockRejectedValueOnce(
    new AcpDesktopError({
      kind: "invalidRequest",
      message: "Session could not start",
    }),
  );
  mount();
  await settleLazyModules();
  fireEvent.change(composer(), { target: { value: "Retry in this chat" } });
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() =>
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Session could not start",
    ),
  );
  expect(composer()).toHaveValue("Retry in this chat");
  const created = await ports.project.getBootstrapSnapshot();
  expect(created.tabs.length).toBe(before.tabs.length + 1);
  await settleLazyModules();
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() => expect(prompts).toHaveLength(1));
  expect(prompts[0]?.chatId).toBe(created.activeTabId);
  expect((await ports.project.getBootstrapSnapshot()).tabs).toEqual(
    created.tabs,
  );
});

it("waits for the scoped page and never replays after leaving and reopening the confirmed chat", async () => {
  const previous = useWorkbenchStore.getState().activeTabId!;
  let release!: (page: AcpChatPage) => void;
  vi.mocked(acpDesktop.getChatPage).mockImplementation(async (request) =>
    request.chatId === previous
      ? emptyPage(request.projectId, request.chatId)
      : new Promise((resolve) => {
          release = resolve;
        }),
  );
  mount();
  await settleLazyModules();
  fireEvent.change(composer(), { target: { value: "Wait for my own page" } });
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() =>
    expect(useWorkbenchStore.getState().pendingFirstSend?.chatId).toBeTruthy(),
  );
  const state = useWorkbenchStore.getState();
  const createdId = state.pendingFirstSend!.chatId!;
  expect(prompts).toEqual([]);
  await act(async () => state.activateTab(previous));
  await act(async () => release(emptyPage(state.project!.id, createdId)));
  await act(async () => state.activateTab(createdId));
  vi.mocked(acpDesktop.getChatPage).mockImplementation(async (request) =>
    emptyPage(request.projectId, request.chatId),
  );
  await settleLazyModules();
  expect(composer()).toHaveValue("Wait for my own page");
  expect(prompts).toEqual([]);
  expect(starts).toEqual([]);
});

it("cancels a submission when navigation wins before the send code loads", async () => {
  const before = await ports.project.getBootstrapSnapshot();
  mount();
  await settleLazyModules();
  fireEvent.change(composer(), {
    target: { value: "Keep this draft for later" },
  });
  fireEvent.submit(screen.getByRole("form"));
  await act(async () =>
    useWorkbenchStore.getState().activateTab(before.activeTabId!),
  );
  await settleLazyModules();
  expect(prompts).toEqual([]);
  expect((await ports.project.getBootstrapSnapshot()).tabs).toEqual(
    before.tabs,
  );
  await act(async () => useWorkbenchStore.getState().openNewChatDraft());
  await settleLazyModules();
  expect(composer()).toHaveValue("Keep this draft for later");
  expect(composer()).toBeEnabled();
});

it("cancels a retained draft's retry when the user leaves and returns during its activation", async () => {
  const state = useWorkbenchStore.getState();
  const previousId = state.activeTabId!;
  const create = vi.mocked(ports.chat.createChat).getMockImplementation()!;
  vi.mocked(ports.chat.createChat).mockImplementationOnce(async (...args) => ({
    ...(await create(...args)),
    snapshot: null,
  }));
  const bootstrap = ports.project.getBootstrapSnapshot.bind(ports.project);
  const projection = vi
    .spyOn(ports.project, "getBootstrapSnapshot")
    .mockRejectedValue(new Error("Projection unavailable"));
  state.updateNewChatDraft({ text: "Retry only with my permission" });
  await state.sendNewChatDraft();
  const created = await bootstrap();
  projection.mockImplementation(async () => ({
    ...withHarnessSnapshot(await bootstrap()),
    activeTabId: previousId,
  }));
  let release!: () => void;
  vi.spyOn(workspaceNotify, "bindWorkspaceNotifyFromState")
    .mockResolvedValue(undefined)
    .mockResolvedValueOnce(undefined)
    .mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        }),
    );
  mount();
  await settleLazyModules();
  let reloading!: Promise<void>;
  act(() => {
    reloading = state.reloadNewChatDraft();
  });
  await waitFor(() => expect(release).toBeTypeOf("function"));
  await act(async () => state.activateTab(previousId));
  await act(async () => state.activateTab(created.activeTabId!));
  await act(async () => {
    release();
    await reloading;
  });
  await settleLazyModules();
  expect(composer()).toHaveValue("Retry only with my permission");
  expect(prompts).toEqual([]);
  expect(starts).toEqual([]);
  expect((await bootstrap()).tabs).toEqual(created.tabs);
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() => expect(prompts).toHaveLength(1));
  expect(prompts[0]).toMatchObject({
    chatId: created.activeTabId,
    prompt: "Retry only with my permission",
  });
});

async function createdDraft(
  patch: Parameters<
    ReturnType<typeof useWorkbenchStore.getState>["updateNewChatDraft"]
  >[0],
) {
  const previousId = (await ports.project.getBootstrapSnapshot()).activeTabId!;
  let release!: () => void;
  vi.spyOn(workspaceNotify, "bindWorkspaceNotifyFromState")
    .mockResolvedValue(undefined)
    .mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        }),
    );
  mount();
  await settleLazyModules();
  act(() => useWorkbenchStore.getState().updateNewChatDraft(patch));
  let sending!: Promise<void>;
  act(() => {
    sending = useWorkbenchStore.getState().sendNewChatDraft();
  });
  await waitFor(() => expect(release).toBeTypeOf("function"));
  const chatId = (await ports.project.getBootstrapSnapshot()).activeTabId!;
  const bootstrap = ports.project.getBootstrapSnapshot.bind(ports.project);
  vi.spyOn(ports.project, "getBootstrapSnapshot").mockImplementation(async () =>
    withHarnessSnapshot(await bootstrap()),
  );
  vi.spyOn(ports.project, "switchProject").mockRejectedValueOnce(
    new Error("Switch refused"),
  );
  await act(async () =>
    useWorkbenchStore.getState().switchProject("missing-project"),
  );
  await act(async () => {
    release();
    await sending;
  });
  await settleLazyModules();
  expect(useWorkbenchStore.getState().newChatDraft?.submission).toEqual({
    status: "created",
    chatId,
  });
  return { chatId, previousId };
}

const draftRegion = () =>
  screen.queryByRole("region", { name: "New chat draft" });

it("shows the draft, not its chat, while the chat it is creating is opened", async () => {
  let release!: () => void;
  vi.spyOn(workspaceNotify, "bindWorkspaceNotifyFromState")
    .mockResolvedValue(undefined)
    .mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        }),
    );
  mount();
  await settleLazyModules();
  fireEvent.change(composer(), { target: { value: "Draft prompt" } });
  let sending!: Promise<void>;
  act(() => {
    sending = useWorkbenchStore.getState().sendNewChatDraft();
  });
  await waitFor(() => expect(release).toBeTypeOf("function"));
  const chatId = (await ports.project.getBootstrapSnapshot()).activeTabId!;
  expect(useWorkbenchStore.getState().activeTabId).toBe(chatId);
  await settleLazyModules();
  expect(draftRegion()).toBeVisible();
  expect(composer()).toHaveValue("Draft prompt");
  expect(composer()).toBeDisabled();
  expect(
    useWorkbenchStore.getState().chatUiById[chatId]?.composerText ?? "",
  ).toBe("");
  await act(async () => {
    release();
    await sending;
  });
  await waitFor(() => expect(prompts).toHaveLength(1));
  expect(prompts[0]).toMatchObject({ chatId, prompt: "Draft prompt" });
});

it.each(["tab strip", "archived neighbour", "closed neighbour"] as const)(
  "shows the draft when its created chat is reached through the %s",
  async (path) => {
    const { chatId, previousId } = await createdDraft({
      text: "Owned prompt",
    });
    await act(async () => useWorkbenchStore.getState().activateTab(previousId));
    await settleLazyModules();
    expect(draftRegion()).toBeNull();
    await act(async () => {
      const state = useWorkbenchStore.getState();
      if (path === "tab strip") await state.activateTab(chatId);
      else if (path === "archived neighbour")
        await state.archiveChat(previousId);
      else state.closeTab(previousId);
    });
    await settleLazyModules();
    await waitFor(() =>
      expect(useWorkbenchStore.getState().activeTabId).toBe(chatId),
    );
    expect(draftRegion()).toBeVisible();
    expect(composer()).toHaveValue("Owned prompt");
    expect(screen.getByRole("alert")).toHaveTextContent(/retry/i);
    expect(
      useWorkbenchStore.getState().chatUiById[chatId]?.composerText ?? "",
    ).toBe("");
    expect(prompts).toEqual([]);
  },
);

it("shows the draft and its error when the handoff fails after opening its chat", async () => {
  const { chatId, previousId } = await createdDraft({ text: "Owned prompt" });
  await act(async () => useWorkbenchStore.getState().activateTab(previousId));
  const created = await ports.project.getBootstrapSnapshot();
  vi.mocked(ports.chat.activateChat).mockResolvedValueOnce({
    committed: false,
    snapshot: null,
    transitionEpoch: created.transitionEpoch,
    recoveryError: "Activation refused",
  });
  await act(async () => useWorkbenchStore.getState().openNewChatDraft());
  await act(async () => useWorkbenchStore.getState().sendNewChatDraft());
  await settleLazyModules();
  expect(draftRegion()).toBeVisible();
  expect(composer()).toHaveValue("Owned prompt");
  expect(screen.getByRole("alert")).toHaveTextContent(/retry/i);
  expect(useWorkbenchStore.getState().newChatDraft?.submission).toEqual({
    status: "created",
    chatId,
  });
  expect(prompts).toEqual([]);
});

it.each(["discarded", "emptied"] as const)(
  "opens the created chat normally once its draft is %s",
  async (release) => {
    const { chatId, previousId } = await createdDraft({ text: "Owned prompt" });
    if (release === "discarded")
      fireEvent.click(screen.getByRole("button", { name: "Discard draft" }));
    else fireEvent.change(composer(), { target: { value: "" } });
    await act(async () => useWorkbenchStore.getState().activateTab(previousId));
    await act(async () => useWorkbenchStore.getState().activateTab(chatId));
    await settleLazyModules();
    expect(draftRegion()).toBeNull();
    expect(screen.getByRole("region", { name: /^Chat pane/ })).toBeVisible();
    fireEvent.change(composer(), { target: { value: "Typed in the chat" } });
    expect(useWorkbenchStore.getState().chatUiById[chatId].composerText).toBe(
      "Typed in the chat",
    );
  },
);

it("hands a retried draft's model, access and effort to the chat it created", async () => {
  const { chatId } = await createdDraft({
    text: "Draft prompt",
    access: "ask",
    effort: "high",
  });
  await act(async () => useWorkbenchStore.getState().openNewChatDraft());
  await act(async () => useWorkbenchStore.getState().sendNewChatDraft());
  await waitFor(() => expect(prompts).toHaveLength(1));
  expect(prompts[0]).toMatchObject({ chatId, prompt: "Draft prompt" });
  expect(starts).toMatchObject([
    {
      chatId,
      bindingId: harness.bindingId,
      modelId: "model-a",
      effort: "high",
    },
  ]);
  expect(useWorkbenchStore.getState().chatUiById[chatId]).toMatchObject({
    harnessId: harness.id,
    modelId: "model-a",
  });
  expect(Object.values(useSessionAccessModeStore.getState().modes)).toContain(
    "ask",
  );
});

it("does not auto-send a handed-off prompt the user edits before the chat's page loads", async () => {
  const before = await ports.project.getBootstrapSnapshot();
  let releasePage!: () => void;
  const pageReady = new Promise<void>((resolve) => {
    releasePage = resolve;
  });
  const getPage = vi.mocked(acpDesktop.getChatPage).getMockImplementation()!;
  vi.mocked(acpDesktop.getChatPage).mockImplementation(async (request) => {
    if (!before.tabs.some((tab) => tab.id === request.chatId)) await pageReady;
    return getPage(request);
  });
  mount();
  await settleLazyModules();
  fireEvent.change(composer(), { target: { value: "Draft prompt" } });
  await act(async () => useWorkbenchStore.getState().sendNewChatDraft());
  await settleLazyModules();
  const chatId = useWorkbenchStore.getState().activeTabId!;
  expect(useWorkbenchStore.getState().pendingFirstSend).toMatchObject({
    chatId,
  });
  expect(composer()).toHaveValue("Draft prompt");
  fireEvent.change(composer(), { target: { value: "Draft prompt, reworded" } });
  await act(async () => {
    releasePage();
    await pageReady;
  });
  await settleLazyModules();
  expect(composer()).toHaveValue("Draft prompt, reworded");
  expect(prompts).toEqual([]);
  expect(useWorkbenchStore.getState().pendingFirstSend).toBeNull();
});

it.each([false, true])(
  "retries the same created chat after a refused project switch during watcher binding (reopen refused: %s)",
  async (refuseReopen) => {
    const before = await ports.project.getBootstrapSnapshot();
    let release!: () => void;
    vi.spyOn(workspaceNotify, "bindWorkspaceNotifyFromState")
      .mockResolvedValue(undefined)
      .mockImplementationOnce(
        () =>
          new Promise<void>((resolve) => {
            release = resolve;
          }),
      );
    mount();
    await settleLazyModules();
    fireEvent.change(composer(), { target: { value: "Keep my created chat" } });
    fireEvent.submit(screen.getByRole("form"));
    await waitFor(() => expect(release).toBeTypeOf("function"));
    const created = await ports.project.getBootstrapSnapshot();
    const epoch = useWorkbenchStore.getState().chatOwnershipEpoch;
    const bootstrap = ports.project.getBootstrapSnapshot.bind(ports.project);
    vi.spyOn(ports.project, "getBootstrapSnapshot").mockImplementation(
      async () => withHarnessSnapshot(await bootstrap()),
    );
    vi.spyOn(ports.project, "switchProject").mockRejectedValueOnce(
      new Error("Switch refused"),
    );
    await act(async () =>
      useWorkbenchStore.getState().switchProject("missing-project"),
    );
    expect(useWorkbenchStore.getState().chatOwnershipEpoch).toBeGreaterThan(
      epoch,
    );
    await act(async () => release());
    await act(async () => useWorkbenchStore.getState().openNewChatDraft());
    await settleLazyModules();
    expect(composer()).toBeEnabled();
    expect(composer()).toHaveValue("Keep my created chat");
    expect(screen.getByRole("alert")).toHaveTextContent(/retry/i);
    expect(prompts).toEqual([]);
    if (refuseReopen) {
      await act(async () =>
        useWorkbenchStore.getState().activateTab(before.activeTabId!),
      );
      await act(async () => useWorkbenchStore.getState().openNewChatDraft());
      vi.mocked(ports.chat.activateChat).mockResolvedValueOnce({
        committed: false,
        snapshot: null,
        transitionEpoch: created.transitionEpoch,
        recoveryError: "Activation refused",
      });
      await act(async () => useWorkbenchStore.getState().sendNewChatDraft());
      await act(async () => useWorkbenchStore.getState().openNewChatDraft());
      await settleLazyModules();
      expect(composer()).toBeEnabled();
      expect(composer()).toHaveValue("Keep my created chat");
      expect(screen.getByRole("alert")).toHaveTextContent(/retry/i);
      expect(prompts).toEqual([]);
    }
    fireEvent.change(composer(), {
      target: { value: "Edited after recovery" },
    });
    fireEvent.submit(screen.getByRole("form"));
    await waitFor(() => expect(prompts).toHaveLength(1));
    expect(prompts[0]).toMatchObject({
      chatId: created.activeTabId,
      prompt: "Edited after recovery",
    });
    expect((await ports.project.getBootstrapSnapshot()).tabs).toEqual(
      created.tabs,
    );
    expect(created.tabs).toHaveLength(before.tabs.length + 1);
    await settleLazyModules();
    expect(composer()).toHaveValue("");
    expect(prompts).toHaveLength(1);
  },
);

it("keeps effort selectable when the narrow toolbar exhausts its compaction steps", async () => {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      return {
        width: 100,
        height: 20,
        left: 0,
        right: this.classList.contains("composer__controls") ? 40 : 100,
        top: 0,
        bottom: 20,
        x: 0,
        y: 0,
        toJSON: () => ({}),
      };
    },
  );
  mount();
  await settleLazyModules();
  fireEvent.click(
    await screen.findByRole("button", { name: /Thinking level/ }),
  );
  const slider = await screen.findByRole("slider", { name: "Thinking level" });
  fireEvent.change(slider, { target: { value: "2" } });
  expect(slider).toHaveAttribute("aria-valuetext", "High");
  expect(useWorkbenchStore.getState().newChatDraft?.effort).toBe("high");
});

it("does not create a chat while the chosen catalog is unavailable", async () => {
  useWorkbenchStore.setState({ models: [], harnessDiscoveryPhase: "error" });
  const before = await ports.project.getBootstrapSnapshot();
  mount();
  await settleLazyModules();
  fireEvent.change(composer(), { target: { value: "Wait for the catalog" } });
  expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
  fireEvent.keyDown(composer(), { key: "Enter" });
  await settleLazyModules();
  expect((await ports.project.getBootstrapSnapshot()).tabs).toEqual(
    before.tabs,
  );
  expect(composer()).toHaveValue("Wait for the catalog");
  expect(prompts).toEqual([]);
});

it.each(["sign-in", "discovery retry"] as const)(
  "preserves draft text and selections through harness %s",
  async (recovery) => {
    const before = await ports.project.getBootstrapSnapshot();
    const accountHarness = { ...otherHarness, accountModelDiscovery: true };
    useWorkbenchStore.setState({ harnesses: [harness, accountHarness] });
    const discover = vi
      .spyOn(acpHarnessDesktop, "discoverCodex")
      .mockImplementation(async (request) => ({
        ...request,
        rootId: before.project!.roots[0].id,
        runtimeEpoch: "1" as never,
        account: "managedChatgpt",
        models: models.map((model) => ({ ...model, id: model.id as never })),
        defaultModelId: "model-a" as never,
      }));
    if (recovery === "sign-in") {
      const signedIn = discover.getMockImplementation()!;
      discover.mockImplementationOnce(async (request) => ({
        ...(await signedIn(request)),
        account: "signedOut",
        models: [],
        defaultModelId: null,
      }));
      vi.spyOn(acpHarnessDesktop, "startCodexAuth").mockImplementation(
        async (request) => ({
          ...request,
          phase: "signedIn",
          revision: "1",
        }),
      );
    } else {
      discover.mockRejectedValueOnce(
        new Error("Account discovery unavailable"),
      );
    }
    mount();
    await settleLazyModules();
    fireEvent.change(composer(), {
      target: { value: "Keep this account draft" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Harness" }));
    fireEvent.click(
      await screen.findByRole("option", { name: /Other harness/ }),
    );
    await settleLazyModules();
    fireEvent.click(screen.getByRole("button", { name: "Access options" }));
    fireEvent.click(
      await screen.findByRole("menuitemradio", { name: "Ask each time" }),
    );
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    expect(composer()).toHaveValue("Keep this account draft");
    if (recovery === "sign-in") {
      fireEvent.click(await screen.findByRole("button", { name: "Sign in" }));
      expect(screen.getByText(/official sign-in page/)).toBeVisible();
      expect(composer()).toHaveValue("Keep this account draft");
      fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    } else {
      expect(screen.getByRole("alert")).toHaveTextContent(
        "Account discovery unavailable",
      );
      fireEvent.click(await screen.findByRole("button", { name: "Retry" }));
    }
    await settleLazyModules();
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Send message" }),
      ).toBeEnabled(),
    );
    expect(composer()).toHaveValue("Keep this account draft");
    expect(screen.getByRole("button", { name: "Harness" })).toHaveTextContent(
      "Other harness",
    );
    expect(screen.getByRole("button", { name: "Model" })).toHaveAttribute(
      "data-selected-id",
      "model-a",
    );
    expect(
      screen.getByRole("button", { name: "Access options" }),
    ).toHaveTextContent("Ask each time");
    expect((await ports.project.getBootstrapSnapshot()).tabs).toEqual(
      before.tabs,
    );
    expect(starts).toEqual([]);
    expect(prompts).toEqual([]);
  },
);

it("starts the first session with the effort chosen in the unsaved draft", async () => {
  mount();
  await settleLazyModules();
  fireEvent.click(
    await screen.findByRole("button", { name: /Thinking level/ }),
  );
  fireEvent.change(
    await screen.findByRole("slider", { name: "Thinking level" }),
    { target: { value: "2" } },
  );
  fireEvent.keyDown(screen.getByRole("slider", { name: "Thinking level" }), {
    key: "Escape",
  });
  fireEvent.change(composer(), { target: { value: "Think carefully" } });
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() => expect(prompts).toHaveLength(1));
  expect(starts).toMatchObject([
    { effort: "high", modelId: "model-a", bindingId: harness.bindingId },
  ]);
  expect(composer()).toHaveValue("");
});

it("keeps the confirmed chat owned by the draft after an Editor round trip during creation, without sending", async () => {
  const before = await ports.project.getBootstrapSnapshot();
  const create = vi.mocked(ports.chat.createChat).getMockImplementation()!;
  let release!: () => void;
  vi.mocked(ports.chat.createChat).mockImplementationOnce(async (...args) => {
    await new Promise<void>((resolve) => {
      release = resolve;
    });
    return create(...args);
  });
  function Modes() {
    const [mode, setMode] = useState<"agent" | "editor">("agent");
    return (
      <>
        <button onClick={() => setMode("editor")}>Editor</button>
        <button onClick={() => setMode("agent")}>Agent</button>
        <EditorContinuityKeeper windowMode={mode} />
        {mode === "agent" ? (
          <AgentChatRuntimeHarness>
            <AgentChatPanes />
          </AgentChatRuntimeHarness>
        ) : null}
      </>
    );
  }
  registerWorkbenchSurfaces();
  render(<Modes />);
  await settleLazyModules();
  fireEvent.change(composer(), {
    target: { value: "Keep this confirmed request" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() => expect(release).toBeTypeOf("function"));
  fireEvent.click(screen.getByRole("button", { name: "Editor" }));
  await act(async () => release());
  await waitFor(() =>
    expect(useWorkbenchStore.getState().projectTransitioning).toBe(false),
  );
  fireEvent.click(screen.getByRole("button", { name: "Agent" }));
  await settleLazyModules();
  const created = await ports.project.getBootstrapSnapshot();
  expect(created.tabs.length).toBe(before.tabs.length + 1);
  expect(useWorkbenchStore.getState().newChatDraft?.submission).toMatchObject({
    status: "created",
  });
  expect(screen.getByRole("region", { name: "New chat draft" })).toBeVisible();
  expect(composer()).toHaveValue("Keep this confirmed request");
  expect(prompts).toEqual([]);
  expect(starts).toEqual([]);
});
