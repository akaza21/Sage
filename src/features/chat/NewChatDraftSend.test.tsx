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
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { acpDesktop } from "../../services/acpDesktop";
import { AcpDesktopError } from "../../services/acpDesktopCommand";
import { desktop } from "../../services/desktop";
import { resetBrowserDesktopForTests } from "../../services/browserDesktop";
import { settleLazyModules } from "../../test/settleLazyModules";
import { AgentChatRuntimeHarness } from "../../test/AgentChatRuntimeHarness";
import { registerWorkbenchSurfaces } from "../../workbench/registeredSurfaces";
import { AgentChatPanes } from "../../workbench/AgentChatPanes";

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
function mount(strict = false) {
  registerWorkbenchSurfaces();
  const content = (
    <AgentChatRuntimeHarness>
      <AgentChatPanes />
    </AgentChatRuntimeHarness>
  );
  return render(strict ? <StrictMode>{content}</StrictMode> : content);
}
const composer = () => screen.getByRole("combobox", { name: "Message agent" });

beforeEach(async () => {
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
  vi.spyOn(ports.chat, "createChat").mockImplementation(async (...args) => {
    const result = await create(...args);
    if (result.snapshot)
      Object.assign(result.snapshot, {
        harnesses: [harness, otherHarness],
        selectedHarnessId: harness.id,
        selectedModelId: "model-a",
      });
    return result;
  });
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

it("retains text and settings after rejected creation, and retries the draft", async () => {
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
  expect((await ports.project.getBootstrapSnapshot()).tabs).toEqual(
    before.tabs,
  );
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() => expect(prompts).toHaveLength(1));
  expect(prompts[0]?.prompt).toBe("Keep this request");
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

it("keeps the confirmed chat and its text after an Editor round trip during creation, without sending", async () => {
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
  expect(screen.queryByRole("region", { name: "New chat draft" })).toBeNull();
  expect(screen.getByRole("region", { name: /^Chat pane/ })).toBeVisible();
  expect(composer()).toHaveValue("Keep this confirmed request");
  expect((await ports.project.getBootstrapSnapshot()).tabs.length).toBe(
    before.tabs.length + 1,
  );
  expect(prompts).toEqual([]);
  expect(starts).toEqual([]);
});
