import { HarnessAccountMenu } from "../harness/HarnessSignInStatus";
import { useAgentSettingsState } from "../settings/agentSettingsState";
import { EditorContinuityKeeper } from "../../workbench/WorkbenchContinuity";
import { StrictMode, useState } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../../app/store";
import {
  pinAgentChatOwner,
  clearAgentChatOwner,
} from "../../app/agentChatOwner";
import * as workspaceNotify from "../../app/workspaceNotifyLifecycle";
import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../../app/acpProjectionRegistry";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import { useChatAccessDrafts } from "../../app/chatAccessDrafts";
import { useChatEffortDraft } from "../../app/chatEffortDraft";
import { useChatAccessStore } from "../../app/sessionAccessActions";
import type {
  AcpChatPage,
  StartAcpSessionRequest,
  SendAcpTurnRequest,
  AcpItem,
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
import { resetBrowserDesktopForTests } from "../../../tests/ui/simulator/browserDesktop";
import { settleLazyModules } from "../../test/settleLazyModules";
import { AgentChatRuntimeHarness } from "../../test/AgentChatRuntimeHarness";
import { registerWorkbenchSurfaces } from "../../workbench/registeredSurfaces";
import { AgentChatPanes } from "../../workbench/AgentChatPanes";
import { ChatSurface } from "./ChatSurface";
import { resetHarnessModelCache } from "../../app/harnessModelCache";
import { useInheritedStore } from "./inheritedComposerModelState";
import { LeftSidebar } from "../../workbench/LeftSidebar";
import { openSideChat } from "../../workbench/sideChatActions";
import { useSideChatState } from "../../workbench/sideChatState";

const nativeAccess = vi.hoisted(() => ({ refuse: false }));

// The browser runtime has no native chat access; this stands in for it.
vi.mock("../../ports/chatAccessChanges", () => ({
  chatAccessChanges: () => ({
    get: async () => null,
    set: async () => {
      if (nativeAccess.refuse) throw new Error("Chat access refused.");
      return null;
    },
    prepare: () => Promise.reject(new Error("Not confirmed in tests.")),
    grant: () => Promise.reject(new Error("Not confirmed in tests.")),
  }),
}));

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

it.each([
  ["unavailable", "Allow once", "allowPending", "permission"],
  ["removed", "Deny", "declinePending", "permission"],
  ["removed", "Stop turn", "cancelled", "permission"],
  ["unavailable", "Cancel turn", "cancelled", "none"],
  ["removed", "Submit", "allowPending", "question"],
] as const)(
  "keeps live controls after discovery reports the harness %s (%s)",
  async (discovery, action, expected, pending) => {
    const chatId = await existingSessionlessChat();
    const state = useWorkbenchStore.getState();
    const session = agentSessionFixture({
      bindingId: harness.bindingId as never,
      modelId: "model-a" as never,
      runtimeEpoch: "1" as never,
      state: "active",
      residency: "hot",
    });
    const permission = {
      id: "permission-a",
      projectId: state.project!.id,
      chatId,
      sessionId: session.id,
      turnId: "turn-a",
      runtimeEpoch: "1",
      ordinal: 0,
      revision: "1",
      origin: "primary",
      activityId: null,
      lifecycle: "streaming",
      createdAt: 1,
      updatedAt: 1,
      terminalAt: null,
      kind: "permissionRequest",
      toolState: "waitingForApproval",
      payload: {
        requestId: "request-a",
        state: "received",
        label: "Permission request",
        summary: "Run a command",
        category: "execute",
        offeredOptionKinds: ["allow_once", "reject_once"],
        ...(pending === "question"
          ? {
              userInput: {
                questions: [
                  {
                    id: "scope",
                    header: "Scope",
                    question: "Which files?",
                    allowsOther: false,
                    secret: false,
                    kind: "text",
                    multiSelect: false,
                    optional: false,
                  },
                ],
              },
            }
          : {}),
      },
    } as Extract<AcpItem, { kind: "permissionRequest" }>;
    const page: AcpChatPage = {
      ...emptyPage(state.project!.id, chatId),
      currentSessionId: session.id,
      sessions: [session],
      turns: [
        {
          id: permission.turnId,
          sessionId: session.id,
          runtimeEpoch: permission.runtimeEpoch,
          ordinal: 0,
          state: "running",
          revision: permission.revision,
          createdAt: 1,
          updatedAt: 1,
          terminalAt: null,
          items: pending === "none" ? [] : [permission],
          activities: [],
          browserOperations: [],
          browserOperationsCollapsed: 0,
        },
      ],
    };
    pages.set(chatId, page);
    const settle = (next: "allowPending" | "declinePending") => {
      pages.set(chatId, {
        ...page,
        projectionRevision: "2" as never,
        turns: page.turns.map((turn) => ({
          ...turn,
          items: [
            { ...permission, payload: { ...permission.payload, state: next } },
          ],
        })),
      });
      acpProjectionStoreFor(chatId).getState().requestReload();
    };
    vi.spyOn(acpDesktop, "resolvePermissionDecision").mockImplementation(
      async (request) => {
        const next =
          request.decision === "deny" ? "declinePending" : "allowPending";
        settle(next);
        return { ...request, state: next };
      },
    );
    vi.spyOn(acpDesktop, "answerUserInput").mockImplementation(
      async (request) => {
        if (request.answers[0]?.text !== "Only tests")
          throw new Error("Answer was lost.");
        settle("allowPending");
        return { ...request, decision: "answer", state: "allowPending" };
      },
    );
    vi.spyOn(acpDesktop, "cancelTurn").mockImplementation(async () => ({
      projection: {
        ...page,
        projectionRevision: "2" as never,
        turns: page.turns.map((turn) => ({
          ...turn,
          state: "cancelled",
          terminalAt: 2,
        })),
      },
    }));
    mount();
    await settleLazyModules();
    const stopLabel = pending === "none" ? "Cancel turn" : "Stop turn";
    expect(
      await screen.findByRole("button", { name: stopLabel }),
    ).toBeEnabled();
    await act(async () =>
      state.applyHarnessDiscovery(
        discovery === "removed"
          ? []
          : [{ ...harness, installState: "unavailable" }],
      ),
    );
    expect(screen.getByRole("button", { name: stopLabel })).toBeEnabled();
    if (pending === "question")
      fireEvent.input(screen.getByRole("textbox", { name: "Answer: Scope" }), {
        target: { value: "Only tests" },
      });
    fireEvent.click(screen.getByRole("button", { name: new RegExp(action) }));
    await waitFor(() => {
      const dock = document.querySelector<HTMLElement>(".chat-dock");
      expect(
        dock &&
          within(dock).queryByRole("group", { name: "Permission request" }),
      ).toBeNull();
      expect(
        dock && within(dock).queryByRole("group", { name: "Agent question" }),
      ).toBeNull();
      if (expected === "cancelled")
        expect(screen.queryByRole("button", { name: stopLabel })).toBeNull();
    });
    const result = acpProjectionStoreFor(chatId).getState().page!;
    if (expected === "cancelled") {
      expect(result.turns[0].state).toBe(expected);
      fireEvent.click(screen.getByRole("button", { name: "Harness" }));
      const picker = await screen.findByRole("dialog", { name: "Harness" });
      expect(await within(picker).findByRole("alert")).toHaveTextContent(
        "Draft harness can't start yet.",
      );
      expect(
        within(picker).getByRole("button", { name: "Continue in a new chat" }),
      ).toBeEnabled();
    } else {
      expect(result.turns[0].items[0].payload).toMatchObject({
        state: expected,
      });
      expect(screen.getByRole("button", { name: "Cancel turn" })).toBeEnabled();
      expect(composer()).toBeDisabled();
    }
  },
);
const composer = () => screen.getByRole("combobox", { name: "Message agent" });
// The thinking level is changed inside the Model popover.
async function openModelEffort() {
  fireEvent.click(screen.getByRole("button", { name: "Model" }));
  await screen.findByRole("dialog", { name: "Select model" });
  return screen.findByRole("slider", { name: "Thinking level" });
}

it("silently replaces an inherited unavailable harness in catalog order", async () => {
  const expected = "draft-harness";
  const unavailable = {
    ...harness,
    id: "removed-harness",
    installState: "unavailable" as const,
  };
  useWorkbenchStore.setState({
    agentCenter: "chat",
    newChatDraft: null,
    harnesses: [harness, otherHarness, unavailable],
    selectedHarnessId: unavailable.id,
    selectedModelId: "old-model",
    models: [],
  });
  await useWorkbenchStore.getState().openNewChatDraft();
  mount();
  await settleLazyModules();
  await waitFor(() =>
    expect(useWorkbenchStore.getState().selectedHarnessId).toBe(expected),
  );
  expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
    harnessId: expected,
    error: null,
  });
  expect(composer()).toHaveAttribute("placeholder", "Do anything");
  expect(screen.queryByRole("region", { name: "Composer notices" })).toBeNull();
  expect(
    screen.queryByText(
      /can't start this harness|reviewed local binding|selected harness is unavailable/,
    ),
  ).toBeNull();
});

it("uses an available harness for a draft after detecting bootstrap harnesses settle", async () => {
  const snapshot = await ports.project.getBootstrapSnapshot();
  vi.spyOn(ports.project, "getBootstrapSnapshot").mockResolvedValue({
    ...snapshot,
    harnesses: [harness, otherHarness].map((candidate) => ({
      ...candidate,
      installState: "detecting",
      bindingId: null,
    })),
    selectedHarnessId: harness.id,
    selectedModelId: null,
  });
  useWorkbenchStore.setState({ newChatDraft: null });
  await useWorkbenchStore.getState().initialize();
  expect(
    useWorkbenchStore
      .getState()
      .harnesses.map((candidate) => candidate.installState),
  ).toEqual(["detecting", "detecting"]);
  // The restored harness is held while it is detected...
  expect(useWorkbenchStore.getState().selectedHarnessId).toBe(harness.id);
  await useWorkbenchStore
    .getState()
    .applyHarnessDiscovery([
      { ...harness, installState: "unavailable" },
      otherHarness,
    ]);
  // ...and gives way once discovery found it unavailable.
  expect(useWorkbenchStore.getState().selectedHarnessId).toBe(otherHarness.id);
  await useWorkbenchStore.getState().openNewChatDraft();
  expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
    harnessId: otherHarness.id,
    error: null,
  });
});

it("keeps a native refusal visible when the frontend catalog is still available", async () => {
  const chatId = await existingSessionlessChat();
  const state = useWorkbenchStore.getState();
  const session = agentSessionFixture({
    bindingId: harness.bindingId as never,
    state: "closed",
    runtimeEpoch: null,
    residency: "cold",
  });
  pages.set(chatId, {
    ...emptyPage(state.project!.id, chatId),
    sessions: [session],
  });
  useWorkbenchStore.setState({ agentCenter: "chat", newChatDraft: null });
  state.setChatActionState(state.captureChatOwnership(chatId)!, {
    sending: false,
    error: "The project root is no longer available.",
    errorKind: "unavailable",
    errorUnavailableReason: "bindingUnavailable",
  });
  mount();
  await settleLazyModules();
  expect(
    screen.getAllByText("The project root is no longer available."),
  ).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "Harness" }));
  await screen.findByRole("dialog", { name: "Harness" });
  expect(
    screen.getAllByText("The project root is no longer available."),
  ).toHaveLength(1);
  expect(useWorkbenchStore.getState().harnesses).toEqual([
    harness,
    otherHarness,
  ]);
});

it("explains an unavailable draft in the picker and opens harness settings", async () => {
  useWorkbenchStore.setState({
    agentCenter: "chat",
    newChatDraft: null,
    harnesses: [{ ...harness, installState: "unavailable" }],
    selectedHarnessId: harness.id,
    models: [],
  });
  await useWorkbenchStore.getState().openNewChatDraft();
  mount();
  await settleLazyModules();
  fireEvent.click(screen.getByRole("button", { name: "Harness" }));
  const notice = await screen.findByRole("alert");
  expect(screen.getAllByRole("alert")).toHaveLength(1);
  expect(notice).toHaveTextContent("Choose a harness.");
  fireEvent.click(
    within(notice).getByRole("button", { name: "Open Settings" }),
  );
  expect(useAgentSettingsState.getState().page).toBe("Harnesses");
  useAgentSettingsState.getState().close();
  expect(
    screen.getByPlaceholderText(
      "Choose an available harness before starting a session",
    ),
  ).toBeDisabled();
  expect(
    screen.queryByText(
      /can't start this harness|reviewed local binding|selected harness is unavailable/,
    ),
  ).toBeNull();
});

it.each([
  ["bindingUnavailable", false],
  ["capabilityUnavailable", false],
  ["bindingUnavailable", true],
] as const)(
  "keeps unavailable chat history, errors and project on recovery (%s, projectless: %s)",
  async (reason, projectless) => {
    const chatId = await existingSessionlessChat();
    const originalProjectId = useWorkbenchStore.getState().project!.id;
    if (projectless) await useWorkbenchStore.getState().setChatProject(null);
    const state = useWorkbenchStore.getState();
    const targetProjectId = projectless ? null : state.project!.id;
    const unavailable = { ...harness, installState: "unavailable" as const };
    const session = agentSessionFixture({
      bindingId:
        harness.bindingId as AcpChatPage["sessions"][number]["bindingId"],
      state: "closed",
      runtimeEpoch: null,
      residency: "cold",
    });
    pages.set(chatId, {
      ...emptyPage(state.project!.id, chatId),
      sessions: [session],
    });
    await state.openNewChatDraft(projectless ? originalProjectId : null);
    useWorkbenchStore.setState({
      harnesses: [unavailable, otherHarness],
      selectedHarnessId: harness.id,
      agentCenter: "chat",
      error: "Project refresh failed.",
    });
    state.setChatActionState(state.captureChatOwnership(chatId)!, {
      sending: false,
      error: "Native availability failure.",
      errorKind: "unavailable",
      errorUnavailableReason: reason,
    });
    mount();
    await settleLazyModules();
    fireEvent.click(screen.getByRole("button", { name: "Harness" }));
    const picker = await screen.findByRole("dialog", { name: "Harness" });
    expect(await within(picker).findByRole("alert")).toHaveTextContent(
      "Draft harness can't start yet.",
    );
    expect(screen.getAllByText("Draft harness can't start yet.")).toHaveLength(
      1,
    );
    expect(screen.getByText("Project refresh failed.")).toBeVisible();
    expect(screen.getByText("Native availability failure.")).toBeVisible();
    expect(
      screen.queryByText(
        /can't start this harness|reviewed local binding|selected harness is unavailable/,
      ),
    ).toBeNull();
    expect(
      screen.getByPlaceholderText(
        "Choose an available harness before starting a session",
      ),
    ).toBeDisabled();
    fireEvent.click(
      within(picker).getByRole("button", { name: "Continue in a new chat" }),
    );
    await waitFor(() =>
      expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
        targetProjectId,
        harnessId: otherHarness.id,
        error: null,
      }),
    );
    expect(pages.get(chatId)?.sessions[0].bindingId).toBe(harness.bindingId);
    fireEvent.change(composer(), {
      target: { value: "Continue work with the available harness" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => expect(prompts).toHaveLength(1));
    const snapshot = await ports.project.getBootstrapSnapshot();
    const newChatId = prompts[0].chatId;
    expect(newChatId).not.toBe(chatId);
    expect(snapshot.tabs).toContainEqual(
      expect.objectContaining({ id: newChatId, kind: "chat" }),
    );
    if (projectless) {
      expect(snapshot.project?.isInternal).toBe(true);
      expect(
        useWorkbenchStore
          .getState()
          .unscopedChats.flatMap((scope) => scope.railChats ?? [])
          .map((chat) => chat.id),
      ).toContain(newChatId);
    } else {
      expect(snapshot.project?.id).toBe(targetProjectId);
    }
    expect(prompts[0]).toMatchObject({
      projectId: snapshot.project!.id,
      prompt: "Continue work with the available harness",
    });
    if (projectless) {
      // The browser fixture reset retains root selection across tests.
      cleanup();
      await useWorkbenchStore.getState().switchProject(originalProjectId);
    }
  },
);

async function existingSessionlessChat() {
  const chatId = useWorkbenchStore
    .getState()
    .tabs.find((tab) => tab.kind === "chat")!.id;
  await useWorkbenchStore.getState().activateTab(chatId);
  const state = useWorkbenchStore.getState();
  state.setChatHarnessSelection(state.captureChatOwnership(chatId)!, {
    harnessId: harness.id,
    modelId: null,
  });
  useWorkbenchStore.setState({ selectedModelId: null });
  return chatId;
}

beforeEach(async () => {
  nativeAccess.refuse = false;
  useSideChatState.setState({ sides: [], pairedId: null, pendingClose: null });
  resetBrowserDesktopForTests();
  useWorkbenchStore.setState(initialState, true);
  await useWorkbenchStore.getState().initialize();
  useSessionAccessModeStore.setState({ ready: true, modes: {}, error: null });
  useChatAccessDrafts.setState({ drafts: {} });
  useChatEffortDraft.setState({ draft: null });
  useInheritedStore.setState({}, true);
  useChatAccessStore.setState({ modes: {}, error: null });
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
  const chooseProject = ports.chat.setChatProject.bind(ports.chat);
  vi.spyOn(ports.chat, "setChatProject").mockImplementation(async (...args) =>
    withHarnessCatalog(await chooseProject(...args)),
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
  await useWorkbenchStore
    .getState()
    .openNewChatDraft(useWorkbenchStore.getState().project!.id);
});
afterEach(() => {
  cleanup();
  clearAgentChatOwner();
  clearAcpProjectionRegistry();
  useSideChatState.setState({ sides: [], pairedId: null, pendingClose: null });
  vi.restoreAllMocks();
  useChatAccessDrafts.setState({ drafts: {} });
  useChatEffortDraft.setState({ draft: null });
  useInheritedStore.setState({}, true);
  useWorkbenchStore.setState(initialState, true);
  resetBrowserDesktopForTests();
});

it.each([
  "Codex discovery is already running; retry it.",
  "Account unavailable.",
])(
  "says in one visible line, with Retry, why a failed discovery blocks the Home draft (%s)",
  async (reason) => {
    useWorkbenchStore.setState({
      models: [],
      harnessDiscoveryPhase: "error",
      harnessDiscoveryError: reason,
    });
    mount();
    await settleLazyModules();
    fireEvent.change(composer(), { target: { value: "Keep my first prompt" } });
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    expect(screen.getAllByText(reason)).toHaveLength(1);
    // The one visible line is the draft notice, with a Retry that asks again.
    expect(screen.getByRole("button", { name: "Retry" })).toBeEnabled();
    fireEvent.keyDown(composer(), { key: "Enter" });
    expect(prompts).toEqual([]);
    expect(composer()).toHaveValue("Keep my first prompt");
    act(() =>
      useWorkbenchStore.setState({
        models,
        harnessDiscoveryPhase: "ready",
        harnessDiscoveryError: null,
      }),
    );
    expect(screen.queryByText(reason)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Send message" })).toBeEnabled();
  },
);

it("labels a session-less existing chat with its default and starts with the model picked before sending", async () => {
  const chatId = await existingSessionlessChat();
  useWorkbenchStore.setState({ models: [...models].reverse() });
  mount();
  await settleLazyModules();
  const model = screen.getByRole("button", { name: "Model" });
  expect(model).toHaveTextContent(/^Model ADefault$/);
  expect(model).toBeEnabled();
  fireEvent.click(model);
  fireEvent.click(await screen.findByRole("option", { name: /Model B/ }));
  expect(model).toHaveTextContent(/^Model B$/);
  fireEvent.change(composer(), { target: { value: "Use my chosen model" } });
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() => expect(prompts).toHaveLength(1));
  expect(starts).toMatchObject([
    { chatId, bindingId: harness.bindingId, modelId: "model-b" },
  ]);
  expect(prompts[0]).toMatchObject({ chatId, prompt: "Use my chosen model" });
});

it("replaces a pre-session label with the harness's confirmed model even when it differs from the request", async () => {
  const chatId = useWorkbenchStore
    .getState()
    .tabs.find((tab) => tab.kind === "chat")!.id;
  await useWorkbenchStore.getState().activateTab(chatId);
  const start = vi.mocked(acpDesktop.startSession).getMockImplementation()!;
  vi.mocked(acpDesktop.startSession).mockImplementation(async (request) => {
    const result = await start(request);
    result.projection.sessionConfig = {
      sageAgentSessionId: result.projection.currentSessionId!,
      runtimeEpoch: "1" as never,
      configRevision: "1" as never,
      confirmedModel: { id: "model-b", name: "Model B" },
      modes: null,
      configOptions: [],
    };
    return result;
  });
  mount();
  await settleLazyModules();
  expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
    /^Model ADefault$/,
  );
  fireEvent.change(composer(), {
    target: { value: "Confirm the effective model" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() => expect(prompts).toHaveLength(1));
  expect(starts).toMatchObject([{ chatId, modelId: "model-a" }]);
  expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
    /^Model B$/,
  );
});

it("starts an existing session-less chat with its inherited model before falling back to the catalog default", async () => {
  const chatId = await existingSessionlessChat();
  useInheritedStore.setState(
    { [chatId]: { id: "model-b", model: "Model B", effort: null } },
    true,
  );
  mount();
  await settleLazyModules();
  expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
    /^Model B$/,
  );
  fireEvent.change(composer(), {
    target: { value: "Keep the inherited model" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() => expect(prompts).toHaveLength(1));
  expect(starts).toMatchObject([{ chatId, modelId: "model-b" }]);
});

it.each(["checking", "idle"] as const)(
  "keeps a session-less chat's model pending while its catalog loads (%s)",
  async (phase) => {
    await existingSessionlessChat();
    useWorkbenchStore.setState({
      selectedModelId: null,
      models: [],
      harnessDiscoveryPhase: phase,
    });
    mount();
    await settleLazyModules();
    const model = screen.getByRole("button", { name: "Model" });
    expect(model).toHaveTextContent("Loading");
    expect(model).toBeDisabled();
    act(() =>
      useWorkbenchStore.setState({ models, harnessDiscoveryPhase: "ready" }),
    );
    expect(model).toHaveTextContent(/^Model ADefault$/);
    expect(model).toBeEnabled();
  },
);

it("shares a pinned chat's rediscovered default, effort and access after a catalog reset with its first native start", async () => {
  const chatId = await existingSessionlessChat();
  resetHarnessModelCache();
  useWorkbenchStore.setState({
    selectedHarnessId: otherHarness.id,
    models: [],
  });
  let publish!: (value: typeof models) => void;
  let publishOld!: (value: typeof models) => void;
  const oldCatalog = new Promise<typeof models>((resolve) => {
    publishOld = resolve;
  });
  const nextCatalog = new Promise<typeof models>((resolve) => {
    publish = resolve;
  });
  vi.mocked(desktop.listHarnessModels)
    .mockReturnValueOnce(oldCatalog)
    .mockReturnValue(nextCatalog);
  mount();
  await settleLazyModules();
  const model = screen.getByRole("button", { name: "Model" });
  expect(model).toHaveTextContent("Loading");
  expect(model).toBeDisabled();
  act(() => resetHarnessModelCache());
  await settleLazyModules();
  await act(async () =>
    publishOld([{ ...models[0], id: "old-model", name: "Old account model" }]),
  );
  expect(model).toHaveTextContent("Loading");
  await act(async () => publish([...models].reverse()));
  await settleLazyModules();
  await waitFor(() => expect(model).toHaveTextContent(/^Model ADefault$/));
  expect(model).toBeEnabled();
  fireEvent.change(await openModelEffort(), { target: { value: "2" } });
  fireEvent.keyDown(screen.getByRole("slider", { name: "Thinking level" }), {
    key: "Escape",
  });
  fireEvent.click(screen.getByRole("button", { name: "Access options" }));
  fireEvent.click(
    await screen.findByRole("menuitemradio", { name: "Ask each time" }),
  );
  fireEvent.change(composer(), {
    target: { value: "Use the discovered default" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() => expect(prompts).toHaveLength(1));
  expect(starts).toMatchObject([
    {
      chatId,
      bindingId: harness.bindingId,
      modelId: "model-a",
      effort: "high",
    },
  ]);
  expect(
    useChatAccessStore.getState().modes[pages.get(chatId)!.currentSessionId!],
  ).toBe("ask");
});

it("drops the inherited model when switching a session-less chat to a harness with no catalog", async () => {
  const chatId = await existingSessionlessChat();
  useInheritedStore.setState(
    { [chatId]: { id: "model-b", model: "Model B", effort: null } },
    true,
  );
  vi.mocked(desktop.listHarnessModels).mockResolvedValue([]);
  mount();
  await settleLazyModules();
  expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
    /^Model B$/,
  );
  fireEvent.click(screen.getByRole("button", { name: "Harness" }));
  fireEvent.click(await screen.findByRole("option", { name: /Other harness/ }));
  await settleLazyModules();
  const model = screen.getByRole("button", { name: "Model" });
  expect(model).toHaveTextContent(/^Model$/);
  expect(model).toBeDisabled();
  fireEvent.change(composer(), {
    target: { value: "Use the new harness default" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() => expect(prompts).toHaveLength(1));
  expect(starts).toMatchObject([
    { chatId, bindingId: otherHarness.bindingId, modelId: null },
  ]);
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

it("shows where the unsaved draft will run on a tab above its composer", async () => {
  mount();
  await settleLazyModules();
  const projectName = useWorkbenchStore.getState().project!.name;
  const draft = screen.getByRole("region", { name: "New chat draft" });
  const strip = within(draft).getByRole("group", { name: "Chat location" });
  expect(strip).toHaveTextContent(`This computer${projectName}`);
  expect(strip.nextElementSibling).toBe(draft.querySelector(".chat-dock"));
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
    nativeAccess.refuse = accessRefused;
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
  fireEvent.click(screen.getByRole("button", { name: "Harness" }));
  const picker = await screen.findByRole("dialog", { name: "Harness" });
  expect(await within(picker).findByRole("alert")).toHaveTextContent(
    "Model service unavailable",
  );
  expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
  fireEvent.click(within(picker).getByRole("button", { name: "Retry" }));
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
  await act(async () =>
    useWorkbenchStore
      .getState()
      .openNewChatDraft(useWorkbenchStore.getState().project!.id),
  );
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
  expect(screen.getByRole("button", { name: "Discard prompt" })).toBeEnabled();
  const catalog = useWorkbenchStore.getState().harnesses;
  act(() =>
    useWorkbenchStore.setState({
      harnesses: catalog.map((item) => ({
        ...item,
        installState: "unavailable",
      })),
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Harness" }));
  const picker = await screen.findByRole("dialog", { name: "Harness" });
  expect(await within(picker).findByRole("alert")).toHaveTextContent(
    "Draft harness can't start yet.",
  );
  expect(screen.getByText(/Chat creation is unconfirmed/)).toBeVisible();
  expect(screen.getByRole("button", { name: "Reload chats" })).toBeEnabled();
  expect(composer()).toBeDisabled();
  expect(composer()).toHaveValue("Edited while unconfirmed");
  fireEvent.keyDown(picker, { key: "Escape" });
  act(() => useWorkbenchStore.setState({ harnesses: catalog }));
  expect(composer()).toHaveValue("Edited while unconfirmed");
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
  fireEvent.change(await openModelEffort(), { target: { value: "2" } });
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
  // The model list shares the still-open popover with the level.
  fireEvent.click(await screen.findByRole("option", { name: /Model B/ }));
  expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent(
    /^Model B$/,
  );
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
  await act(async () =>
    useWorkbenchStore
      .getState()
      .openNewChatDraft(useWorkbenchStore.getState().project!.id),
  );
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
  await act(async () =>
    useWorkbenchStore
      .getState()
      .openNewChatDraft(useWorkbenchStore.getState().project!.id),
  );
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

it("retargets a created draft without losing its unsent prompt on global New chat", async () => {
  const originalProjectId = useWorkbenchStore.getState().project!.id;
  const { chatId } = await createdDraft({ text: "Owned prompt" });
  render(<LeftSidebar />);
  fireEvent.click(screen.getByRole("button", { name: "New chat" }));
  await settleLazyModules();
  expect(useWorkbenchStore.getState().newChatDraft).toMatchObject({
    targetProjectId: null,
    text: "Owned prompt",
    submission: { status: "created", chatId },
  });
  expect(screen.getByRole("button", { name: "Choose project" })).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() => expect(prompts).toHaveLength(1));
  expect(prompts[0]).toMatchObject({ chatId, prompt: "Owned prompt" });
  expect(useWorkbenchStore.getState().project?.isInternal).toBe(true);
  await act(async () =>
    useWorkbenchStore.getState().switchProject(originalProjectId),
  );
  expect(useWorkbenchStore.getState().tabs).not.toContainEqual(
    expect.objectContaining({ id: chatId }),
  );
});

it("opens the created chat normally once its draft is discarded", async () => {
  const { chatId, previousId } = await createdDraft({ text: "Owned prompt" });
  fireEvent.click(screen.getByRole("button", { name: "Discard prompt" }));
  await act(async () => useWorkbenchStore.getState().activateTab(previousId));
  await act(async () => useWorkbenchStore.getState().activateTab(chatId));
  await settleLazyModules();
  expect(draftRegion()).toBeNull();
  expect(screen.getByRole("region", { name: /^Chat pane/ })).toBeVisible();
  fireEvent.change(composer(), { target: { value: "Typed in the chat" } });
  expect(useWorkbenchStore.getState().chatUiById[chatId].composerText).toBe(
    "Typed in the chat",
  );
});

it("keeps the draft shown for its chat while its text is cleared and retyped", async () => {
  const { chatId, previousId } = await createdDraft({ text: "Owned prompt" });
  await act(async () => useWorkbenchStore.getState().activateTab(previousId));
  await act(async () => useWorkbenchStore.getState().activateTab(chatId));
  await settleLazyModules();
  const field = composer();
  fireEvent.change(field, { target: { value: "" } });
  await settleLazyModules();
  expect(draftRegion()).toBeVisible();
  expect(composer()).toBe(field);
  fireEvent.change(field, { target: { value: "Retyped" } });
  expect(useWorkbenchStore.getState().newChatDraft?.text).toBe("Retyped");
  expect(
    screen.getByRole("button", { name: "Discard prompt" }),
  ).toHaveAccessibleDescription("The chat it created stays in your chats.");
  expect(
    useWorkbenchStore.getState().chatUiById[chatId]?.composerText ?? "",
  ).toBe("");
});

it("never re-owns a chat once its draft is discarded and a new draft is typed", async () => {
  const { chatId } = await createdDraft({ text: "Owned prompt" });
  fireEvent.click(screen.getByRole("button", { name: "Discard prompt" }));
  await act(async () => useWorkbenchStore.getState().activateTab(chatId));
  await settleLazyModules();
  fireEvent.change(composer(), { target: { value: "Kept in the chat" } });
  await act(async () =>
    useWorkbenchStore
      .getState()
      .openNewChatDraft(useWorkbenchStore.getState().project!.id),
  );
  await settleLazyModules();
  fireEvent.change(composer(), { target: { value: "A new chat" } });
  expect(useWorkbenchStore.getState().newChatDraft?.submission).toEqual({
    status: "released",
  });
  await act(async () => useWorkbenchStore.getState().sendNewChatDraft());
  await waitFor(() => expect(prompts).toHaveLength(1));
  expect(prompts[0].chatId).not.toBe(chatId);
  expect(useWorkbenchStore.getState().chatUiById[chatId].composerText).toBe(
    "Kept in the chat",
  );
});

it("keeps the draft in place of its chat while a non-chat tab is active", async () => {
  const { chatId, previousId } = await createdDraft({ text: "Owned prompt" });
  await act(async () => useWorkbenchStore.getState().activateTab(previousId));
  await act(async () => useWorkbenchStore.getState().activateTab(chatId));
  await settleLazyModules();
  expect(draftRegion()).toBeVisible();
  act(() => {
    const { tabs } = useWorkbenchStore.getState();
    useWorkbenchStore.setState({
      tabs: [
        ...tabs,
        {
          id: "file-1",
          kind: "file",
          title: "File",
          position: tabs.length,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "file-1",
    });
  });
  await settleLazyModules();
  expect(draftRegion()).toBeVisible();
  expect(composer()).toHaveValue("Owned prompt");
  expect(screen.queryByRole("region", { name: /^Chat pane/ })).toBeNull();
});

it("refuses a Chat Handoff into the chat its draft owns", async () => {
  const { chatId, previousId } = await createdDraft({ text: "Owned prompt" });
  await act(async () => useWorkbenchStore.getState().activateTab(previousId));
  const state = useWorkbenchStore.getState();
  expect(state.captureChatOwnership(chatId)).toBeNull();
  let committed!: boolean;
  await act(async () => {
    committed = await state.handoffChat({
      projectId: state.project!.id,
      projectGeneration: state.projectGeneration,
      transitionEpoch: state.nativeTransitionEpoch,
      sourceChatId: previousId,
      targetChatId: chatId,
      draft: "Handoff text",
    });
  });
  expect(committed).toBe(false);
  expect(
    useWorkbenchStore.getState().chatUiById[chatId]?.composerText ?? "",
  ).toBe("");
  await act(async () => useWorkbenchStore.getState().sendNewChatDraft());
  await waitFor(() => expect(prompts).toHaveLength(1));
  expect(prompts[0]).toMatchObject({ chatId, prompt: "Owned prompt" });
});

it("releases the draft when its created chat is deleted, and sends into a new chat", async () => {
  const { chatId } = await createdDraft({ text: "Owned prompt" });
  const archive = ports.chat.archiveChat.bind(ports.chat);
  vi.spyOn(ports.chat, "archiveChat").mockImplementation(async (...args) =>
    withHarnessCatalog(await archive(...args)),
  );
  const remove = ports.chat.deleteChat.bind(ports.chat);
  vi.spyOn(ports.chat, "deleteChat").mockImplementation(async (...args) =>
    withHarnessCatalog(await remove(...args)),
  );
  await act(async () => {
    await useWorkbenchStore.getState().archiveChat(chatId);
    expect(await useWorkbenchStore.getState().deleteChat(chatId)).toBe(true);
  });
  expect(useWorkbenchStore.getState().archivedChats).not.toContainEqual(
    expect.objectContaining({ id: chatId }),
  );
  await act(async () =>
    useWorkbenchStore
      .getState()
      .openNewChatDraft(useWorkbenchStore.getState().project!.id),
  );
  await settleLazyModules();
  expect(screen.queryByRole("button", { name: "Discard prompt" })).toBeNull();
  await act(async () => useWorkbenchStore.getState().sendNewChatDraft());
  await waitFor(() => expect(prompts).toHaveLength(1));
  expect(prompts[0].chatId).not.toBe(chatId);
  expect(prompts[0].prompt).toBe("Owned prompt");
});

it("hands a retried draft's model, access and effort to the chat it created", async () => {
  const { chatId } = await createdDraft({
    text: "Draft prompt",
    access: "ask",
    effort: "high",
  });
  await act(async () =>
    useWorkbenchStore
      .getState()
      .openNewChatDraft(useWorkbenchStore.getState().project!.id),
  );
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
  const sessionId = pages.get(chatId)!.currentSessionId!;
  expect(useChatAccessStore.getState().modes[sessionId]).toBe("ask");
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
    await act(async () =>
      useWorkbenchStore
        .getState()
        .openNewChatDraft(useWorkbenchStore.getState().project!.id),
    );
    await settleLazyModules();
    expect(composer()).toBeEnabled();
    expect(composer()).toHaveValue("Keep my created chat");
    expect(screen.getByRole("alert")).toHaveTextContent(/retry/i);
    expect(prompts).toEqual([]);
    if (refuseReopen) {
      await act(async () =>
        useWorkbenchStore.getState().activateTab(before.activeTabId!),
      );
      await act(async () =>
        useWorkbenchStore
          .getState()
          .openNewChatDraft(useWorkbenchStore.getState().project!.id),
      );
      vi.mocked(ports.chat.activateChat).mockResolvedValueOnce({
        committed: false,
        snapshot: null,
        transitionEpoch: created.transitionEpoch,
        recoveryError: "Activation refused",
      });
      await act(async () => useWorkbenchStore.getState().sendNewChatDraft());
      await act(async () =>
        useWorkbenchStore
          .getState()
          .openNewChatDraft(useWorkbenchStore.getState().project!.id),
      );
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

it("keeps effort selectable in the model popover when the narrow toolbar exhausts its compaction steps", async () => {
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
  const slider = await openModelEffort();
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
        accountReadStartedAt: 100,
        models: models.map((model) => ({ ...model, id: model.id as never })),
        defaultModelId: "model-a" as never,
      }));
    if (recovery === "sign-in") {
      const signedIn = discover.getMockImplementation()!;
      discover.mockImplementationOnce(async (request) => ({
        ...(await signedIn(request)),
        account: "signedOut",
        accountReadStartedAt: 100,
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
      expect(screen.queryByText("Other harness isn't signed in")).toBeNull();
      render(<HarnessAccountMenu avatar="W" />);
      fireEvent.click(
        await screen.findByRole("button", { name: "Account: sign-in needed" }),
      );
      fireEvent.click(
        await screen.findByRole("menuitem", {
          name: "Sign in to Other harness",
        }),
      );
      expect(composer()).toHaveValue("Keep this account draft");
    } else {
      fireEvent.click(screen.getByRole("button", { name: "Harness" }));
      const picker = await screen.findByRole("dialog", { name: "Harness" });
      expect(await within(picker).findByRole("alert")).toHaveTextContent(
        "Account discovery unavailable",
      );
      fireEvent.click(within(picker).getByRole("button", { name: "Retry" }));
      fireEvent.keyDown(picker, { key: "Escape" });
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
  fireEvent.change(await openModelEffort(), { target: { value: "2" } });
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

it("sends a top-level new chat in its private scope without changing the public project list", async () => {
  const real = useWorkbenchStore.getState().project!.id;
  useWorkbenchStore.getState().discardNewChatDraft();
  await useWorkbenchStore.getState().openNewChatDraft();
  mount();
  await settleLazyModules();
  fireEvent.change(composer(), { target: { value: "Work without a project" } });
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() => expect(prompts).toHaveLength(1));
  const state = useWorkbenchStore.getState();
  expect(state.project?.isInternal).toBe(true);
  expect(state.project?.id).not.toBe(real);
  expect(state.projects.map((project) => project.id)).toEqual([real]);
  expect(prompts[0]).toMatchObject({
    projectId: state.project!.id,
    chatId: state.activeTabId,
    prompt: "Work without a project",
  });
  expect(
    state.unscopedChats
      .flatMap((scope) => scope.railChats ?? [])
      .map((chat) => chat.id),
  ).toEqual([state.activeTabId]);
});

it.each([false, true])(
  "preserves a dirty right-pane editor when retrying a draft in another project (flush refused: %s)",
  async (refuseFlush) => {
    const { chatId } = await createdDraft({
      text: "Move only my owned prompt",
    });
    const state = useWorkbenchStore.getState();
    const sourceProject = state.project!.id;
    const resource = {
      rootId: state.project!.roots[0].id,
      relativePath: `draft-notes-${crypto.randomUUID()}.txt`,
    };
    await act(async () => {
      await state.openFile(resource);
      await state.loadEditor(resource);
    });
    act(() => state.updateEditorContent("Keep my unsaved editor work"));
    expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
      phase: "ready",
      content: "Keep my unsaved editor work",
      revision: 1,
      persistedRevision: 0,
    });
    if (refuseFlush)
      vi.spyOn(ports.editor, "snapshotEditorBuffer").mockRejectedValueOnce(
        new Error("Editor storage unavailable"),
      );
    act(() => {
      useWorkbenchStore.setState({
        agentCenter: "draft",
      });
      state.updateNewChatDraft({ targetProjectId: null });
    });
    await act(async () => useWorkbenchStore.getState().sendNewChatDraft());
    if (refuseFlush) {
      expect(prompts).toEqual([]);
      expect(useWorkbenchStore.getState()).toMatchObject({
        project: { id: sourceProject },
        projectTransitioning: false,
        activeEditor: { content: "Keep my unsaved editor work" },
        newChatDraft: { submission: { status: "created", chatId } },
      });
      expect((await ports.project.getBootstrapSnapshot()).project!.id).toBe(
        sourceProject,
      );
      await act(async () => useWorkbenchStore.getState().sendNewChatDraft());
    }
    await waitFor(() => expect(prompts).toHaveLength(1));
    expect(
      (await ports.editor.openEditorDocument(resource)).buffer.content,
    ).toBe("Keep my unsaved editor work");
    expect(useWorkbenchStore.getState().activeTabId).toBe(chatId);
    expect(useWorkbenchStore.getState().project?.isInternal).toBe(true);
    expect(prompts[0]).toMatchObject({
      chatId,
      prompt: "Move only my owned prompt",
    });
  },
);

it("changes the visible chat's project while a right-pane File is active", async () => {
  useWorkbenchStore.getState().discardNewChatDraft();
  const chatId = await existingSessionlessChat();
  pinAgentChatOwner(chatId);
  render(<ChatSurface chatId={chatId} />);
  await settleLazyModules();
  const state = useWorkbenchStore.getState();
  const resource = {
    rootId: state.project!.roots[0].id,
    relativePath: "notes.txt",
  };
  await act(async () => state.openFile(resource));
  expect(useWorkbenchStore.getState().activeTabId).not.toBe(chatId);
  const location = within(screen.getByRole("group", { name: "Chat location" }));
  fireEvent.click(location.getByRole("button", { name: /^Project:/ }));
  await waitFor(() =>
    expect(useWorkbenchStore.getState().projectTransitioning).toBe(false),
  );
  fireEvent.click(
    await screen.findByRole("option", { name: "Don't work in a project" }),
  );
  await settleLazyModules();
  expect(useWorkbenchStore.getState().error).toBeNull();
  await waitFor(() =>
    expect(useWorkbenchStore.getState()).toMatchObject({
      activeTabId: chatId,
      project: { isInternal: true },
    }),
  );
  expect((await ports.project.getBootstrapSnapshot()).activeTabId).toBe(chatId);
});
