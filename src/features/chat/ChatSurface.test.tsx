import {
  act,
  cleanup,
  createEvent,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../../app/acpProjectionRegistry";
import { useAgentArtifactAdmissionStore } from "../../app/agentArtifactAdmissionStore";
import {
  AgentSessionChromeContext,
  clearAgentChatOwner,
  pinAgentChatOwner,
} from "../../app/agentChatOwner";
import { useAcpProjectionStore } from "../../app/acpProjectionStore";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage, AcpItem } from "../../domain/acp";
import type { ResourceRef } from "../../domain/workbench";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { acpDesktop } from "../../services/acpDesktop";
import { desktop } from "../../services/desktop";
import { settleLazyModules } from "../../test/settleLazyModules";
import { useSideChatState } from "../../workbench/sideChatState";
import { Workbench } from "../../workbench/Workbench";
import { useAgentSettingsState } from "../settings/agentSettingsState";
import type * as AcpModule from "./acp";

const hooks = vi.hoisted(() => ({
  page: null as AcpChatPage | null,
  actions: {
    busy: false,
    error: null as string | null,
    errorKind: null as string | null,
    cancellableTurnId: null as string | null,
    submit: vi.fn(),
    steer: vi.fn(),
    cancel: vi.fn(),
    close: vi.fn(),
    setSessionMode: vi.fn(),
    clearError: vi.fn(),
    modeError: null as string | null,
    modeChanging: false,
  },
}));

vi.mock("./acp", async () => {
  const actual = await vi.importActual<typeof AcpModule>("./acp");
  return {
    ...actual,
    useAcpChatProjection: () => hooks.page,
    useAcpChatActions: () => hooks.actions,
  };
});

import { ChatSurface } from "./ChatSurface";

const initialState = useWorkbenchStore.getState();
const nativeReadOnly = [
  {
    id: "readOnly" as const,
    label: "Read-only",
    description: "Read workspace files; no writes or network.",
    fallback: true,
  },
];

function openComposerOptions() {
  const options = screen.queryByRole("button", { name: "Composer options" });
  if (options) fireEvent.click(options);
}

function openComposerModel() {
  fireEvent.click(screen.getByRole("button", { name: "Model" }));
}

function itemBase(id: string) {
  return {
    id,
    projectId: "project-a",
    chatId: "chat-a",
    sessionId: "session-a",
    turnId: "turn-a",
    runtimeEpoch: "1",
    ordinal: 0,
    origin: "primary",
    activityId: null,
    lifecycle: "completed",
    revision: "1",
    createdAt: 1,
    updatedAt: 1,
    terminalAt: 1,
  };
}

function projection(): AcpChatPage {
  const items = [
    {
      ...itemBase("item-file"),
      kind: "toolCall",
      lifecycle: "streaming",
      terminalAt: null,
      toolState: "inProgress",
      payload: {
        presentation: {
          kind: "read",
          label: "Read file",
          resources: [
            {
              kind: "resource",
              resource: { rootId: "root-a", relativePath: "src/main.ts" },
            },
          ],
        },
        inputSummary: null,
      },
    },
    {
      ...itemBase("item-diff"),
      kind: "fileChangeDiff",
      toolState: null,
      payload: {
        changes: [
          {
            resource: {
              kind: "resource",
              resource: { rootId: "root-b", relativePath: "src/main.ts" },
            },
            operation: "updated",
            originalResource: null,
            summary: "Updated duplicate path",
            additions: 2,
            deletions: 1,
            pairState: "available",
            snapshotRevision: "1",
            oldBytes: 4,
            newBytes: 4,
            contentDigest: "a".repeat(64),
          },
        ],
        truncated: false,
      },
    },
  ] as unknown as AcpItem[];
  return {
    projectId: "project-a",
    chatId: "chat-a",
    title: "Durable chat",
    projectionRevision: "2",
    currentSessionId: "session-a",
    sessions: [{ id: "session-a", ordinal: 0 }] as never,
    turns: [
      {
        id: "turn-a",
        sessionId: "session-a",
        runtimeEpoch: "1",
        ordinal: 0,
        state: "running",
        revision: "1",
        createdAt: 1,
        updatedAt: 1,
        terminalAt: null,
        items,
        activities: [],
      },
    ],
    nextCursor: null,
  } as unknown as AcpChatPage;
}

function matchConfiguredSession() {
  hooks.page = {
    ...hooks.page!,
    sessions: [
      {
        id: "session-a",
        ordinal: 0,
        runtimeEpoch: "1",
        state: "active",
        legacy: false,
        bindingId: "codex-app-server",
        modelId: useWorkbenchStore.getState().selectedModelId,
        capabilities: { prompt: true, closeSession: true },
      },
    ] as never,
  };
}

function measureTranscript({ scrollTop }: { scrollTop: number }): Element {
  const transcript = document.querySelector(".message-list");
  expect(transcript).not.toBeNull();
  Object.defineProperties(transcript!, {
    clientHeight: { configurable: true, value: 400 },
    scrollHeight: { configurable: true, value: 1_000 },
    scrollTop: { configurable: true, value: scrollTop, writable: true },
  });
  return transcript!;
}

function setTurnState(state: AcpChatPage["turns"][number]["state"]) {
  const page = hooks.page!;
  hooks.page = {
    ...page,
    projectionRevision: `${Number(page.projectionRevision) + 1}` as never,
    turns: page.turns.map((turn, index) =>
      index === 0
        ? {
            ...turn,
            state,
            terminalAt: [
              "completed",
              "interrupted",
              "cancelled",
              "failed",
              "deliveryUnknown",
            ].includes(state)
              ? 3
              : null,
          }
        : turn,
    ),
  };
}

async function expandReadActivity() {
  const disclosure = await waitFor(() => {
    const found = document.querySelector<HTMLDetailsElement>(
      '.acp-tool-rollup[data-activity-kind="read"]',
    );
    if (!found) throw new Error("Expected the read activity disclosure");
    return found;
  });
  const summary = disclosure.querySelector("summary");
  if (!summary) throw new Error("Expected the read activity summary");
  if (!disclosure.open) fireEvent.click(summary);
}

beforeEach(() => {
  useSessionAccessModeStore.setState({ modes: {}, ready: true, error: null });
  useAcpProjectionStore.setState({ retentionIncomplete: false });
  hooks.page = projection();
  hooks.actions.busy = false;
  hooks.actions.error = null;
  hooks.actions.errorKind = null;
  hooks.actions.cancellableTurnId = null;
  hooks.actions.modeError = null;
  hooks.actions.modeChanging = false;
  Element.prototype.scrollIntoView = vi.fn();
  Element.prototype.scrollTo = vi.fn();
  useWorkbenchStore.setState({
    harnesses: [
      {
        id: "codex",
        name: "Codex",
        description: "ACP",
        installState: "available",
        source: "builtIn",
        accountModelDiscovery: true,
        transport: "acp",
        bindingId: "codex-app-server",
        accessModes: nativeReadOnly,
      },
    ],
    selectedHarnessId: "codex",
    harnessDiscoveryPhase: "ready",
    harnessDiscoveryError: null,
  });
});

afterEach(() => {
  cleanup();
  useAgentSettingsState.getState().close();
  useWorkbenchStore.setState(initialState, true);
  clearAcpProjectionRegistry();
  clearAgentChatOwner();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

describe("ChatSurface approval slot", () => {
  function withPermission(state: "received" | "declinePending") {
    const page = projection();
    hooks.page = {
      ...page,
      turns: [
        {
          ...page.turns[0],
          items: [
            ...page.turns[0].items,
            {
              ...itemBase("item-permission"),
              kind: "permissionRequest",
              lifecycle: "streaming",
              terminalAt: null,
              toolState: "waitingForApproval",
              payload: {
                requestId: "request-a",
                state,
                label: "Permission request",
                summary: "curl.exe https://example.com",
                offeredOptionKinds: [
                  "allow_once",
                  "allow_always",
                  "reject_once",
                ],
                category: "execute",
                reason: "May I retry the network check outside the sandbox?",
              },
            } as unknown as AcpItem,
          ],
        },
      ],
    };
  }

  it("replaces the composer with the pending approval and returns the draft after it settles", async () => {
    matchConfiguredSession();
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });
    const { rerender } = render(<ChatSurface chatId="chat-a" />);
    fireEvent.change(screen.getByRole("textbox", { name: "Message agent" }), {
      target: { value: "keep this draft" },
    });
    expect(screen.getByRole("textbox", { name: "Message agent" })).toHaveValue(
      "keep this draft",
    );
    withPermission("received");
    rerender(<ChatSurface chatId="chat-a" />);
    await settleLazyModules();
    const prompt = await screen.findByRole("group", {
      name: "Permission request",
    });
    expect(within(prompt).getByRole("button", { name: /Deny/ })).toBeVisible();
    // The composer held focus, so the card announces itself instead of
    // taking focus (an Enter meant for the draft must never approve).
    expect(
      within(prompt).getByRole("button", { name: /Allow once/ }),
    ).not.toHaveFocus();
    expect(within(prompt).getByRole("status")).toHaveTextContent(
      "Approval needed: Allow this command?",
    );
    expect(screen.queryByRole("textbox", { name: "Message agent" })).toBeNull();
    expect(document.querySelector("form.composer")).toHaveAttribute("hidden");
    expect(
      document.querySelector(
        ".message-list [data-acp-item-id='item-permission']",
      ),
    ).toBeNull();

    withPermission("declinePending");
    rerender(<ChatSurface chatId="chat-a" />);
    expect(screen.queryByRole("button", { name: /Allow once/ })).toBeNull();
    const composer = screen.getByRole("textbox", { name: "Message agent" });
    expect(composer).toHaveValue("keep this draft");
    expect(composer).toHaveFocus();
  });
});

describe("ChatSurface ACP resource integration", () => {
  it("retains composer project scope while an empty retained page is not the new-chat home", () => {
    hooks.page = { ...projection(), turns: [], nextCursor: "earlier" as never };
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "Sage", roots: [] },
    });
    render(<ChatSurface />);
    expect(screen.queryByRole("region", { name: "New chat" })).toBeNull();
    expect(
      document.querySelector("form.composer .project-scope"),
    ).toHaveTextContent("Sage");
  });
  it.each(["primary", "focused"] as const)(
    "explains the existing %s turn guard when the empty home's project action is refused",
    (owner) => {
      const live = projection();
      hooks.page = {
        ...live,
        chatId: "chat-b" as never,
        currentSessionId: null,
        sessions: [],
        turns: [],
      };
      const switchProject = vi.fn();
      useWorkbenchStore.setState({
        project: { id: "project-a", name: "A", roots: [] },
        projects: [
          { id: "project-a", name: "A", rootCount: 0, lastOpenedAt: 1 },
          { id: "project-b", name: "B", rootCount: 0, lastOpenedAt: 1 },
        ],
        activeTabId: "chat-a",
        tabs: ["chat-a", "chat-b"].map((id, position) => ({
          id,
          kind: "chat",
          title: id,
          position,
          pinned: false,
          dirty: false,
        })),
        switchProject,
      });
      const active =
        owner === "primary"
          ? useAcpProjectionStore
          : acpProjectionStoreFor("chat-a");
      active.setState({ page: live });
      acpProjectionStoreFor("chat-b").setState({ page: hooks.page });
      try {
        render(<ChatSurface chatId="chat-b" />);
        fireEvent.click(
          within(screen.getByRole("region", { name: "New chat" })).getByRole(
            "button",
            { name: "Project: A" },
          ),
        );
        const picker = screen.getByRole("combobox", { name: "Project: A" });
        fireEvent.keyDown(picker, { key: "End" });
        fireEvent.keyDown(picker, { key: "Enter" });
        expect(screen.getByRole("alert")).toHaveTextContent(
          "Finish the active turn before switching projects.",
        );
        expect(switchProject).not.toHaveBeenCalled();
      } finally {
        active.getState().clear();
      }
    },
  );

  it.each([
    "unsupported",
    "unavailable",
    "noGoal",
    "loading",
    "offline",
  ] as const)("omits the %s Goal row from the composer", (kind) => {
    hooks.page = { ...projection(), goal: { kind } };
    matchConfiguredSession();
    render(<ChatSurface />);
    expect(screen.queryByText("Goal", { exact: true })).toBeNull();
    expect(screen.queryByLabelText("Goal details")).toBeNull();
  });

  it("opens an earlier-session assistant file link in its owning Chat", async () => {
    const resource = { rootId: "root-a", relativePath: "README.md" };
    vi.spyOn(acpDesktop, "resolveChatFileReference").mockResolvedValue({
      resource,
      lineNumber: null,
    });
    const openFile = vi.fn().mockResolvedValue(undefined);
    hooks.page = {
      ...projection(),
      currentSessionId: "session-new" as never,
      sessions: [
        { id: "session-a", ordinal: 0 },
        { id: "session-new", ordinal: 1 },
      ] as never,
      turns: [
        {
          ...projection().turns[0],
          state: "completed",
          items: [
            {
              ...itemBase("earlier-link"),
              kind: "text",
              payload: {
                role: "assistant",
                text: "See [README.md](README.md)",
              },
            } as AcpItem,
          ],
        },
      ],
    };
    useWorkbenchStore.setState({
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
        ],
      },
      projectGeneration: 0,
      projectTransitioning: false,
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
        {
          id: "file:readme",
          kind: "file",
          title: "README.md",
          position: 1,
          pinned: false,
          dirty: false,
          resource,
        },
      ],
      activeTabId: "chat-a",
      openFile,
    });
    acpProjectionStoreFor("chat-a").setState({
      scope: {
        projectId: "project-a",
        projectGeneration: "0",
        chatId: "chat-a",
      } as never,
      page: hooks.page,
    });

    render(<ChatSurface chatId="chat-a" />);
    await settleLazyModules();
    fireEvent.click(screen.getByRole("button", { name: "Open README.md" }));

    await waitFor(() => expect(openFile).toHaveBeenCalledWith(resource));
    expect(useAgentArtifactAdmissionStore.getState().admissions).toContainEqual(
      {
        owner: {
          projectId: "project-a",
          chatId: "chat-a",
          sessionId: "session-new",
        },
        tabId: "file:readme",
      },
    );
  });

  it("keeps assistant file links inert when the Chat has no Project root", async () => {
    const resolve = vi.spyOn(acpDesktop, "resolveChatFileReference");
    hooks.page = {
      ...projection(),
      turns: [
        {
          ...projection().turns[0],
          items: [
            {
              ...itemBase("rootless-link"),
              kind: "text",
              payload: {
                role: "assistant",
                text: "See [README.md](README.md)",
              },
            } as AcpItem,
          ],
        },
      ],
    };
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });

    render(<ChatSurface chatId="chat-a" />);
    expect(await screen.findByText("See README.md")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open README.md" })).toBeNull();
    expect(resolve).not.toHaveBeenCalled();
  });

  it("queues a message during a turn and sends it when the turn finishes", async () => {
    hooks.actions.submit.mockResolvedValue("applied");
    matchConfiguredSession();
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });
    const view = render(<ChatSurface chatId="chat-a" />);
    fireEvent.change(screen.getByRole("textbox", { name: "Message agent" }), {
      target: { value: "Follow up" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Queue message" }));
    await waitFor(() =>
      expect(
        screen.getByRole("textbox", { name: "Message agent" }),
      ).toHaveValue(""),
    );
    expect(await screen.findByText("Follow up")).toBeInTheDocument();
    expect(hooks.actions.submit).not.toHaveBeenCalled();

    setTurnState("completed");
    view.rerender(<ChatSurface chatId="chat-a" />);
    await waitFor(() =>
      expect(hooks.actions.submit).toHaveBeenCalledWith(
        "Follow up",
        [],
        "codex-app-server",
        null,
      ),
    );
    await waitFor(() => expect(screen.queryByText("Follow up")).toBeNull());
  });

  it("drains a queued follow-up while Settings sleeps the real Chat surface", async () => {
    localStorage.clear();
    vi.stubGlobal(
      "matchMedia",
      vi.fn(() => ({
        matches: false,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      })),
    );
    hooks.actions.submit.mockResolvedValue("applied");
    matchConfiguredSession();
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });
    const view = render(<Workbench />);
    fireEvent.click(screen.getByRole("radio", { name: "Agent" }));
    await settleLazyModules();
    fireEvent.change(
      await screen.findByRole("textbox", { name: "Message agent" }),
      {
        target: { value: "Continue while settings is open" },
      },
    );
    fireEvent.click(screen.getByRole("button", { name: "Queue message" }));
    await waitFor(() =>
      expect(
        useWorkbenchStore.getState().chatUiById["chat-a"].queuedPrompts?.[0]
          ?.status,
      ).toBe("queued"),
    );
    fireEvent.click(screen.getByRole("button", { name: "Open settings" }));
    await act(async () => {
      await vi.dynamicImportSettled();
    });
    await screen.findByRole("region", { name: "Agent settings" });
    expect(screen.queryByRole("textbox", { name: "Message agent" })).toBeNull();
    setTurnState("completed");
    view.rerender(<Workbench />);
    await waitFor(() =>
      expect(hooks.actions.submit).toHaveBeenCalledWith(
        "Continue while settings is open",
        [],
        "codex-app-server",
        null,
      ),
    );
    expect(
      screen.getByRole("region", { name: "Agent settings" }),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Back to chats" }));
    await screen.findByRole("textbox", { name: "Message agent" });
    expect(hooks.actions.submit).toHaveBeenCalledOnce();
  });

  it("keeps one send-slot control while a cancellable Turn can queue by Enter", async () => {
    hooks.actions.cancellableTurnId = "turn-a";
    matchConfiguredSession();
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });

    render(<ChatSurface chatId="chat-a" />);
    const composer = screen.getByRole("textbox", { name: "Message agent" });
    expect(composer).toHaveAccessibleDescription(
      /Enter queues a message while this turn runs\. Shift\+Enter adds a new line\./,
    );
    expect(composer).toHaveAttribute("placeholder", "Do anything");
    expect(composer).toHaveAttribute(
      "aria-description",
      "Enter queues a message while this turn runs.",
    );
    fireEvent.change(composer, { target: { value: "Follow up" } });
    const stop = screen.getByRole("button", { name: "Cancel turn" });
    expect(stop.closest("form")?.querySelectorAll(".send-button")).toHaveLength(
      1,
    );
    expect(screen.queryByRole("button", { name: "Queue message" })).toBeNull();

    fireEvent.keyDown(composer, { key: "Enter" });
    await waitFor(() => expect(screen.getByText("Follow up")).toBeVisible());
    expect(stop.closest("form")?.querySelectorAll(".send-button")).toHaveLength(
      1,
    );
    expect(stop).toBeEnabled();
    fireEvent.click(stop);
    expect(hooks.actions.cancel).toHaveBeenCalledWith("turn-a");
    expect(hooks.actions.submit).not.toHaveBeenCalled();
  });

  it("keeps a queued draft until native persistence acknowledges it", async () => {
    let acknowledge!: () => void;
    const persistence = new Promise<void>((resolve) => {
      acknowledge = resolve;
    });
    const save = vi
      .spyOn(getWorkbenchPorts().project, "replaceChatQueuedPrompts")
      .mockReturnValue(persistence);
    matchConfiguredSession();
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });

    render(<ChatSurface chatId="chat-a" />);
    const composer = screen.getByRole("textbox", { name: "Message agent" });
    fireEvent.change(composer, { target: { value: "Follow up" } });
    fireEvent.click(screen.getByRole("button", { name: "Queue message" }));

    expect(save).toHaveBeenCalledWith("project-a", "chat-a", [
      expect.objectContaining({ text: "Follow up", status: "queued" }),
    ]);
    expect(composer).toHaveValue("Follow up");
    expect(composer).toBeEnabled();
    expect(composer).toHaveFocus();
    expect(screen.getByText("Saving…")).toBeVisible();
    expect(hooks.actions.submit).not.toHaveBeenCalled();

    acknowledge();
    await waitFor(() => expect(composer).toHaveValue(""));
    expect(screen.getByText("Queued")).toBeVisible();
  });

  it("cannot send a draft again when its queue save outlives the active Turn", async () => {
    hooks.actions.submit.mockResolvedValue("applied");
    let acknowledge!: () => void;
    vi.spyOn(
      getWorkbenchPorts().project,
      "replaceChatQueuedPrompts",
    ).mockReturnValueOnce(
      new Promise<void>((resolve) => {
        acknowledge = resolve;
      }),
    );
    matchConfiguredSession();
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });

    const view = render(<ChatSurface chatId="chat-a" />);
    const composer = screen.getByRole("textbox", { name: "Message agent" });
    fireEvent.change(composer, { target: { value: "Once only" } });
    fireEvent.click(screen.getByRole("button", { name: "Queue message" }));
    setTurnState("completed");
    view.rerender(<ChatSurface chatId="chat-a" />);

    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    expect(hooks.actions.submit).not.toHaveBeenCalled();
    expect(composer).toBeEnabled();
    expect(composer).toHaveFocus();
    acknowledge();
    await waitFor(() => expect(composer).toHaveValue(""));
    await waitFor(() => expect(hooks.actions.submit).toHaveBeenCalledTimes(1));
  });

  it("keeps the draft and reports an error when queue persistence fails", async () => {
    vi.spyOn(
      getWorkbenchPorts().project,
      "replaceChatQueuedPrompts",
    ).mockRejectedValueOnce(new Error("disk full"));
    matchConfiguredSession();
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });

    render(<ChatSurface chatId="chat-a" />);
    const composer = screen.getByRole("textbox", { name: "Message agent" });
    fireEvent.change(composer, { target: { value: "Keep this" } });
    fireEvent.click(screen.getByRole("button", { name: "Queue message" }));

    await waitFor(() =>
      expect(screen.getByText("Could not save queued message.")).toBeVisible(),
    );
    expect(composer).toHaveValue("Keep this");
    expect(
      screen.queryByRole("img", { name: /Queued attachment preview/ }),
    ).toBeNull();
    expect(screen.queryByText("Saving…")).toBeNull();
  });

  it("keeps a queued row visible until native deletion is durable", async () => {
    let acknowledgeDelete!: () => void;
    const deletion = new Promise<void>((resolve) => {
      acknowledgeDelete = resolve;
    });
    const save = vi
      .spyOn(getWorkbenchPorts().project, "replaceChatQueuedPrompts")
      .mockResolvedValueOnce()
      .mockReturnValueOnce(deletion);
    matchConfiguredSession();
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });

    render(<ChatSurface chatId="chat-a" />);
    const composer = screen.getByRole("textbox", { name: "Message agent" });
    fireEvent.change(composer, { target: { value: "Remove me" } });
    fireEvent.click(screen.getByRole("button", { name: "Queue message" }));
    await waitFor(() => expect(composer).toHaveValue(""));

    fireEvent.click(
      screen.getByRole("button", { name: "Delete queued message: Remove me" }),
    );
    expect(save).toHaveBeenLastCalledWith("project-a", "chat-a", []);
    expect(screen.getByText("Remove me")).toBeVisible();
    acknowledgeDelete();
    await waitFor(() => expect(screen.queryByText("Remove me")).toBeNull());
  });

  it("keeps a queued row for retry when native deletion fails", async () => {
    vi.spyOn(getWorkbenchPorts().project, "replaceChatQueuedPrompts")
      .mockResolvedValueOnce()
      .mockRejectedValueOnce(new Error("disk full"));
    matchConfiguredSession();
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });

    render(<ChatSurface chatId="chat-a" />);
    fireEvent.change(screen.getByRole("textbox", { name: "Message agent" }), {
      target: { value: "Keep this row" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Queue message" }));
    await waitFor(() => expect(screen.getByText("Queued")).toBeVisible());
    fireEvent.click(
      screen.getByRole("button", {
        name: "Delete queued message: Keep this row",
      }),
    );

    await waitFor(() =>
      expect(
        screen.getByText("Could not remove queued message."),
      ).toBeVisible(),
    );
    expect(screen.getByText("Keep this row")).toBeVisible();
    expect(screen.getByText("Needs retry")).toBeVisible();
  });

  it("does not apply a stale delete failure to a replacement Chat owner", async () => {
    let rejectDelete!: (error: Error) => void;
    vi.spyOn(getWorkbenchPorts().project, "replaceChatQueuedPrompts")
      .mockResolvedValueOnce()
      .mockReturnValueOnce(
        new Promise<void>((_resolve, reject) => {
          rejectDelete = reject;
        }),
      );
    matchConfiguredSession();
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });

    const view = render(<ChatSurface chatId="chat-a" />);
    fireEvent.change(screen.getByRole("textbox", { name: "Message agent" }), {
      target: { value: "Old queue row" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Queue message" }));
    await waitFor(() => expect(screen.getByText("Queued")).toBeVisible());
    fireEvent.click(
      screen.getByRole("button", {
        name: "Delete queued message: Old queue row",
      }),
    );
    const oldState = useWorkbenchStore.getState();
    const replacementQueue = [
      {
        ...oldState.chatUiById["chat-a"].queuedPrompts![0],
        status: "queued" as const,
        text: "New owner row",
      },
    ];
    view.unmount();
    useWorkbenchStore.setState({
      chatOwnershipEpoch: oldState.chatOwnershipEpoch + 1,
      chatUiById: {
        ...oldState.chatUiById,
        "chat-a": {
          ...oldState.chatUiById["chat-a"],
          queuedPrompts: replacementQueue,
          error: null,
        },
      },
    });
    await act(async () => rejectDelete(new Error("old write failed")));
    expect(
      useWorkbenchStore.getState().chatUiById["chat-a"].queuedPrompts,
    ).toEqual(replacementQueue);
    expect(useWorkbenchStore.getState().chatUiById["chat-a"].error).toBeNull();
  });

  it("offers Steer only for an active capable session and retains a rejected row", async () => {
    let rejectSteer!: (value: "rejected") => void;
    const delayedRejection = new Promise<"rejected">((resolve) => {
      rejectSteer = resolve;
    });
    hooks.actions.steer
      .mockReturnValueOnce(delayedRejection)
      .mockResolvedValueOnce("applied");
    matchConfiguredSession();
    hooks.page = {
      ...hooks.page!,
      sessions: hooks.page!.sessions.map((session) => ({
        ...session,
        capabilities: { ...session.capabilities, activeTurnSteer: true },
      })),
    };
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });
    const view = render(<ChatSurface chatId="chat-a" />);
    fireEvent.change(screen.getByRole("textbox", { name: "Message agent" }), {
      target: { value: "Change direction" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Queue message" }));
    await waitFor(() =>
      expect(
        screen.getByRole("textbox", { name: "Message agent" }),
      ).toHaveValue(""),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "Steer queued message: Change direction",
      }),
    );
    expect(screen.getByText("Change direction")).toBeVisible();
    expect(screen.queryByText("Needs retry")).toBeNull();
    rejectSteer("rejected");
    await waitFor(() => expect(screen.getByText("Needs retry")).toBeVisible());
    expect(screen.getByText("Change direction")).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", {
        name: "Steer queued message: Change direction",
      }),
    );
    await waitFor(() =>
      expect(screen.queryByText("Change direction")).toBeNull(),
    );
    expect(hooks.actions.steer).toHaveBeenCalledTimes(2);
    view.unmount();
  });

  it("keeps a Steer timeout row queued with an explicit retry cue", async () => {
    hooks.actions.steer.mockResolvedValueOnce("outcomeUnknown");
    const persist = vi
      .spyOn(getWorkbenchPorts().project, "replaceChatQueuedPrompts")
      .mockResolvedValue(undefined);
    matchConfiguredSession();
    hooks.page = {
      ...hooks.page!,
      sessions: hooks.page!.sessions.map((session) => ({
        ...session,
        capabilities: { ...session.capabilities, activeTurnSteer: true },
      })),
    };
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });
    render(<ChatSurface chatId="chat-a" />);
    fireEvent.change(screen.getByRole("textbox", { name: "Message agent" }), {
      target: { value: "Check the result" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Queue message" }));
    await waitFor(() =>
      expect(
        screen.getByRole("textbox", { name: "Message agent" }),
      ).toHaveValue(""),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "Steer queued message: Check the result",
      }),
    );
    await waitFor(() =>
      expect(screen.getByText("May have been delivered")).toBeVisible(),
    );
    expect(screen.getByText("Check the result")).toBeVisible();
    expect(
      screen.getByRole("button", {
        name: "Steer queued message: Check the result",
      }),
    ).toHaveAttribute("title", "Retry Steer with the same message ID");
    const queuedId =
      useWorkbenchStore.getState().chatUiById["chat-a"].queuedPrompts![0].id;
    expect(hooks.actions.steer).toHaveBeenCalledWith(
      "Check the result",
      [],
      queuedId,
    );
    expect(persist).toHaveBeenCalledWith("project-a", "chat-a", [
      expect.objectContaining({ id: queuedId, status: "outcomeUnknown" }),
    ]);
  });

  it("drains queued messages in order across successive turns", async () => {
    hooks.actions.submit.mockResolvedValue("applied");
    matchConfiguredSession();
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });
    const view = render(<ChatSurface chatId="chat-a" />);
    const composer = screen.getByRole("textbox", { name: "Message agent" });
    for (const message of ["First", "Second"]) {
      fireEvent.change(composer, { target: { value: message } });
      fireEvent.click(screen.getByRole("button", { name: "Queue message" }));
      await waitFor(() => expect(composer).toHaveValue(""));
    }
    expect(screen.getByText("First")).toBeInTheDocument();
    expect(screen.getByText("Second")).toBeInTheDocument();

    setTurnState("completed");
    view.rerender(<ChatSurface chatId="chat-a" />);
    await waitFor(() => expect(hooks.actions.submit).toHaveBeenCalledTimes(1));
    expect(hooks.actions.submit.mock.calls[0]?.[0]).toBe("First");
    view.unmount();
    const completedPage = hooks.page;
    hooks.page = null;
    const returned = render(<ChatSurface chatId="chat-a" />);
    expect(hooks.actions.submit).toHaveBeenCalledTimes(1);
    hooks.page = completedPage;
    returned.rerender(<ChatSurface chatId="chat-a" />);
    expect(hooks.actions.submit).toHaveBeenCalledTimes(1);
    hooks.page = {
      ...hooks.page!,
      turns: [
        ...hooks.page!.turns,
        {
          ...hooks.page!.turns[0],
          id: "turn-b" as never,
          ordinal: 1,
          state: "running",
        },
      ],
    };
    returned.rerender(<ChatSurface chatId="chat-a" />);
    hooks.page = {
      ...hooks.page!,
      turns: hooks.page!.turns.map((turn) =>
        turn.id === "turn-b" ? { ...turn, state: "completed" } : turn,
      ),
    };
    returned.rerender(<ChatSurface chatId="chat-a" />);
    await waitFor(() => expect(hooks.actions.submit).toHaveBeenCalledTimes(2));
    expect(hooks.actions.submit.mock.calls[1]?.[0]).toBe("Second");
  });

  it("does not treat a returning old Turn as the new queued Turn", async () => {
    hooks.actions.submit.mockResolvedValue("applied");
    matchConfiguredSession();
    const historicalTurns = Array.from({ length: 18 }, (_, ordinal) => ({
      ...projection().turns[0],
      id: `turn-${ordinal}` as never,
      ordinal,
      state: ordinal === 17 ? ("running" as const) : ("completed" as const),
    }));
    hooks.page = { ...hooks.page!, turns: historicalTurns };
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });
    const view = render(<ChatSurface chatId="chat-a" />);
    const composer = screen.getByRole("textbox", { name: "Message agent" });
    for (const message of ["First", "Second"]) {
      fireEvent.change(composer, { target: { value: message } });
      fireEvent.click(screen.getByRole("button", { name: "Queue message" }));
      await waitFor(() => expect(composer).toHaveValue(""));
    }
    hooks.page = { ...hooks.page!, turns: [] };
    view.rerender(<ChatSurface chatId="chat-a" />);
    await waitFor(() => expect(hooks.actions.submit).toHaveBeenCalledTimes(1));
    expect(hooks.actions.submit.mock.calls[0]?.[0]).toBe("First");
    view.unmount();
    const returned = render(<ChatSurface chatId="chat-a" />);
    expect(hooks.actions.submit).toHaveBeenCalledTimes(1);
    hooks.page = {
      ...hooks.page!,
      turns: [{ ...historicalTurns[0], state: "completed" }],
    };
    returned.rerender(<ChatSurface chatId="chat-a" />);
    expect(hooks.actions.submit).toHaveBeenCalledTimes(1);
    hooks.page = {
      ...hooks.page!,
      turns: [
        ...hooks.page!.turns,
        {
          ...projection().turns[0],
          id: "new-turn" as never,
          ordinal: 18,
          state: "completed",
        },
      ],
    };
    returned.rerender(<ChatSurface chatId="chat-a" />);
    await waitFor(() => expect(hooks.actions.submit).toHaveBeenCalledTimes(2));
    expect(hooks.actions.submit.mock.calls[1]?.[0]).toBe("Second");
  });

  it("blocks the next queued prompt if an accepted send never starts a Turn", async () => {
    const now = vi.spyOn(Date, "now").mockReturnValue(1_000);
    hooks.actions.submit.mockResolvedValue("applied");
    matchConfiguredSession();
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });
    const view = render(<ChatSurface chatId="chat-a" />);
    const composer = screen.getByRole("textbox", { name: "Message agent" });
    for (const message of ["First", "Second"]) {
      fireEvent.change(composer, { target: { value: message } });
      fireEvent.click(screen.getByRole("button", { name: "Queue message" }));
      await waitFor(() => expect(composer).toHaveValue(""));
    }
    setTurnState("completed");
    view.rerender(<ChatSurface chatId="chat-a" />);
    await waitFor(() => expect(hooks.actions.submit).toHaveBeenCalledTimes(1));
    now.mockReturnValue(31_001);
    view.unmount();
    render(<ChatSurface chatId="chat-a" />);
    await waitFor(() => expect(screen.getByText("Needs retry")).toBeVisible());
    expect(screen.getByText("Second")).toBeVisible();
    expect(hooks.actions.submit).toHaveBeenCalledTimes(1);
    now.mockRestore();
  });

  it("caps the per-Chat queue at eight and lets the user delete a message", async () => {
    matchConfiguredSession();
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });
    render(<ChatSurface chatId="chat-a" />);
    const composer = screen.getByRole("textbox", { name: "Message agent" });
    for (let index = 1; index <= 8; index += 1) {
      fireEvent.change(composer, { target: { value: `Message ${index}` } });
      fireEvent.click(screen.getByRole("button", { name: "Queue message" }));
      await waitFor(() => expect(composer).toHaveValue(""));
    }
    expect(screen.getByLabelText("Queued messages").children).toHaveLength(8);
    expect(composer).toBeEnabled();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    expect(screen.getByText("Queue is full (8 messages).")).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", { name: "Delete queued message: Message 1" }),
    );
    await waitFor(() => expect(screen.queryByText("Message 1")).toBeNull());
    expect(composer).toBeEnabled();
    expect(hooks.actions.submit).not.toHaveBeenCalled();
  });

  it("shows bounded attachment previews for captured queued context", () => {
    const browserPage = {
      kind: "browserPage" as const,
      tabId: "browser-1",
      url: "https://example.com/report",
      title: "Report",
      text: "Summary",
    };
    const diffComment = {
      kind: "diffLineComment" as const,
      diffRef: {
        projectId: "project-a",
        chatId: "chat-a",
        sessionId: "session-a",
        runtimeEpoch: "1",
        turnId: "turn-a",
        itemId: "item-a",
        diffOrdinal: 0,
        snapshotRevision: "1",
        rootId: "root-a",
        relativePath: "src/AgentView.tsx",
        contentDigest: "a".repeat(64),
      } as never,
      side: "new" as const,
      startLine: "27",
      endLine: "27",
      note: "Keep it explicit",
    };
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
      chatUiById: {
        "chat-a": {
          composerText: "",
          attachedContext: [],
          queuedPrompts: [
            {
              id: "file",
              text: "Review this",
              attachedContext: [diffComment, browserPage],
              bindingId: "codex-app-server",
              modelId: null,
              status: "blocked",
            },
            {
              id: "page",
              text: "Summarize page",
              attachedContext: [browserPage],
              bindingId: "codex-app-server",
              modelId: null,
              status: "blocked",
            },
          ],
          sending: false,
          error: null,
          errorKind: null,
          requestGeneration: 0,
        },
      },
    });

    render(<ChatSurface chatId="chat-a" />);

    expect(
      screen.getByRole("img", {
        name: "Queued attachment preview: AgentView.tsx (2 attachments)",
      }),
    ).toHaveTextContent("AgentView.tsx");
    expect(
      screen.getByRole("img", {
        name: "Queued attachment preview: example.com (1 attachment)",
      }),
    ).toHaveTextContent("example.com");
  });

  it("keeps a rejected queued message available for retry", async () => {
    hooks.actions.submit
      .mockResolvedValueOnce("rejected")
      .mockResolvedValueOnce("applied");
    matchConfiguredSession();
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });
    const view = render(<ChatSurface chatId="chat-a" />);
    fireEvent.change(screen.getByRole("textbox", { name: "Message agent" }), {
      target: { value: "Try again" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Queue message" }));
    setTurnState("completed");
    view.rerender(<ChatSurface chatId="chat-a" />);
    await waitFor(() => expect(screen.getByText("Needs retry")).toBeVisible());
    expect(screen.getByText("Try again")).toBeVisible();
    expect(hooks.actions.submit).toHaveBeenCalledTimes(1);
    fireEvent.click(
      screen.getByLabelText("More options for queued message: Try again"),
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "Retry when ready" }));
    await waitFor(() => expect(hooks.actions.submit).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByText("Try again")).toBeNull());
  });

  it("retains a queued message while its Chat surface is unmounted", async () => {
    hooks.actions.submit.mockResolvedValue("applied");
    matchConfiguredSession();
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });
    const view = render(<ChatSurface chatId="chat-a" />);
    fireEvent.change(screen.getByRole("textbox", { name: "Message agent" }), {
      target: { value: "After return" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Queue message" }));
    view.unmount();
    expect(
      useWorkbenchStore.getState().chatUiById["chat-a"]?.queuedPrompts,
    ).toHaveLength(1);
    setTurnState("completed");
    render(<ChatSurface chatId="chat-a" />);
    await waitFor(() => expect(hooks.actions.submit).toHaveBeenCalledTimes(1));
    expect(hooks.actions.submit.mock.calls[0]?.[0]).toBe("After return");
  });

  it("publishes session controls to the Agent header without a second row", () => {
    const publish = vi.fn();
    pinAgentChatOwner("chat-a");
    render(
      <AgentSessionChromeContext.Provider value={publish}>
        <ChatSurface chatId="chat-a" />
      </AgentSessionChromeContext.Provider>,
    );
    expect(screen.queryByRole("button", { name: "Hand off chat" })).toBeNull();
    expect(screen.queryByLabelText(/^Harness status:/)).toBeNull();
    expect(document.querySelector(".surface-header")).toBeNull();
    expect(publish).toHaveBeenCalledWith(
      "chat-a",
      expect.objectContaining({ chatId: "chat-a" }),
    );
  });

  it("keeps publishing a parent's session chrome while its side chat is focused", () => {
    const publish = vi.fn();
    useSideChatState.setState({
      sides: [{ id: "side-a", projectId: "project-a", parentChatId: "chat-a" }],
    });
    pinAgentChatOwner("side-a");
    try {
      const view = render(
        <AgentSessionChromeContext.Provider value={publish}>
          <ChatSurface chatId="chat-a" />
        </AgentSessionChromeContext.Provider>,
      );
      expect(publish).toHaveBeenCalledWith(
        "chat-a",
        expect.objectContaining({ chatId: "chat-a" }),
      );
      view.unmount();
      expect(publish).toHaveBeenLastCalledWith("chat-a", null);
      publish.mockClear();
      // A Chat that is neither focused nor the side chat's parent is silent.
      render(
        <AgentSessionChromeContext.Provider value={publish}>
          <ChatSurface chatId="chat-b" />
        </AgentSessionChromeContext.Provider>,
      );
      expect(publish).not.toHaveBeenCalled();
    } finally {
      useSideChatState.setState({ sides: [] });
    }
  });

  it("renders no header of its own under Agent chrome, side chats included", () => {
    // The task header names the Chat; a side chat's tab lives in the Agent
    // center header row (Codex s2/f0244).
    const publish = vi.fn();
    for (const chatId of ["side-chat-a", "chat-a"]) {
      const view = render(
        <AgentSessionChromeContext.Provider value={publish}>
          <ChatSurface chatId={chatId} />
        </AgentSessionChromeContext.Provider>,
      );
      expect(
        document.querySelector("section.chat-surface > header"),
      ).toBeNull();
      view.unmount();
    }
  });

  it("renders only provider-advertised session modes and preserves their order", () => {
    hooks.page = {
      ...hooks.page!,
      turns: [],
      sessionConfig: {
        sageAgentSessionId: "session-a",
        runtimeEpoch: "1",
        configRevision: "1",
        modes: {
          currentModeId: "review",
          availableModes: [
            { id: "review", name: "Review changes" },
            { id: "apply", name: "Apply changes" },
          ],
        },
        configOptions: [],
      },
    } as AcpChatPage;
    matchConfiguredSession();
    render(<ChatSurface />);
    openComposerOptions();

    const modes = screen.getByRole("group", { name: "Session mode" });
    expect(
      within(modes)
        .getAllByRole("menuitemradio")
        .map((option) => option.getAttribute("aria-label")),
    ).toEqual(["Review changes", "Apply changes"]);
    expect(
      within(modes).getByRole("menuitemradio", { name: "Review changes" }),
    ).toHaveAttribute("aria-checked", "true");
    fireEvent.click(
      within(modes).getByRole("menuitemradio", { name: "Apply changes" }),
    );
    expect(hooks.actions.setSessionMode).toHaveBeenCalledExactlyOnceWith(
      "apply",
    );
  });

  it("does not render a session mode picker without advertised modes", () => {
    render(<ChatSurface />);
    openComposerOptions();
    expect(screen.queryByRole("button", { name: "Session mode" })).toBeNull();
  });

  it("renders advertised config options without restating the mode picker", async () => {
    hooks.page = {
      ...hooks.page!,
      turns: [],
      sessionConfig: {
        sageAgentSessionId: "session-a",
        runtimeEpoch: "1",
        configRevision: "1",
        modes: {
          currentModeId: "default",
          availableModes: [
            { id: "default", name: "Manual" },
            { id: "plan", name: "Plan" },
          ],
        },
        configOptions: [
          {
            id: "mode",
            name: "Mode",
            type: "select",
            currentValue: "default",
            options: [
              { value: "default", name: "Manual" },
              { value: "plan", name: "Plan" },
            ],
          },
          {
            id: "model",
            name: "Model",
            category: "model",
            type: "select",
            currentValue: "sonnet",
            options: [
              { value: "sonnet", name: "Sonnet 5" },
              { value: "haiku", name: "Haiku 4.5" },
            ],
          },
          {
            id: "fast",
            name: "Fast mode",
            type: "boolean",
            currentValue: false,
          },
        ],
      },
    } as AcpChatPage;
    matchConfiguredSession();
    render(<ChatSurface />);
    openComposerOptions();

    expect(screen.getByRole("group", { name: "Session mode" })).toBeTruthy();
    expect(
      screen.queryAllByRole("group", { name: "Session mode" }),
    ).toHaveLength(1);
    expect(
      screen.getByRole("menuitemcheckbox", { name: "Session fast mode" }),
    ).not.toBeChecked();
    openComposerModel();
    expect(
      await screen.findByRole("option", { name: /Sonnet 5/ }),
    ).toBeTruthy();
  });

  it("omits unsupported config actions instead of enabling an inert Add menu", async () => {
    hooks.page = {
      ...hooks.page!,
      turns: [],
      sessionConfig: {
        sageAgentSessionId: "session-a",
        runtimeEpoch: "1",
        configRevision: "1",
        modes: null,
        configOptions: [
          {
            id: "budget",
            name: "Thinking budget",
            type: "unsupported",
            kind: "integer",
          },
          {
            id: "model",
            name: "Model",
            type: "select",
            currentValue: "sonnet",
            options: [{ value: "sonnet", name: "Sonnet 5" }],
          },
        ],
      },
    } as AcpChatPage;
    matchConfiguredSession();
    render(<ChatSurface />);
    openComposerOptions();

    expect(screen.queryByText("Thinking budget")).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Composer options" }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Session thinking budget" }),
    ).toBeNull();
    expect(
      screen.queryByRole("checkbox", { name: "Session thinking budget" }),
    ).toBeNull();
    openComposerModel();
    expect(
      await screen.findByRole("option", { name: /Sonnet 5/ }),
    ).toBeTruthy();
  });

  it("lets a prompt-capable harness without a model catalog use its own default", () => {
    hooks.page = {
      ...projection(),
      turns: [],
      sessions: [
        {
          id: "session-a",
          bindingId: "claude-agent-acp",
          modelId: null,
          state: "active",
          capabilities: { prompt: true },
        },
      ],
    } as unknown as AcpChatPage;
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "claude-code",
          name: "Claude Code",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "claude-agent-acp",
          accessModes: [
            {
              id: "ask",
              label: "Ask each time",
              description: "Review permission requests.",
              fallback: true,
            },
          ],
        },
      ],
      models: [],
      selectedHarnessId: "claude-code",
      selectedModelId: null,
    });

    render(<ChatSurface />);
    openComposerOptions();

    expect(
      screen.getByRole("textbox", { name: "Message agent" }),
    ).toBeEnabled();
    expect(screen.getByRole("button", { name: "Model" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Harness" })).toBeEnabled();
  });

  it("treats an available selected harness without a session as startable", () => {
    hooks.page = {
      ...projection(),
      currentSessionId: null,
      sessions: [],
      turns: [],
    } as AcpChatPage;
    useWorkbenchStore.setState({
      models: [{ id: "gpt-6-astra", name: "GPT-6-Astra" }],
      selectedModelId: "gpt-6-astra",
    });

    render(<ChatSurface />);

    expect(
      screen.getByRole("textbox", { name: "Message agent" }),
    ).toBeEnabled();
    expect(
      screen.queryByText(
        "Choose an available harness before starting a session",
      ),
    ).toBeNull();
    expect(
      screen.queryByText(
        "The selected Harness cannot accept prompts in this session.",
      ),
    ).toBeNull();
  });

  it("explains when the selected ready session cannot accept prompts", () => {
    hooks.page = {
      ...projection(),
      turns: [],
      sessions: [
        {
          id: "session-a",
          ordinal: 0,
          runtimeEpoch: "1",
          nextRuntimeEpoch: null,
          state: "ready",
          residency: "hot",
          legacy: false,
          bindingId: "codex-app-server",
          modelId: null,
          capabilities: { prompt: false, closeSession: false },
          revision: "1",
          projectionRevision: "2",
        },
      ],
    } as unknown as AcpChatPage;

    render(<ChatSurface />);

    expect(screen.getByLabelText("Harness status: Ready")).toBeInTheDocument();
    expect(
      screen.getByRole("textbox", { name: "Message agent" }),
    ).toHaveAttribute("placeholder", "This session cannot accept prompts.");
    expect(
      screen.getByText(
        "The selected Harness cannot accept prompts in this session.",
      ),
    ).toBeInTheDocument();
  });

  it("shows the selected harness even when the current session uses another", () => {
    hooks.page = {
      ...projection(),
      turns: [],
      sessions: [
        {
          id: "session-a",
          ordinal: 0,
          runtimeEpoch: "1",
          nextRuntimeEpoch: null,
          state: "active",
          residency: "hot",
          legacy: false,
          bindingId: "claude-agent-acp",
          modelId: null,
          capabilities: { prompt: true, closeSession: true },
          revision: "1",
          projectionRevision: "2",
        },
      ],
    } as unknown as AcpChatPage;
    useWorkbenchStore.setState({
      harnesses: [
        ...useWorkbenchStore.getState().harnesses,
        {
          id: "claude-code",
          name: "Claude Code",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "claude-agent-acp",
          accessModes: [
            {
              id: "ask",
              label: "Ask each time",
              description: "Review permission requests.",
              fallback: true,
            },
          ],
        },
      ],
    });

    render(<ChatSurface />);
    expect(screen.getByRole("button", { name: "Harness" })).toHaveTextContent(
      "Codex",
    );
  });

  it("removes a withdrawn session mode picker and returns focus to the composer", async () => {
    hooks.page = {
      ...hooks.page!,
      turns: [],
      sessionConfig: {
        sageAgentSessionId: "session-a",
        runtimeEpoch: "1",
        configRevision: "1",
        modes: {
          currentModeId: "review",
          availableModes: [{ id: "review", name: "Review changes" }],
        },
        configOptions: [],
      },
    } as AcpChatPage;
    matchConfiguredSession();
    const { rerender } = render(<ChatSurface />);
    openComposerOptions();
    const modeInput = screen.getByRole("menuitemradio", {
      name: "Review changes",
    });
    await waitFor(() => expect(modeInput).toHaveFocus());
    hooks.page = {
      ...hooks.page!,
      sessionConfig: {
        sageAgentSessionId: "session-a",
        runtimeEpoch: "1",
        configRevision: "2",
        removed: true,
        modes: null,
        configOptions: [],
      },
    } as AcpChatPage;
    rerender(<ChatSurface />);

    expect(screen.queryByRole("group", { name: "Session mode" })).toBeNull();
    expect(
      screen.queryByRole("menu", { name: "Composer options menu" }),
    ).toBeNull();
    await waitFor(() =>
      expect(
        screen.getByRole("textbox", { name: "Message agent" }),
      ).toHaveFocus(),
    );
  });

  it("does not move focus when an unfocused Chat withdraws its mode picker", () => {
    pinAgentChatOwner("chat-a");
    hooks.page = {
      ...hooks.page!,
      turns: [],
      sessionConfig: {
        sageAgentSessionId: "session-a",
        runtimeEpoch: "1",
        configRevision: "1",
        modes: {
          currentModeId: "review",
          availableModes: [{ id: "review", name: "Review changes" }],
        },
        configOptions: [],
      },
    } as AcpChatPage;
    const { rerender } = render(
      <>
        <textarea aria-label="Foreground draft" />
        <ChatSurface chatId="chat-b" />
      </>,
    );
    const foregroundDraft = screen.getByRole("textbox", {
      name: "Foreground draft",
    });
    foregroundDraft.focus();
    hooks.page = {
      ...hooks.page!,
      sessionConfig: {
        sageAgentSessionId: "session-a",
        runtimeEpoch: "1",
        configRevision: "2",
        removed: true,
        modes: null,
        configOptions: [],
      },
    } as AcpChatPage;
    rerender(
      <>
        <textarea aria-label="Foreground draft" />
        <ChatSurface chatId="chat-b" />
      </>,
    );

    expect(foregroundDraft).toHaveFocus();
  });

  it("announces pending mode state and errors outside the action menu", () => {
    hooks.page = {
      ...hooks.page!,
      turns: [],
      sessionConfig: {
        sageAgentSessionId: "session-a",
        runtimeEpoch: "1",
        configRevision: "1",
        modes: {
          currentModeId: "review",
          availableModes: [
            { id: "review", name: "Review changes" },
            { id: "apply", name: "Apply changes" },
          ],
        },
        configOptions: [],
      },
    } as AcpChatPage;
    matchConfiguredSession();
    hooks.actions.modeChanging = true;
    hooks.actions.modeError = "Mode change was rejected";
    hooks.actions.error = "Prompt dispatch failed";
    render(<ChatSurface />);
    openComposerOptions();

    const modeTrigger = screen.getByRole("menuitemradio", {
      name: "Review changes",
    });
    expect(modeTrigger).toHaveAttribute("aria-checked", "true");
    expect(modeTrigger).toBeDisabled();
    const pending = screen.getByText("Changing session settings…");
    expect(pending).toHaveAttribute("role", "status");
    const menu = screen.getByRole("menu", { name: "Composer options menu" });
    expect(menu).not.toContainElement(pending);
    expect(screen.getByText("Prompt dispatch failed")).toHaveClass(
      "composer__error",
    );
    const error = screen.getByText("Mode change was rejected");
    expect(error).toHaveAttribute("role", "alert");
    expect(menu).not.toContainElement(error);
  });

  it("offers a clicked-only Claude login and an explicit sign-in recheck", async () => {
    const start = vi
      .spyOn(acpDesktop, "startClaudeAuthLogin")
      .mockResolvedValue();
    const status = vi
      .spyOn(acpDesktop, "getClaudeAuthStatus")
      .mockResolvedValue({ loggedIn: true });
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "claude-code",
          name: "Claude Code",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "claude-agent-acp",
          accessModes: [
            {
              id: "ask",
              label: "Ask each time",
              description: "Review permission requests.",
              fallback: true,
            },
          ],
          signIn: {
            command: "claude auth login",
            hint: "Complete sign-in in the terminal.",
          },
        },
      ],
      selectedHarnessId: "claude-code",
      harnessDiscoveryPhase: "ready",
    });
    hooks.actions.error =
      "The harness requires sign-in before it can continue.";
    hooks.actions.errorKind = "signInRequired";

    render(<ChatSurface />);

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Claude Code requires sign-in before it can continue. Run claude auth login. Complete sign-in in the terminal. Then try again.",
    );
    expect(start).not.toHaveBeenCalled();
    fireEvent.click(
      await screen.findByRole("button", { name: "Open Claude sign-in" }),
    );
    await waitFor(() => expect(start).toHaveBeenCalledOnce());
    fireEvent.click(screen.getByRole("button", { name: "Check sign-in" }));
    await waitFor(() => expect(status).toHaveBeenCalledOnce());
    expect(
      screen.getByText("Claude reports signed in. Send your prompt again."),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("textbox", { name: "Message agent" }),
    ).toBeEnabled();
    let finishCheck: (value: { loggedIn: boolean }) => void = () => {};
    status.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishCheck = resolve;
        }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Check sign-in" }));
    act(() => {
      useWorkbenchStore.setState({
        nativeTransitionEpoch:
          useWorkbenchStore.getState().nativeTransitionEpoch + 1,
      });
    });
    await act(async () => finishCheck({ loggedIn: true }));
    expect(
      screen.queryByText("Claude reports signed in. Send your prompt again."),
    ).not.toBeInTheDocument();
    status.mockRejectedValueOnce(new Error("secret detail"));
    fireEvent.click(screen.getByRole("button", { name: "Check sign-in" }));
    expect(
      await screen.findByText(
        "Could not start or check sign-in. Run claude auth login in a terminal.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText("secret detail")).not.toBeInTheDocument();
  });

  it("satisfies the exported ACP-04 Codex-ready selector with real chat controls", async () => {
    hooks.page = {
      ...projection(),
      currentSessionId: null,
      sessions: [],
      turns: [],
    };
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "codex",
          name: "Codex",
          version: "codex 0.146.0",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: true,
          transport: "acp",
          bindingId: "codex-app-server",
          accessModes: nativeReadOnly,
        },
      ],
      models: [{ id: "gpt-5.6", name: "gpt-5.6" }],
      selectedModelId: "gpt-5.6",
    });
    render(
      <main className="workbench workbench--agent">
        <div role="radiogroup" aria-label="Window mode">
          <input type="radio" name="workbench-window-mode" value="editor" />
          <input
            type="radio"
            name="workbench-window-mode"
            value="agent"
            defaultChecked
          />
        </div>
        <section className="agent-chat-pane agent-chat-pane--focused">
          <ChatSurface />
        </section>
      </main>,
    );
    openComposerOptions();

    expect(screen.getByRole("button", { name: "Harness" })).toBeVisible();
  });

  it("follows an advancing ACP streaming projection to its latest content", () => {
    const scrollTo = vi.mocked(Element.prototype.scrollTo);
    const { rerender } = render(<ChatSurface />);
    measureTranscript({ scrollTop: 600 });
    scrollTo.mockClear();

    hooks.page = {
      ...hooks.page!,
      projectionRevision: "3" as never,
      turns: hooks.page!.turns.map((turn) => ({
        ...turn,
        items: turn.items.map((item) =>
          item.kind === "toolCall"
            ? {
                ...item,
                payload: {
                  ...item.payload,
                  inputSummary: "Streaming the next retained update",
                },
              }
            : item,
        ),
      })),
    };
    rerender(<ChatSurface />);

    expect(scrollTo).toHaveBeenCalledWith({ behavior: "smooth", top: 600 });
  });

  it("follows without smooth motion when reduced motion is requested", () => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn(
        (query) =>
          ({
            matches: query === "(prefers-reduced-motion: reduce)",
          }) as MediaQueryList,
      ),
    );
    const scrollTo = vi.mocked(Element.prototype.scrollTo);
    const { rerender } = render(<ChatSurface />);
    measureTranscript({ scrollTop: 600 });
    scrollTo.mockClear();

    hooks.page = {
      ...hooks.page!,
      projectionRevision: "3" as never,
    };
    rerender(<ChatSurface />);

    expect(scrollTo).toHaveBeenCalledWith({ behavior: "auto", top: 600 });
  });

  it("stops following when the reader scrolls away and offers a return to latest", () => {
    // Synchronous frames 16 ms apart, so the timed jump runs to its end.
    let clock = 0;
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      callback((clock += 16));
      return 1;
    });
    const scrollTo = vi.mocked(Element.prototype.scrollTo);
    const { rerender } = render(<ChatSurface />);
    const transcript = measureTranscript({ scrollTop: 100 });

    fireEvent.scroll(transcript);
    scrollTo.mockClear();
    hooks.page = {
      ...hooks.page!,
      projectionRevision: "4" as never,
    };
    rerender(<ChatSurface />);

    expect(scrollTo).not.toHaveBeenCalled();
    const returnToLatest = screen.getByRole("button", {
      name: "Jump to latest",
    });
    fireEvent.click(returnToLatest);
    expect(scrollTo).toHaveBeenCalledWith({ behavior: "auto", top: 600 });
  });

  it("coalesces a scroll burst and cancels pending follow work on owner change or unmount", () => {
    const request = vi.fn((_: FrameRequestCallback) => 17);
    const cancel = vi.fn();
    vi.stubGlobal("requestAnimationFrame", request);
    vi.stubGlobal("cancelAnimationFrame", cancel);
    const { rerender, unmount } = render(<ChatSurface chatId="chat-a" />);
    const transcript = document.querySelector(".message-list");
    expect(transcript).not.toBeNull();
    for (let index = 0; index < 24; index += 1) fireEvent.scroll(transcript!);
    expect(request).toHaveBeenCalledTimes(1);
    rerender(<ChatSurface chatId="chat-b" />);
    expect(cancel).toHaveBeenCalledWith(17);
    fireEvent.scroll(document.querySelector(".message-list")!);
    expect(request).toHaveBeenCalledTimes(2);
    unmount();
    expect(cancel).toHaveBeenCalledTimes(2);
  });

  it("keeps selected not-installed Codex truthful and hides later built-in harnesses", () => {
    hooks.page = null;
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "codex",
          name: "Codex",
          description: "ACP",
          installState: "notInstalled",
          source: "builtIn",
          accountModelDiscovery: true,
          transport: "acp",
          bindingId: "codex-app-server",
          accessModes: nativeReadOnly,
        },
        {
          id: "pi",
          name: "Pi",
          description: "Later harness",
          installState: "notInstalled",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: null,
        },
        {
          id: "claude-code",
          name: "Claude Code",
          description: "Later harness",
          installState: "notInstalled",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: null,
        },
        {
          id: "oh-my-pi",
          name: "Oh My Pi",
          description: "Later harness",
          installState: "notInstalled",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: null,
        },
        {
          id: "sage-simulator",
          name: "Simulator",
          description: "Development",
          installState: "available",
          source: "development",
          accountModelDiscovery: false,
          transport: "legacySimulator",
          bindingId: null,
        },
      ],
      selectedHarnessId: "codex",
    });

    render(<ChatSurface />);
    openComposerOptions();

    expect(screen.getByText("Unavailable")).toBeInTheDocument();
    expect(screen.queryByText("Live")).toBeNull();
    expect(screen.queryByRole("button", { name: "Attach context" })).toBeNull();
    const harness = screen.getByRole("button", { name: "Harness" });
    expect(
      screen.queryByRole("button", { name: "Permission mode" }),
    ).toBeNull();
    fireEvent.click(harness);
    const options = screen.getAllByRole("option");
    expect(options).toHaveLength(5);
    expect(options[0]).toHaveTextContent(/^Simulator$/);
    expect(options[0]).toHaveAccessibleName("Simulator — test only");
    expect(options[1]).toHaveAccessibleName(
      "Codex — not installed. Harness is not installed.",
    );
    expect(options[1]).toBeDisabled();
    expect(options[2]).toHaveAccessibleName(
      "Pi — not installed. Harness is not installed.",
    );
    expect(options[3]).toHaveAccessibleName(
      "Claude Code — not installed. Harness is not installed.",
    );
    expect(options[4]).toHaveAccessibleName(
      "Oh My Pi — not installed. Harness is not installed.",
    );
    expect(
      options.slice(1).every((option) => option.hasAttribute("disabled")),
    ).toBe(true);
    expect(
      options.every((option) => option.querySelector("small") === null),
    ).toBe(true);
  });

  it("shows present-but-not-admitted Codex as unavailable without hiding the simulator", () => {
    hooks.page = null;
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "codex",
          name: "Codex",
          description: "ACP",
          installState: "unavailable",
          source: "builtIn",
          accountModelDiscovery: true,
          transport: "acp",
          bindingId: "codex-app-server",
          accessModes: nativeReadOnly,
        },
        {
          id: "pi",
          name: "Pi",
          description: "Later harness",
          installState: "notInstalled",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: null,
        },
        {
          id: "sage-simulator",
          name: "Simulator",
          description: "Development",
          installState: "available",
          source: "development",
          accountModelDiscovery: false,
          transport: "legacySimulator",
          bindingId: null,
        },
      ],
      selectedHarnessId: "sage-simulator",
    });

    render(<ChatSurface />);
    openComposerOptions();

    const harness = screen.getByRole("button", { name: "Harness" });
    fireEvent.click(harness);
    const options = screen.getAllByRole("option");
    expect(options).toHaveLength(3);
    expect(options[0]).toHaveTextContent(/^Simulator$/);
    expect(options[0]).toHaveAccessibleName("Simulator — test only");
    expect(options[1]).toHaveAccessibleName(
      "Codex — installed but not drivable. Sage did not admit this harness and will not start it.",
    );
    expect(options[1]).toBeDisabled();
    expect(options[2]).toHaveAccessibleName(
      "Pi — not installed. Harness is not installed.",
    );
    expect(options[2]).toBeDisabled();
  });

  it("keeps the available development fallback visible when the selected harness is absent", () => {
    hooks.page = null;
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "pi",
          name: "Pi",
          description: "Later harness",
          installState: "notInstalled",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: null,
        },
        {
          id: "sage-simulator",
          name: "Simulator",
          description: "Development",
          installState: "available",
          source: "development",
          accountModelDiscovery: false,
          transport: "legacySimulator",
          bindingId: null,
        },
      ],
      selectedHarnessId: "removed-harness",
    });

    render(<ChatSurface />);
    openComposerOptions();

    expect(screen.getByText("No harness")).toBeInTheDocument();
    const harness = screen.getByRole("button", { name: "Harness" });
    fireEvent.click(harness);
    const options = screen.getAllByRole("option");
    expect(options).toHaveLength(2);
    expect(options[0]).toHaveTextContent(/^Simulator$/);
    expect(options[0]).toHaveAccessibleName("Simulator — test only");
    expect(options[1]).toHaveAccessibleName(
      "Pi — not installed. Harness is not installed.",
    );
    expect(options[1]).toBeDisabled();
  });

  it("marks the development simulator as test-only without policy claims", () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "sage-simulator",
          name: "Simulator",
          description: "Development",
          installState: "available",
          source: "development",
          accountModelDiscovery: false,
          transport: "legacySimulator",
          bindingId: null,
        },
      ],
      selectedHarnessId: "sage-simulator",
    });

    render(<ChatSurface />);

    expect(screen.getByText("Test simulator")).toBeInTheDocument();
    expect(screen.getByText("Development simulator")).toBeInTheDocument();
    expect(
      screen.getByText(
        "This test-only harness exercises the workbench without connecting a production agent.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText("Phase 0 has started")).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Permission mode" }),
    ).toBeNull();
    expect(
      screen.queryByText(
        "No reviewed local binding is available for this harness.",
      ),
    ).toBeNull();
  });

  it("shows an available ACP binding as ready before a session starts", () => {
    hooks.page = { ...hooks.page!, turns: [] };
    render(<ChatSurface />);
    openComposerOptions();

    expect(screen.getByText("Ready")).toBeInTheDocument();
    expect(screen.queryByText("Live")).toBeNull();
    expect(
      screen.getByRole("button", { name: "Access options" }),
    ).toHaveTextContent("Read-only");
  });

  it("keeps Ask unavailable for fresh characterised Codex 0.146 without native approval proof", () => {
    hooks.page = {
      ...projection(),
      currentSessionId: null,
      sessions: [],
      turns: [],
    };
    useSessionAccessModeStore.setState({
      modes: { "codex-app-server": "readOnly" },
      ready: true,
      error: null,
    });
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "codex",
          name: "Codex",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: true,
          transport: "acp",
          bindingId: "codex-app-server",
          boundVersion: "0.146.0",
          characterisation: "characterised",
          accessModes: nativeReadOnly,
        },
      ],
      selectedHarnessId: "codex",
    });

    render(<ChatSurface />);
    openComposerOptions();
    fireEvent.click(screen.getByRole("button", { name: "Access options" }));
    expect(
      screen.queryByRole("menuitemradio", { name: "Ask each time" }),
    ).toBeNull();
    expect(useSessionAccessModeStore.getState().modes["codex-app-server"]).toBe(
      "readOnly",
    );
  });

  it("shows effective Read-only for saved Ask across observed and pinned Codex", async () => {
    hooks.page = {
      ...projection(),
      currentSessionId: null,
      sessions: [],
      turns: [],
    };
    hooks.actions.submit.mockResolvedValue("applied");
    useSessionAccessModeStore.setState({
      modes: { "codex-app-server": "ask" },
      ready: true,
      error: null,
    });
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "codex",
          name: "Codex observed",
          accessModes: nativeReadOnly,
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: true,
          transport: "acp",
          bindingId: "codex-app-server",
          boundVersion: "0.146.0",
          characterisation: "characterised",
        },
        {
          id: "pinned-codex",
          name: "Codex pinned",
          accessModes: nativeReadOnly,
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: true,
          transport: "acp",
          bindingId: "codex-app-server",
          boundVersion: "0.145.0",
          characterisation: "characterised",
        },
      ],
      selectedHarnessId: "codex",
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });
    render(<ChatSurface />);
    expect(screen.getByText("Read-only")).toBeVisible();

    act(() =>
      useWorkbenchStore.setState({ selectedHarnessId: "pinned-codex" }),
    );
    expect(screen.getByText("Read-only")).toBeVisible();
    fireEvent.change(screen.getByRole("textbox", { name: "Message agent" }), {
      target: { value: "Inspect safely" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() =>
      expect(hooks.actions.submit).toHaveBeenCalledWith(
        "Inspect safely",
        [],
        "codex-app-server",
        null,
      ),
    );
    expect(useSessionAccessModeStore.getState().modes["codex-app-server"]).toBe(
      "ask",
    );
  });

  it.each([
    [
      "pinned Codex 0.145",
      {
        id: "codex",
        bindingId: "codex-app-server",
        boundVersion: "0.145.0",
        characterisation: "characterised",
      },
    ],
    [
      "uncharacterised Codex 0.146",
      {
        id: "codex",
        bindingId: "codex-app-server",
        boundVersion: "0.146.0",
        characterisation: "uncharacterised",
      },
    ],
    [
      "Claude",
      {
        id: "claude",
        bindingId: "claude-acp",
        boundVersion: "0.146.0",
        characterisation: "characterised",
      },
    ],
    [
      "OpenCode",
      {
        id: "opencode",
        bindingId: "opencode-acp",
        boundVersion: "0.146.0",
        characterisation: "characterised",
      },
    ],
  ] as const)(
    "does not infer access capabilities from a harness identity alone for %s",
    (_label, identity) => {
      hooks.page = {
        ...projection(),
        currentSessionId: null,
        sessions: [],
        turns: [],
      };
      useSessionAccessModeStore.setState({
        modes: { "codex-app-server": "readOnly" },
        ready: true,
        error: null,
      });
      useWorkbenchStore.setState({
        harnesses: [
          {
            name: identity.id,
            description: "ACP",
            installState: "available",
            source: "builtIn",
            accountModelDiscovery: true,
            transport: "acp",
            ...identity,
          },
        ],
        selectedHarnessId: identity.id,
      });

      render(<ChatSurface />);
      expect(
        screen.queryByRole("button", { name: "Access options" }),
      ).toBeNull();
      expect(
        screen.getByRole("textbox", { name: "Message agent" }),
      ).toBeDisabled();
      expect(
        screen.getByText("Choose a supported access mode before sending."),
      ).toBeVisible();
    },
  );

  it("does not project an active session onto a different selected model or binding", () => {
    hooks.page = {
      ...hooks.page!,
      currentSessionId: "session-a" as never,
      sessions: [
        {
          id: "session-a",
          ordinal: 0,
          runtimeEpoch: "1",
          nextRuntimeEpoch: null,
          state: "active",
          residency: "hot",
          legacy: false,
          bindingId: "codex-app-server",
          modelId: "codex-old",
          capabilities: { closeSession: true },
          revision: "1",
          projectionRevision: "2",
        },
      ],
    } as unknown as AcpChatPage;
    useWorkbenchStore.setState({
      models: [
        { id: "codex-old", name: "Codex old" },
        { id: "codex-new", name: "Codex new" },
      ],
      selectedModelId: "codex-new",
    });
    render(<ChatSurface />);

    expect(screen.getByLabelText("Harness status: Ready")).toBeInTheDocument();
    expect(screen.queryByText("Live")).toBeNull();

    act(() => {
      useWorkbenchStore.setState({
        harnesses: [
          ...useWorkbenchStore.getState().harnesses,
          {
            id: "pi",
            name: "Pi",
            description: "Second admitted binding",
            installState: "available",
            source: "builtIn",
            accountModelDiscovery: false,
            transport: "acp",
            bindingId: "pi-acp",
          },
        ],
        selectedHarnessId: "pi",
        selectedModelId: "codex-old",
      });
    });

    expect(screen.getByLabelText("Harness status: Ready")).toBeInTheDocument();
    expect(screen.queryByText("Live")).toBeNull();
  });

  it("announces the active Chat failure and exposes the bounded composer", () => {
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat: A",
          position: 0,
          pinned: true,
          dirty: false,
          status: "idle",
        },
      ],
      activeTabId: "chat-a",
      harnesses: [
        {
          id: "sage-simulator",
          name: "Simulator",
          description: "Development",
          installState: "available",
          source: "development",
          accountModelDiscovery: false,
          transport: "legacySimulator",
          bindingId: null,
        },
      ],
      selectedHarnessId: "sage-simulator",
      chatError: "Only Chat A failed",
    });
    render(<ChatSurface />);

    expect(screen.getByRole("alert")).toHaveTextContent("Only Chat A failed");
    expect(
      screen.getByRole("textbox", { name: "Message agent" }),
    ).toHaveAttribute("maxlength", "16384");
  });

  it("previews edits cancels and confirms a bounded unsent Chat Handoff", async () => {
    const handoffChat = vi.fn(async () => true);
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      projectGeneration: 4,
      nativeTransitionEpoch: 7,
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat: Architecture",
          position: 0,
          pinned: true,
          dirty: false,
          status: "idle",
        },
        {
          id: "chat-b",
          kind: "chat",
          title: "Chat: Testing",
          position: 1,
          pinned: false,
          dirty: false,
          status: "idle",
        },
        ...Array.from({ length: 62 }, (_, index) => ({
          id: `handoff-target-${index + 2}`,
          kind: "chat" as const,
          title: `Chat: Indexed target ${index + 2}`,
          position: index + 2,
          pinned: false,
          dirty: false,
          status: "idle" as const,
        })),
      ],
      activeTabId: "chat-a",
      harnesses: [
        {
          id: "sage-simulator",
          name: "Simulator",
          description: "Development",
          installState: "available",
          source: "development",
          accountModelDiscovery: false,
          transport: "legacySimulator",
          bindingId: null,
        },
      ],
      selectedHarnessId: "sage-simulator",
      messages: [
        {
          id: "message-a",
          sessionId: "session-a",
          sequence: 1,
          role: "user",
          text: "Keep SQLite authoritative.",
          createdAt: 1,
        },
      ],
      composerText: "source-only draft",
      handoffChat,
    });
    render(<ChatSurface />);

    const trigger = screen.getByRole("button", { name: "Hand off chat" });
    fireEvent.click(trigger);
    // The dialog loads in its own lazy chunk on the first Hand off.
    const dialog = await screen.findByRole("dialog", {
      name: "Hand off this Chat",
    });
    expect(
      screen.getByRole("button", { name: "Target Chat" }),
    ).toHaveTextContent("Testing");
    fireEvent.click(screen.getByRole("button", { name: "Target Chat" }));
    const targetPicker = screen.getByRole("combobox", {
      name: "Target Chat",
    });
    const targetListbox = screen.getByRole("listbox", {
      name: "Target Chat",
    });
    expect(dialog).toContainElement(targetPicker);
    expect(dialog).toContainElement(targetListbox);
    expect(screen.getAllByRole("option")).toHaveLength(63);
    expect(screen.getByRole("option", { name: "Testing" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByText("Current: Testing")).toBeVisible();
    await waitFor(() => expect(targetPicker).toHaveFocus());
    fireEvent.keyDown(targetPicker, { key: "ArrowDown" });
    fireEvent.keyDown(targetPicker, { key: "ArrowDown" });
    fireEvent.keyDown(targetPicker, { key: "Enter" });
    expect(
      screen.getByRole("button", { name: "Target Chat" }),
    ).toHaveTextContent("Indexed target 2");
    expect(screen.getByRole("button", { name: "Target Chat" })).toHaveFocus();
    const preview = screen.getByRole("textbox", {
      name: "Handoff prompt",
    });
    expect((preview as HTMLTextAreaElement).value).toContain(
      'Chat handoff from "Architecture"',
    );
    expect((preview as HTMLTextAreaElement).value).toContain(
      "User: Keep SQLite authoritative.",
    );
    fireEvent.change(preview, { target: { value: "Edited ordinary prompt" } });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(handoffChat).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState().composerText).toBe("source-only draft");
    await waitFor(() => expect(trigger).toHaveFocus());

    fireEvent.click(trigger);
    act(() => useWorkbenchStore.setState({ projectTransitioning: true }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(handoffChat).not.toHaveBeenCalled();
    act(() => useWorkbenchStore.setState({ projectTransitioning: false }));
    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole("button", { name: "Target Chat" }));
    const reopenedTargetPicker = screen.getByRole("combobox", {
      name: "Target Chat",
    });
    fireEvent.keyDown(reopenedTargetPicker, { key: "ArrowDown" });
    fireEvent.keyDown(reopenedTargetPicker, { key: "ArrowDown" });
    fireEvent.keyDown(reopenedTargetPicker, { key: "Enter" });
    fireEvent.change(screen.getByRole("textbox", { name: "Handoff prompt" }), {
      target: { value: "Edited ordinary prompt" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Place in target" }));
    await waitFor(() =>
      expect(handoffChat).toHaveBeenCalledWith({
        projectId: "project-a",
        projectGeneration: 4,
        transitionEpoch: 7,
        sourceChatId: "chat-a",
        targetChatId: "handoff-target-2",
        draft: "Edited ordinary prompt",
      }),
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() =>
      expect(
        screen.getByRole("textbox", { name: "Message agent" }),
      ).toHaveFocus(),
    );
    expect(hooks.actions.submit).not.toHaveBeenCalled();
  });

  it("includes only primary ACP text in the Chat Handoff preview", async () => {
    const base = projection();
    hooks.page = {
      ...base,
      turns: [
        {
          ...base.turns[0],
          state: "completed",
          terminalAt: 3,
          items: [
            {
              ...itemBase("item-primary-text"),
              kind: "text",
              toolState: null,
              payload: { role: "assistant", text: "Primary response" },
            },
            {
              ...itemBase("item-child-text"),
              origin: "harnessActivity",
              activityId: "activity-child",
              kind: "text",
              toolState: null,
              payload: { role: "assistant", text: "Private child work" },
            },
          ] as unknown as AcpItem[],
        },
      ],
    };
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      projectGeneration: 4,
      nativeTransitionEpoch: 7,
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat: Architecture",
          position: 0,
          pinned: true,
          dirty: false,
          status: "idle",
        },
        {
          id: "chat-b",
          kind: "chat",
          title: "Chat: Testing",
          position: 1,
          pinned: false,
          dirty: false,
          status: "idle",
        },
      ],
      activeTabId: "chat-a",
    });
    render(<ChatSurface />);

    fireEvent.click(screen.getByRole("button", { name: "Hand off chat" }));
    const preview = await screen.findByRole("textbox", {
      name: "Handoff prompt",
    });
    expect((preview as HTMLTextAreaElement).value).toContain(
      "Primary response",
    );
    expect((preview as HTMLTextAreaElement).value).not.toContain(
      "Private child work",
    );
  });

  it("contains modal focus at both boundaries and closes on Escape", async () => {
    setTurnState("completed");
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      projectGeneration: 4,
      nativeTransitionEpoch: 7,
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat: Architecture",
          position: 0,
          pinned: true,
          dirty: false,
          status: "idle",
        },
        {
          id: "chat-b",
          kind: "chat",
          title: "Chat: Testing",
          position: 1,
          pinned: false,
          dirty: false,
          status: "idle",
        },
      ],
      activeTabId: "chat-a",
      messages: [],
    });
    render(<ChatSurface />);

    const trigger = screen.getByRole("button", { name: "Hand off chat" });
    fireEvent.click(trigger);
    const dialog = await screen.findByRole("dialog", {
      name: "Hand off this Chat",
    });
    const first = screen.getByRole("button", { name: "Target Chat" });
    const last = screen.getByRole("button", { name: "Place in target" });
    fireEvent.click(first);
    const pickerInput = screen.getByRole("combobox", { name: "Target Chat" });
    expect(dialog.contains(pickerInput)).toBe(true);
    await waitFor(() => expect(pickerInput).toHaveFocus());
    const forwardTab = createEvent.keyDown(pickerInput, { key: "Tab" });
    fireEvent(pickerInput, forwardTab);
    expect(forwardTab.defaultPrevented).toBe(false);
    expect(screen.queryByRole("combobox", { name: "Target Chat" })).toBeNull();
    expect(first).toHaveFocus();
    fireEvent.click(first);
    const reverseInput = screen.getByRole("combobox", { name: "Target Chat" });
    await waitFor(() => expect(reverseInput).toHaveFocus());
    const reverseTab = createEvent.keyDown(reverseInput, {
      key: "Tab",
      shiftKey: true,
    });
    fireEvent(reverseInput, reverseTab);
    expect(reverseTab.defaultPrevented).toBe(true);
    expect(screen.queryByRole("combobox", { name: "Target Chat" })).toBeNull();
    expect(last).toHaveFocus();
    fireEvent.click(first);
    const escapeInput = screen.getByRole("combobox", { name: "Target Chat" });
    await waitFor(() => expect(escapeInput).toHaveFocus());
    fireEvent.keyDown(escapeInput, { key: "Escape" });
    expect(screen.queryByRole("combobox", { name: "Target Chat" })).toBeNull();
    expect(dialog).toBeVisible();
    expect(first).toHaveFocus();
    first.focus();
    fireEvent.keyDown(first, { key: "Tab", shiftKey: true });
    expect(last).toHaveFocus();
    fireEvent.keyDown(last, { key: "Tab" });
    expect(first).toHaveFocus();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => expect(trigger).toHaveFocus());
    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole("button", { name: "Target Chat" }));
    await waitFor(() =>
      expect(
        screen.getByRole("combobox", { name: "Target Chat" }),
      ).toHaveFocus(),
    );
    act(() => useWorkbenchStore.setState({ projectGeneration: 5 }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(screen.queryByRole("combobox", { name: "Target Chat" })).toBeNull();
    await waitFor(() => expect(trigger).toHaveFocus());
  });

  it("shows construction failures and restores source focus after failed confirmation", async () => {
    const handoffChat = vi.fn(async () => {
      useWorkbenchStore.setState({ projectTransitioning: true });
      await Promise.resolve();
      useWorkbenchStore.setState({
        projectTransitioning: false,
        error: "Chat Handoff navigation failed",
      });
      return false;
    });
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      projectGeneration: 4,
      nativeTransitionEpoch: 7,
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat: Architecture",
          position: 0,
          pinned: true,
          dirty: false,
          status: "idle",
        },
        {
          id: "chat-b",
          kind: "chat",
          title: "Chat: Testing",
          position: 1,
          pinned: false,
          dirty: false,
          status: "idle",
        },
      ],
      activeTabId: "chat-a",
      harnesses: [
        {
          id: "sage-simulator",
          name: "Simulator",
          description: "Development",
          installState: "available",
          source: "development",
          accountModelDiscovery: false,
          transport: "legacySimulator",
          bindingId: null,
        },
      ],
      selectedHarnessId: "sage-simulator",
      messages: [
        {
          id: "message-a",
          sessionId: "session-a",
          sequence: 1,
          role: "user",
          text: "x".repeat(2049),
          createdAt: 1,
        },
      ],
      handoffChat,
    });
    render(<ChatSurface />);

    const trigger = screen.getByRole("button", { name: "Hand off chat" });
    fireEvent.click(trigger);
    const constructionAlert = screen.getByRole("alert");
    expect(constructionAlert).toHaveTextContent(
      "Chat Handoff message bytes exceed 2048",
    );
    expect(constructionAlert).not.toHaveClass("sr-only");
    expect(screen.queryByRole("dialog")).toBeNull();

    act(() =>
      useWorkbenchStore.setState({
        messages: [],
        error: null,
      }),
    );
    fireEvent.click(trigger);
    fireEvent.click(
      await screen.findByRole("button", { name: "Place in target" }),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(trigger).toHaveFocus());
    expect(
      screen.getByRole("textbox", { name: "Message agent" }),
    ).not.toHaveFocus();
  });

  it("does not leak a Source Control mutation error into the composer", () => {
    useWorkbenchStore.setState({
      repositoryMutationError: {
        resource: { rootId: "root-a", relativePath: "src/main.ts" },
        action: "discard",
        kind: "failed",
        message: "Git index is locked",
      },
      error: null,
    });

    render(<ChatSurface />);

    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText("Git index is locked")).toBeNull();
  });

  it("offers queueing during a running Turn alongside optional Cancel", () => {
    useWorkbenchStore.setState({ composerText: "Follow up" });
    const { rerender } = render(<ChatSurface />);
    expect(document.querySelector(".message-list")).not.toHaveAttribute(
      "aria-live",
    );
    const composer = screen.getByRole("textbox", { name: "Message agent" });

    expect(composer).toBeEnabled();
    expect(composer).toHaveAccessibleDescription(
      /Enter queues a message while this turn runs\. Shift\+Enter adds a new line\./,
    );
    expect(screen.getByRole("button", { name: "Queue message" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "Cancel turn" })).toBeNull();

    hooks.actions.cancellableTurnId = "turn-a";
    rerender(<ChatSurface />);
    expect(screen.queryByRole("button", { name: "Queue message" })).toBeNull();
    const cancel = screen.getByRole("button", { name: "Cancel turn" });
    expect(cancel).toBeEnabled();
    expect(composer).toHaveAccessibleDescription(
      /Enter queues a message while this turn runs\. Shift\+Enter adds a new line\./,
    );
    fireEvent.click(cancel);
    expect(hooks.actions.cancel).toHaveBeenCalledWith("turn-a");

    hooks.actions.cancellableTurnId = null;
    setTurnState("cancelling");
    rerender(<ChatSurface />);
    expect(composer).toBeEnabled();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    expect(composer).not.toHaveFocus();
  });

  it("restores focus only after the authoritative Turn becomes terminal", () => {
    useWorkbenchStore.setState({ composerText: "Follow up" });
    const { rerender } = render(<ChatSurface />);
    const composer = screen.getByRole("textbox", { name: "Message agent" });

    hooks.actions.cancellableTurnId = "turn-a";
    rerender(<ChatSurface />);
    hooks.actions.busy = true;
    hooks.actions.cancellableTurnId = null;
    rerender(<ChatSurface />);
    expect(composer).not.toHaveFocus();

    hooks.actions.busy = false;
    setTurnState("cancelling");
    rerender(<ChatSurface />);
    expect(composer).not.toHaveFocus();

    setTurnState("completed");
    rerender(<ChatSurface />);
    expect(composer).toBeEnabled();
    expect(composer).toHaveFocus();
    expect(screen.getByRole("button", { name: "Send message" })).toBeEnabled();
  });

  it("does not steal focus when a background Chat becomes terminal", () => {
    pinAgentChatOwner("chat-foreground");
    const { rerender } = render(<ChatSurface chatId="chat-background" />);
    const foregroundControl = document.createElement("button");
    document.body.append(foregroundControl);
    foregroundControl.focus();

    setTurnState("completed");
    rerender(<ChatSurface chatId="chat-background" />);

    expect(foregroundControl).toHaveFocus();
    expect(
      screen.getByRole("textbox", { name: "Message agent" }),
    ).not.toHaveFocus();
  });

  it.each(["diagram", "button", "pane"] as const)(
    "respects deliberate %s focus when activating a background Chat after completion",
    (target) => {
      pinAgentChatOwner("chat-foreground");
      const renderPane = () => (
        <section
          aria-label="Background pane"
          tabIndex={0}
          onFocus={() => pinAgentChatOwner("chat-background")}
        >
          <ChatSurface chatId="chat-background" />
        </section>
      );
      const { rerender } = render(renderPane());
      // Focusable transcript content (a scrollable diagram and its action).
      const figure = document.createElement("figure");
      figure.setAttribute("aria-label", "Overflowing diagram");
      figure.tabIndex = 0;
      const action = document.createElement("button");
      action.type = "button";
      action.textContent = "Diagram action";
      document.querySelector(".chat-surface")!.append(figure, action);
      setTurnState("completed");
      rerender(renderPane());
      const composer = screen.getByRole("textbox", { name: "Message agent" });
      expect(composer).not.toHaveFocus();
      const deliberateTarget =
        target === "diagram"
          ? screen.getByRole("figure", { name: "Overflowing diagram" })
          : target === "button"
            ? screen.getByRole("button", { name: "Diagram action" })
            : screen.getByRole("region", { name: "Background pane" });
      act(() => deliberateTarget.focus());
      expect(target === "pane" ? composer : deliberateTarget).toHaveFocus();
    },
  );

  it("exposes the authoritative session close as a semantic control", () => {
    setTurnState("completed");
    hooks.page = {
      ...hooks.page!,
      currentSessionId: "session-a" as never,
      sessions: [
        {
          id: "session-a",
          ordinal: 0,
          runtimeEpoch: "1",
          nextRuntimeEpoch: null,
          state: "active",
          residency: "hot",
          legacy: false,
          bindingId: "codex-app-server",
          modelId: null,
          capabilities: { closeSession: true },
          revision: "1",
          projectionRevision: "2",
        },
      ],
    } as unknown as AcpChatPage;

    render(<ChatSurface />);
    expect(screen.getByText("Live")).toBeInTheDocument();
    const close = screen.getByRole("button", { name: "Close session" });
    expect(close).toBeEnabled();
    fireEvent.click(close);
    expect(hooks.actions.close).toHaveBeenCalledOnce();
  });

  it("keeps the current closable session available when selected model binding does not match", () => {
    setTurnState("completed");
    hooks.page = {
      ...hooks.page!,
      currentSessionId: "session-a" as never,
      sessions: [
        {
          id: "session-a",
          ordinal: 0,
          runtimeEpoch: "1",
          nextRuntimeEpoch: null,
          state: "active",
          residency: "hot",
          legacy: false,
          bindingId: "codex-app-server",
          modelId: "a-different-model",
          capabilities: { closeSession: true },
          revision: "1",
          projectionRevision: "2",
        },
      ],
    } as unknown as AcpChatPage;

    render(<ChatSurface />);
    fireEvent.click(screen.getByRole("button", { name: "Close session" }));
    expect(hooks.actions.close).toHaveBeenCalledOnce();
  });

  it("keeps ACP prompting disabled until authoritative Chat state loads", () => {
    hooks.page = null;
    render(<ChatSurface />);

    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    expect(screen.getByLabelText("Message agent")).toBeDisabled();
  });

  it("does not misreport a failed admitted session as a missing local binding", () => {
    hooks.page = {
      ...hooks.page!,
      currentSessionId: "session-a" as never,
      sessions: [
        {
          id: "session-a",
          ordinal: 0,
          runtimeEpoch: "1",
          nextRuntimeEpoch: null,
          state: "failed",
          residency: "cold",
          legacy: false,
          bindingId: "codex-app-server",
          modelId: null,
          capabilities: { prompt: false, closeSession: false },
          revision: "2",
          projectionRevision: "3",
        },
      ],
    } as unknown as AcpChatPage;
    hooks.actions.error = "Codex launch failed safely; retry the session.";

    render(<ChatSurface />);

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Codex launch failed safely; retry the session.",
    );
    expect(
      screen.queryByText(
        "No reviewed local binding is available for this harness.",
      ),
    ).toBeNull();
  });

  it("keeps model selection and prompting disabled while Codex discovery checks", async () => {
    const selectHarness = vi.fn();
    const selectModel = vi.fn();
    hooks.page = { ...hooks.page!, turns: [] } as AcpChatPage;
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "old-harness",
          name: "Unavailable harness",
          description: "Previous binding",
          installState: "unavailable",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "old-acp",
        },
        {
          id: "codex",
          name: "Codex",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: true,
          transport: "acp",
          bindingId: "codex-app-server",
          accessModes: nativeReadOnly,
        },
      ],
      selectedHarnessId: "old-harness",
      harnessDiscoveryPhase: "checking",
      models: [],
      selectedModelId: null,
      selectHarness,
      selectModel,
    });

    render(<ChatSurface />);

    fireEvent.click(screen.getByRole("button", { name: "Harness" }));
    expect(
      screen.getByRole("option", {
        name: "Unavailable harness — installed but not drivable. Sage did not admit this harness and will not start it.",
      }),
    ).toBeDisabled();
    fireEvent.click(screen.getByRole("option", { name: "Codex — available" }));
    expect(selectHarness).toHaveBeenCalledExactlyOnceWith("codex");
    act(() =>
      useWorkbenchStore.setState({
        selectedHarnessId: "codex",
        selectedModelId: null,
      }),
    );
    expect(
      screen.getByText(/Checking the Codex account and available models/),
    ).toBeVisible();
    openComposerModel();
    expect(screen.getByLabelText("Model")).toBeDisabled();
    expect(
      screen.queryByText(
        "Wait for the active Turn to finish, or cancel it when available.",
      ),
    ).toBeNull();
    expect(
      screen.queryByText("Select a ready harness to load models."),
    ).toBeNull();
    expect(screen.getByLabelText("Message agent")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    const freshModels = Array.from({ length: 128 }, (_, index) => ({
      id: `fresh-model-${index + 1}`,
      name: `Fresh advertised model ${index + 1}`,
    }));
    act(() => {
      useWorkbenchStore.setState({
        harnessDiscoveryPhase: "ready",
        models: freshModels,
        selectedModelId: "fresh-model-1",
      });
    });
    const model = screen.getByRole("button", { name: "Model" });
    await waitFor(() => expect(model).toBeEnabled());
    fireEvent.click(model);
    const list = screen.getByRole("listbox", { name: "Select model" });
    expect(screen.queryByRole("option", { name: /old model/i })).toBeNull();
    expect(within(list).getAllByRole("option")).toHaveLength(128);
    expect(
      within(list).getByRole("option", {
        name: "Fresh advertised model 1",
      }),
    ).toHaveAttribute("aria-selected", "true");
    const first = within(list).getByRole("option", {
      name: "Fresh advertised model 1",
    });
    expect(first).toHaveFocus();
    fireEvent.keyDown(first, { key: "ArrowDown" });
    const second = within(list).getByRole("option", {
      name: "Fresh advertised model 2",
    });
    expect(second).toHaveFocus();
    fireEvent.click(second);
    expect(selectModel).toHaveBeenCalledExactlyOnceWith("fresh-model-2");
  });

  it("annotates an uncharacterised harness without disabling it", () => {
    const selectHarness = vi.fn();
    hooks.page = { ...hooks.page!, turns: [] } as AcpChatPage;
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "claude-code",
          name: "Claude Code",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "claude-agent-acp",
          accessModes: [
            {
              id: "ask",
              label: "Ask each time",
              description: "Review permission requests.",
              fallback: true,
            },
          ],
          version: "2.1.270",
          boundVersion: "0.99.0",
          characterisation: "uncharacterised",
        },
      ],
      selectedHarnessId: "claude-code",
      harnessDiscoveryPhase: "idle",
      models: [],
      selectedModelId: null,
      selectHarness,
    });

    render(<ChatSurface />);

    fireEvent.click(screen.getByRole("button", { name: "Harness" }));
    const option = screen.getByRole("option", {
      name: "Claude Code — available — uncharacterised",
    });
    expect(option).toBeEnabled();
    fireEvent.click(option);
    expect(selectHarness).toHaveBeenCalledExactlyOnceWith("claude-code");
  });

  it("keeps a present harness selectable while its version is checking", () => {
    const selectHarness = vi.fn();
    hooks.page = { ...hooks.page!, turns: [] } as AcpChatPage;
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "claude-code",
          name: "Claude Code",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "claude-agent-acp",
          accessModes: [
            {
              id: "ask",
              label: "Ask each time",
              description: "Review permission requests.",
              fallback: true,
            },
          ],
          versionChecking: true,
        },
      ],
      selectedHarnessId: "claude-code",
      harnessDiscoveryPhase: "idle",
      models: [],
      selectedModelId: null,
      selectHarness,
    });

    render(<ChatSurface />);

    fireEvent.click(screen.getByRole("button", { name: "Harness" }));
    const option = screen.getByRole("option", {
      name: "Claude Code — version checking…",
    });
    expect(option).toBeEnabled();
    fireEvent.click(option);
    expect(selectHarness).toHaveBeenCalledExactlyOnceWith("claude-code");
  });

  it("requires consent before starting bounded browser sign-in", () => {
    const start = vi
      .spyOn(acpDesktop, "startCodexAuth")
      .mockReturnValue(new Promise(() => {}));
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "Project A", roots: [] },
      harnessDiscoveryPhase: "signInRequired",
      models: [],
      selectedModelId: null,
    });

    render(<ChatSurface />);

    expect(
      screen.getByText(/sign in to the account required by codex/i),
    ).toBeVisible();
    expect(screen.getByLabelText("Message agent")).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    expect(
      screen.getByText(
        /Sage will open this harness's official sign-in page in your browser\./,
      ),
    ).toHaveTextContent(
      "Sage will open this harness's official sign-in page in your browser. Codex stores credentials; Sage does not receive or retain them.",
    );
    expect(screen.getByRole("button", { name: "Continue" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "Continue" }));

    expect(start).toHaveBeenCalledOnce();
    expect(
      screen.getByText(/finish signing in through your browser/i),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeVisible();
  });

  it("does not let a stale ACP page mask simulator messages", () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "sage-simulator",
          name: "Simulator",
          description: "Legacy simulator",
          installState: "available",
          source: "development",
          accountModelDiscovery: false,
          transport: "legacySimulator",
          bindingId: null,
        },
      ],
      selectedHarnessId: "sage-simulator",
      messages: [
        {
          id: "simulator-message",
          sessionId: "simulator-session",
          sequence: 2,
          role: "assistant",
          text: "Visible simulator result",
          createdAt: 2,
        },
      ],
    });

    render(<ChatSurface />);
    expect(screen.getByText("Visible simulator result")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Open file src/main.ts" }),
    ).toBeNull();
  });

  it("routes duplicate-path ACP file and diff actions by canonical ResourceRef", async () => {
    const openFile = vi.fn().mockResolvedValue(undefined);
    const openDiff = vi.fn().mockResolvedValue(undefined);
    useWorkbenchStore.setState({
      project: {
        id: "project-a",
        name: "Project A",
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
      projectGeneration: 0,
      projectTransitioning: false,
      openFile,
      openDiff,
    });
    acpProjectionStoreFor("chat-a").setState({
      scope: {
        projectId: "project-a",
        projectGeneration: "0",
        chatId: "chat-a",
      } as never,
      page: hooks.page,
    });

    const { rerender } = render(<ChatSurface />);
    await expandReadActivity();
    fireEvent.click(
      await screen.findByRole("button", { name: "Open file src/main.ts" }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: /file changed/ }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Open diff B / src/main.ts" }),
    );

    expect(openFile).toHaveBeenCalledWith({
      rootId: "root-a",
      relativePath: "src/main.ts",
    });
    await waitFor(() =>
      expect(openDiff).toHaveBeenCalledWith(
        { rootId: "root-b", relativePath: "src/main.ts" },
        undefined,
        expect.objectContaining({
          projectId: "project-a",
          chatId: "chat-a",
          sessionId: "session-a",
          turnId: "turn-a",
          itemId: "item-diff",
          runtimeEpoch: "1",
          snapshotRevision: "1",
          rootId: "root-b",
          relativePath: "src/main.ts",
          contentDigest: "a".repeat(64),
        }),
      ),
    );

    const refreshed = structuredClone(hooks.page!);
    refreshed.currentSessionId = "session-next" as never;
    refreshed.sessions = [{ id: "session-next", ordinal: 0 }] as never;
    refreshed.turns = refreshed.turns.map(
      (turn) =>
        ({
          ...turn,
          sessionId: "session-next" as never,
          revision: "2" as never,
          items: turn.items.map(
            (item) =>
              ({
                ...item,
                sessionId: "session-next" as never,
                revision: "2" as never,
              }) as AcpItem,
          ),
        }) as typeof turn,
    );
    hooks.page = refreshed;
    acpProjectionStoreFor("chat-a").setState({
      scope: {
        projectId: "project-a",
        projectGeneration: "0",
        chatId: "chat-a",
      } as never,
      page: refreshed,
    });
    rerender(<ChatSurface />);
    await expandReadActivity();
    fireEvent.click(
      await screen.findByRole("button", { name: "Open file src/main.ts" }),
    );

    expect(openFile).toHaveBeenLastCalledWith({
      rootId: "root-a",
      relativePath: "src/main.ts",
    });
  });

  it("rejects an old rendered ToolItem handler after owner rollover", async () => {
    const openAgentArtifact = vi.fn(async () => true);
    vi.resetModules();
    vi.doMock("../../app/store", () => ({ useWorkbenchStore }));
    vi.doMock("../../app/openAgentArtifact", () => ({ openAgentArtifact }));
    vi.doMock("./acp", async () => {
      const actual = await vi.importActual<typeof AcpModule>("./acp");
      return {
        ...actual,
        useAcpChatActions: () => hooks.actions,
        useAcpChatProjection: () => hooks.page,
      };
    });
    const { ChatSurface: CapturingChatSurface } = await import("./ChatSurface");
    const { rerender } = render(<CapturingChatSurface />);
    await expandReadActivity();
    const oldButton = await screen.findByRole("button", {
      name: "Open file src/main.ts",
    });
    const oldPropsKey = Object.keys(oldButton).find((key) =>
      key.startsWith("__reactProps$"),
    ) as keyof typeof oldButton;
    const oldHandler = (
      oldButton[oldPropsKey] as unknown as { onClick: () => void }
    ).onClick;
    const refreshed = structuredClone(hooks.page!);
    refreshed.currentSessionId = "session-next" as never;
    refreshed.sessions = [{ id: "session-next", ordinal: 0 }] as never;
    refreshed.turns = refreshed.turns.map(
      (turn) =>
        ({
          ...turn,
          sessionId: "session-next" as never,
          revision: "2" as never,
          items: turn.items.map(
            (item) =>
              ({
                ...item,
                projectId: "project-next" as never,
                chatId: "chat-next" as never,
                sessionId: "session-next" as never,
                revision: "2" as never,
              }) as AcpItem,
          ),
        }) as never,
    );
    hooks.page = {
      ...refreshed,
      projectId: "project-next" as never,
      chatId: "chat-next" as never,
      currentSessionId: "session-next" as never,
    };
    rerender(<CapturingChatSurface />);
    oldHandler();
    await Promise.resolve();
    expect(openAgentArtifact).not.toHaveBeenCalled();
    await expandReadActivity();
    const freshButton = await screen.findByRole("button", {
      name: "Open file src/main.ts",
    });
    const freshPropsKey = Object.keys(freshButton).find((key) =>
      key.startsWith("__reactProps$"),
    ) as keyof typeof freshButton;
    (
      freshButton[freshPropsKey] as unknown as { onClick: () => void }
    ).onClick();
    await Promise.resolve();
    expect(openAgentArtifact).toHaveBeenCalledWith(
      expect.objectContaining({
        owner: expect.objectContaining({
          projectId: "project-next",
          chatId: "chat-next",
          sessionId: "session-next",
        }),
      }),
    );
    vi.doUnmock("../../app/store");
    vi.doUnmock("../../app/openAgentArtifact");
    vi.doUnmock("./acp");
    vi.resetModules();
  });

  it("passes frontend retention incompleteness into the ACP edit summary", async () => {
    useAcpProjectionStore.setState({ retentionIncomplete: true });

    render(<ChatSurface />);

    const editPill = await screen.findByRole("button", {
      name: /Showing 1 changed file/,
    });
    fireEvent.click(editPill);
    expect(
      screen.getByText(/discarded to stay within memory limits/i),
    ).toBeVisible();
  });

  it("does not mix the global legacy edit summary into an ACP Chat", () => {
    const resource: ResourceRef = {
      rootId: "root-c",
      relativePath: "legacy/change.ts",
    };
    const openDiff = vi.fn().mockResolvedValue(undefined);
    useWorkbenchStore.setState({
      projectTransitioning: false,
      openDiff,
      turnChanges: [
        {
          resource,
          indexStatus: "unchanged",
          worktreeStatus: "modified",
          additions: 2,
          deletions: 1,
        },
      ],
    });

    render(<ChatSurface />);
    expect(
      screen.queryByRole("button", {
        name: "Open diff for legacy/change.ts",
      }),
    ).toBeNull();
    expect(openDiff).not.toHaveBeenCalled();
  });

  it("keeps the bounded global edit summary for the development simulator", () => {
    const resource: ResourceRef = {
      rootId: "root-c",
      relativePath: "legacy/change.ts",
    };
    const openDiff = vi.fn().mockResolvedValue(undefined);
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "sage-simulator",
          name: "Simulator",
          description: "Legacy simulator",
          installState: "available",
          source: "development",
          accountModelDiscovery: false,
          transport: "legacySimulator",
          bindingId: null,
        },
      ],
      selectedHarnessId: "sage-simulator",
      openDiff,
      turnChanges: [
        {
          resource,
          indexStatus: "unchanged",
          worktreeStatus: "modified",
          additions: 2,
          deletions: 1,
        },
      ],
    });

    render(<ChatSurface />);
    fireEvent.click(
      screen.getByRole("button", {
        name: "Open diff for legacy/change.ts",
      }),
    );
    expect(openDiff).toHaveBeenCalledWith(resource);
  });

  it("hides ACP and legacy resource actions during project transition", () => {
    useWorkbenchStore.setState({
      projectTransitioning: true,
      turnChanges: [
        {
          resource: { rootId: "root-c", relativePath: "legacy/change.ts" },
          indexStatus: "unchanged",
          worktreeStatus: "modified",
          additions: 2,
          deletions: 1,
        },
      ],
    });

    render(<ChatSurface />);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Loading the authoritative project",
    );
    expect(
      screen.queryByRole("button", { name: /open (?:file|diff)/i }),
    ).toBeNull();
  });
});

describe("ChatSurface per-chat composer", () => {
  it("restores a failed Codex chat from durable history instead of the simulator", async () => {
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      projectGeneration: 1,
      nativeTransitionEpoch: 1,
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
      chatUiById: {},
      harnesses: [
        {
          id: "sage-simulator",
          name: "Simulator",
          description: "Development",
          installState: "available",
          source: "development",
          accountModelDiscovery: false,
          transport: "legacySimulator",
          bindingId: null,
        },
        {
          id: "codex",
          name: "Codex",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: true,
          transport: "acp",
          bindingId: "codex-app-server",
          accessModes: nativeReadOnly,
        },
      ],
      selectedHarnessId: "sage-simulator",
      selectedModelId: null,
      harnessDiscoveryPhase: "ready",
    });
    const failedPage = {
      ...projection(),
      currentSessionId: null,
      sessions: [
        {
          id: "session-a",
          ordinal: 0,
          bindingId: "codex-app-server",
          modelId: "gpt-6-astra",
          state: "failed",
          residency: "cold",
          legacy: false,
          runtimeEpoch: null,
        },
      ] as never,
      turns: [
        {
          ...projection().turns[0],
          state: "failed",
          items: [
            {
              ...itemBase("user-prompt"),
              kind: "text",
              payload: { role: "user", text: "Design a small task manager" },
            } as AcpItem,
            {
              ...itemBase("partial-answer"),
              ordinal: 1,
              kind: "text",
              lifecycle: "cancelled",
              payload: { role: "assistant", text: "Start with durable tasks" },
            } as AcpItem,
          ],
        },
      ],
    } as AcpChatPage;

    hooks.page = null;
    const view = render(<ChatSurface chatId="chat-a" />);
    expect(
      useWorkbenchStore.getState().chatUiById["chat-a"]?.harnessId,
    ).toBeUndefined();
    hooks.page = failedPage;
    view.rerender(<ChatSurface chatId="chat-a" />);
    await settleLazyModules();

    expect(useWorkbenchStore.getState().chatUiById["chat-a"]?.harnessId).toBe(
      "codex",
    );
    expect(screen.getByText("Design a small task manager")).toBeVisible();
    expect(screen.getByText("Start with durable tasks")).toBeVisible();
    expect(screen.queryByText("Development simulator")).toBeNull();
  });

  it("keeps harness selection and prompt routing with the owning mounted Chat", async () => {
    vi.spyOn(desktop, "listHarnessModels").mockImplementation(
      async (harnessId) =>
        harnessId === "claude"
          ? [{ id: "claude-model", name: "Claude model", isDefault: true }]
          : [{ id: "codex-model", name: "Codex model", isDefault: true }],
    );
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      projectGeneration: 1,
      nativeTransitionEpoch: 1,
      chatOwnershipEpoch: 0,
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "A",
          position: 0,
          pinned: false,
          dirty: false,
        },
        {
          id: "chat-b",
          kind: "chat",
          title: "B",
          position: 1,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
      chatUiById: {
        "chat-a": {
          composerText: "",
          attachedContext: [],
          harnessId: null,
          modelId: null,
          sending: false,
          error: null,
          errorKind: null,
          requestGeneration: 0,
        },
        "chat-b": {
          composerText: "",
          attachedContext: [],
          harnessId: null,
          modelId: null,
          sending: false,
          error: null,
          errorKind: null,
          requestGeneration: 0,
        },
      },
      composerText: "",
      harnesses: [
        {
          id: "codex",
          name: "Codex",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "codex-app-server",
          accessModes: nativeReadOnly,
        },
        {
          id: "claude",
          name: "Claude",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "claude-agent-acp",
          accessModes: [
            {
              id: "ask",
              label: "Ask each time",
              description: "Review permission requests.",
              fallback: true,
            },
          ],
        },
      ],
      selectedHarnessId: "codex",
      selectedModelId: "codex-model",
      models: [{ id: "codex-model", name: "Codex model", isDefault: true }],
      harnessDiscoveryPhase: "ready",
    });
    hooks.page = { ...projection(), turns: [] };
    render(
      <>
        <ChatSurface chatId="chat-a" />
        <ChatSurface chatId="chat-b" />
      </>,
    );
    const [left, right] = screen.getAllByRole("region", {
      name: "Agent chat",
    });
    if (!left || !right) throw new Error("Missing mounted Chats");

    fireEvent.click(within(right).getByRole("button", { name: "Harness" }));
    fireEvent.click(screen.getByRole("option", { name: /Claude/ }));
    await waitFor(() =>
      expect(
        within(right).getByRole("button", { name: "Harness" }),
      ).toHaveTextContent("Claude"),
    );
    expect(
      within(left).getByRole("button", { name: "Harness" }),
    ).toHaveTextContent("Codex");

    fireEvent.change(
      within(left).getByRole("textbox", { name: "Message agent" }),
      {
        target: { value: "left only" },
      },
    );
    fireEvent.click(within(left).getByRole("button", { name: "Send message" }));
    expect(hooks.actions.submit).toHaveBeenCalledWith(
      "left only",
      [],
      "codex-app-server",
      "codex-model",
    );
  });

  it("keeps an existing account-backed session selected while its models load", () => {
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      projectGeneration: 1,
      nativeTransitionEpoch: 1,
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "A",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
      chatUiById: {
        "chat-a": {
          composerText: "",
          attachedContext: [],
          harnessId: null,
          modelId: null,
          sending: false,
          error: null,
          errorKind: null,
          requestGeneration: 0,
        },
      },
      harnesses: [
        {
          id: "codex",
          name: "Codex",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: true,
          transport: "acp",
          bindingId: "codex-app-server",
          accessModes: nativeReadOnly,
        },
      ],
      selectedHarnessId: "codex",
      selectedModelId: null,
      models: [],
      harnessDiscoveryPhase: "checking",
    });
    hooks.page = {
      ...projection(),
      turns: [],
      sessions: [
        {
          id: "session-a",
          ordinal: 0,
          runtimeEpoch: "1",
          nextRuntimeEpoch: null,
          state: "active",
          residency: "hot",
          legacy: false,
          bindingId: "codex-app-server",
          modelId: "gpt-x",
          capabilities: { prompt: true, closeSession: true },
          revision: "1",
          projectionRevision: "1",
        },
      ],
    } as unknown as AcpChatPage;
    render(<ChatSurface chatId="chat-a" />);

    expect(screen.getByLabelText("Harness status: Live")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Message agent"), {
      target: { value: "continue the existing session" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(hooks.actions.submit).toHaveBeenCalledWith(
      "continue the existing session",
      [],
      "codex-app-server",
      "gpt-x",
    );
  });

  it("adopts the chosen harness model after sign-in completes", async () => {
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      projectGeneration: 1,
      nativeTransitionEpoch: 1,
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "A",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
      chatUiById: {
        "chat-a": {
          composerText: "",
          attachedContext: [],
          harnessId: "claude",
          modelId: null,
          sending: false,
          error: null,
          errorKind: null,
          requestGeneration: 0,
        },
      },
      harnesses: [
        {
          id: "claude",
          name: "Claude",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: true,
          transport: "acp",
          bindingId: "claude-agent-acp",
          accessModes: [
            {
              id: "ask",
              label: "Ask each time",
              description: "Review permission requests.",
              fallback: true,
            },
          ],
        },
      ],
      selectedHarnessId: "claude",
      selectedModelId: null,
      models: [],
      harnessDiscoveryPhase: "signInRequired",
    });
    hooks.page = {
      ...projection(),
      currentSessionId: null,
      sessions: [],
      turns: [],
    } as AcpChatPage;
    render(<ChatSurface chatId="chat-a" />);

    act(() =>
      useWorkbenchStore.setState({
        selectedModelId: "claude-model",
        models: [{ id: "claude-model", name: "Claude model" }],
        harnessDiscoveryPhase: "ready",
      }),
    );

    await waitFor(() =>
      expect(useWorkbenchStore.getState().chatUiById["chat-a"]?.modelId).toBe(
        "claude-model",
      ),
    );
    fireEvent.change(screen.getByLabelText("Message agent"), {
      target: { value: "start after sign-in" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(hooks.actions.submit).toHaveBeenCalledWith(
      "start after sign-in",
      [],
      "claude-agent-acp",
      "claude-model",
    );
  });

  it("does not borrow a global model when the owning Chat has none", () => {
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      projectGeneration: 1,
      nativeTransitionEpoch: 1,
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "A",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
      chatUiById: {
        "chat-a": {
          composerText: "",
          attachedContext: [],
          harnessId: "codex",
          modelId: null,
          sending: false,
          error: null,
          errorKind: null,
          requestGeneration: 0,
        },
      },
      harnesses: [
        {
          id: "codex",
          name: "Codex",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "codex-app-server",
          accessModes: nativeReadOnly,
        },
        {
          id: "claude",
          name: "Claude",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "claude-agent-acp",
          accessModes: [
            {
              id: "ask",
              label: "Ask each time",
              description: "Review permission requests.",
              fallback: true,
            },
          ],
        },
      ],
      selectedHarnessId: "claude",
      selectedModelId: "claude-model",
      models: [{ id: "claude-model", name: "Claude model" }],
      harnessDiscoveryPhase: "ready",
    });
    hooks.page = { ...projection(), turns: [] };
    render(<ChatSurface chatId="chat-a" />);

    fireEvent.change(screen.getByLabelText("Message agent"), {
      target: { value: "use the harness default" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    expect(hooks.actions.submit).toHaveBeenCalledWith(
      "use the harness default",
      [],
      "codex-app-server",
      null,
    );
  });

  it("keeps an unrelated Chat ready while another harness needs sign-in", () => {
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      projectGeneration: 1,
      nativeTransitionEpoch: 1,
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "A",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
      chatUiById: {
        "chat-a": {
          composerText: "",
          attachedContext: [],
          harnessId: "codex",
          modelId: "codex-model",
          sending: false,
          error: null,
          errorKind: null,
          requestGeneration: 0,
        },
      },
      harnesses: [
        {
          id: "codex",
          name: "Codex",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          bindingId: "codex-app-server",
          accessModes: nativeReadOnly,
        },
        {
          id: "claude",
          name: "Claude",
          description: "ACP",
          installState: "available",
          source: "builtIn",
          accountModelDiscovery: true,
          transport: "acp",
          bindingId: "claude-agent-acp",
          accessModes: [
            {
              id: "ask",
              label: "Ask each time",
              description: "Review permission requests.",
              fallback: true,
            },
          ],
        },
      ],
      selectedHarnessId: "claude",
      selectedModelId: null,
      models: [],
      harnessDiscoveryPhase: "signInRequired",
    });
    hooks.page = { ...projection(), turns: [] };
    render(<ChatSurface chatId="chat-a" />);

    expect(screen.getByLabelText("Harness status: Ready")).toBeInTheDocument();
    expect(screen.getByLabelText("Message agent")).toBeEnabled();
    expect(
      screen.queryByText(/sign in to the account required by claude/i),
    ).toBeNull();
  });

  it("keeps each mounted Chat composer bound to its chat id", () => {
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      projectGeneration: 1,
      nativeTransitionEpoch: 1,
      chatOwnershipEpoch: 0,
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "A",
          position: 0,
          pinned: false,
          dirty: false,
        },
        {
          id: "chat-b",
          kind: "chat",
          title: "B",
          position: 1,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
      chatUiById: {
        "chat-a": {
          composerText: "",
          attachedContext: [],
          sending: false,
          error: null,
          errorKind: null,
          requestGeneration: 0,
        },
        "chat-b": {
          composerText: "",
          attachedContext: [],
          sending: false,
          error: null,
          errorKind: null,
          requestGeneration: 0,
        },
      },
      composerText: "",
      harnesses: [
        {
          id: "sage-simulator",
          name: "Simulator",
          description: "Development",
          installState: "available",
          source: "development",
          accountModelDiscovery: false,
          transport: "legacySimulator",
          bindingId: null,
        },
      ],
      selectedHarnessId: "sage-simulator",
    });
    hooks.page = null;
    render(
      <>
        <ChatSurface chatId="chat-a" />
        <ChatSurface chatId="chat-b" />
      </>,
    );
    const composers = screen.getAllByPlaceholderText("Do anything");
    fireEvent.change(composers[0], { target: { value: "Draft A" } });
    fireEvent.change(composers[1], { target: { value: "Draft B" } });
    expect(composers[0]).toHaveValue("Draft A");
    expect(composers[1]).toHaveValue("Draft B");
    expect(useWorkbenchStore.getState().chatUiById["chat-a"].composerText).toBe(
      "Draft A",
    );
    expect(useWorkbenchStore.getState().chatUiById["chat-b"].composerText).toBe(
      "Draft B",
    );
  });
});

describe("ChatSurface A11Y-2 singleton chrome", () => {
  function announcementCountInChat(chatId: string) {
    const composer = document.getElementById(`prompt-${chatId}`);
    return (
      composer
        ?.closest(".chat-surface")
        ?.querySelectorAll('[data-acp-announcement]:not([aria-live="off"])')
        .length ?? 0
    );
  }

  it("keeps one announcement pair when two ACP panes are mounted", () => {
    pinAgentChatOwner("chat-a");
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "A",
          position: 0,
          pinned: false,
          dirty: false,
        },
        {
          id: "chat-b",
          kind: "chat",
          title: "B",
          position: 1,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });
    render(
      <>
        <ChatSurface chatId="chat-a" />
        <ChatSurface chatId="chat-b" />
      </>,
    );
    expect(
      document.querySelectorAll(
        '[data-acp-announcement]:not([aria-live="off"])',
      ),
    ).toHaveLength(2);
    expect(screen.getAllByText("Durable chat")).toHaveLength(2);
  });

  it("keeps the pinned Agent chat as a11y owner when a File tab is active", () => {
    pinAgentChatOwner("chat-a");
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "A",
          position: 0,
          pinned: false,
          dirty: false,
        },
        {
          id: "chat-b",
          kind: "chat",
          title: "B",
          position: 1,
          pinned: false,
          dirty: false,
        },
        {
          id: "file:main",
          kind: "file",
          title: "main.ts",
          position: 2,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "src/main.ts" },
        },
      ],
      activeTabId: "file:main",
    });
    render(
      <>
        <ChatSurface chatId="chat-a" />
        <ChatSurface chatId="chat-b" />
      </>,
    );
    expect(
      document.querySelectorAll(
        '[data-acp-announcement]:not([aria-live="off"])',
      ),
    ).toHaveLength(2);
    expect(screen.getAllByText("Durable chat")).toHaveLength(2);
    expect(announcementCountInChat("chat-a")).toBe(2);
    expect(announcementCountInChat("chat-b")).toBe(0);
  });
});

describe("ChatSurface embedded chrome", () => {
  it("leaves the session title to the pane header instead of repeating it as a tab", () => {
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat: Review",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });
    render(<ChatSurface chatId="chat-a" />);

    expect(screen.queryByRole("tab")).toBeNull();
    expect(screen.queryByText("Sage workbench")).toBeNull();
    expect(screen.queryByText("Persistent agent session")).toBeNull();
  });

  it("does not restore embedded chrome when a stored title is invalid", () => {
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "chat-bad",
          kind: "chat",
          title: "Chat: bad\ntitle",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-bad",
    });
    render(<ChatSurface chatId="chat-bad" />);

    expect(screen.queryByRole("tab")).toBeNull();
    expect(screen.queryByText("Sage workbench")).toBeNull();
  });
});

describe("ChatSurface composer focus hotkey", () => {
  it("focuses the enabled composer on Ctrl+L", () => {
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "chat-hotkey",
          kind: "chat",
          title: "Chat: Hotkey",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-hotkey",
      harnesses: [
        {
          id: "sage-simulator",
          name: "Simulator",
          description: "Development",
          installState: "available",
          source: "development",
          accountModelDiscovery: false,
          transport: "legacySimulator",
          bindingId: null,
        },
      ],
      selectedHarnessId: "sage-simulator",
    });
    render(<ChatSurface />);
    const composer = screen.getByRole("textbox", { name: "Message agent" });
    expect(composer).toBeEnabled();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    expect(
      screen.queryByRole("button", { name: "Composer options" }),
    ).toBeNull();
    expect(composer).toHaveAttribute("placeholder", "Do anything");
    composer.blur();
    expect(composer).not.toHaveFocus();
    fireEvent.keyDown(window, { key: "l", ctrlKey: true });
    expect(composer).toHaveFocus();
    fireEvent.change(composer, { target: { value: "Send this" } });
    expect(screen.getByRole("button", { name: "Send message" })).toBeEnabled();
  });

  it("does not steal Ctrl+L when the composer is disabled", () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "codex",
          name: "Codex",
          description: "ACP",
          installState: "notInstalled",
          source: "builtIn",
          accountModelDiscovery: true,
          transport: "acp",
          bindingId: "codex-app-server",
          accessModes: nativeReadOnly,
        },
      ],
      selectedHarnessId: "codex",
    });
    hooks.page = null;
    render(<ChatSurface />);
    const composer = screen.getByRole("textbox", { name: "Message agent" });
    expect(composer).toBeDisabled();
    expect(composer).toHaveAttribute(
      "placeholder",
      "Choose an available harness before starting a session",
    );
    fireEvent.keyDown(window, { key: "l", ctrlKey: true });
    expect(composer).not.toHaveFocus();
  });
});

describe("ChatSurface attached context", () => {
  it("renders a removable diff comment chip for the bound Chat", () => {
    const chatId = "chat-context";
    hooks.page = { ...projection(), chatId: chatId as never };
    useWorkbenchStore.setState({
      tabs: [
        {
          id: chatId,
          kind: "chat",
          title: "Context",
          position: 0,
          pinned: true,
          dirty: false,
        },
      ],
      activeTabId: chatId,
      project: { id: "project-a", name: "A", roots: [] },
      chatOwnershipEpoch: 1,
      chatUiById: {
        [chatId]: {
          composerText: "Fix this",
          attachedContext: [
            {
              kind: "diffLineComment",
              diffRef: {
                projectId: "project-a",
                chatId,
                sessionId: "session-a",
                runtimeEpoch: "1",
                turnId: "turn-a",
                itemId: "item-a",
                diffOrdinal: 0,
                snapshotRevision: "1",
                rootId: "root-a",
                relativePath: "src/AgentView.tsx",
                contentDigest: "a".repeat(64),
              } as never,
              side: "new",
              startLine: "27",
              endLine: "31",
              note: "Keep the selection explicit",
            },
          ],
          sending: false,
          error: null,
          errorKind: null,
          requestGeneration: 0,
        },
      },
      composerText: "Fix this",
    });

    render(<ChatSurface chatId={chatId} />);
    const attachmentRow = document.querySelector(".composer__attachment-row");
    expect(attachmentRow).not.toBeNull();
    expect(
      within(attachmentRow as HTMLElement).getByLabelText("Attached context"),
    ).toHaveTextContent("Comment · src/AgentView.tsx:27-31");
    fireEvent.click(
      screen.getByRole("button", {
        name: "Remove Comment · src/AgentView.tsx:27-31 context",
      }),
    );
    expect(
      useWorkbenchStore.getState().chatUiById[chatId].attachedContext,
    ).toEqual([]);
  });
});

describe("ChatSurface composer context", () => {
  it("keeps the composer hook order when speech support disappears", () => {
    class FakeRecognition {}
    vi.stubGlobal("webkitSpeechRecognition", FakeRecognition);
    const view = render(<ChatSurface chatId="chat-a" />);
    expect(
      screen.getByRole("button", { name: "Start dictation" }),
    ).toBeVisible();
    vi.stubGlobal("webkitSpeechRecognition", undefined);
    expect(() => view.rerender(<ChatSurface chatId="chat-a" />)).not.toThrow();
    expect(
      screen.getByRole("button", { name: "Voice input unavailable" }),
    ).toBeDisabled();
  });

  it("keeps the microphone affordance visible when speech recognition is unavailable", () => {
    vi.stubGlobal("SpeechRecognition", undefined);
    vi.stubGlobal("webkitSpeechRecognition", undefined);
    render(<ChatSurface chatId="chat-a" />);
    expect(
      screen.getByRole("button", { name: "Voice input unavailable" }),
    ).toBeDisabled();
  });
  it("cancels active dictation on a chat switch and ignores late speech", () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "sage-simulator",
          name: "Simulator",
          description: "Development",
          installState: "available",
          source: "development",
          accountModelDiscovery: false,
          transport: "legacySimulator",
          bindingId: null,
        },
      ],
      selectedHarnessId: "sage-simulator",
      composerText: "First draft",
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "A",
          position: 0,
          pinned: false,
          dirty: false,
        },
        {
          id: "chat-b",
          kind: "chat",
          title: "B",
          position: 1,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });
    class FakeRecognition {
      static current: FakeRecognition;
      onresult: ((event: unknown) => void) | null = null;
      onerror: ((event: unknown) => void) | null = null;
      onend: (() => void) | null = null;
      start = vi.fn();
      stop = vi.fn();
      abort = vi.fn();
      constructor() {
        FakeRecognition.current = this;
      }
    }
    vi.stubGlobal("webkitSpeechRecognition", FakeRecognition);
    render(<ChatSurface />);
    fireEvent.click(screen.getByRole("button", { name: "Start dictation" }));
    const listening = FakeRecognition.current;
    expect(listening.start).toHaveBeenCalledOnce();
    act(() =>
      useWorkbenchStore.setState({
        activeTabId: "chat-b",
        composerText: "Second draft",
      }),
    );
    expect(listening.abort).toHaveBeenCalledOnce();
    act(() =>
      listening.onresult?.({
        resultIndex: 0,
        results: [{ isFinal: true, 0: { transcript: " late speech" } }],
      }),
    );
    expect(useWorkbenchStore.getState().composerText).toBe("Second draft");
  });

  it("aborts active dictation on unmount and ignores late speech", () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "sage-simulator",
          name: "Simulator",
          description: "Development",
          installState: "available",
          source: "development",
          accountModelDiscovery: false,
          transport: "legacySimulator",
          bindingId: null,
        },
      ],
      selectedHarnessId: "sage-simulator",
      composerText: "Draft",
    });
    class FakeRecognition {
      static current: FakeRecognition;
      onresult: ((event: unknown) => void) | null = null;
      onerror: ((event: unknown) => void) | null = null;
      onend: (() => void) | null = null;
      start = vi.fn();
      stop = vi.fn();
      abort = vi.fn();
      constructor() {
        FakeRecognition.current = this;
      }
    }
    vi.stubGlobal("webkitSpeechRecognition", FakeRecognition);
    const view = render(<ChatSurface />);
    fireEvent.click(screen.getByRole("button", { name: "Start dictation" }));
    const listening = FakeRecognition.current;
    expect(listening.start).toHaveBeenCalledOnce();
    view.unmount();
    expect(listening.abort).toHaveBeenCalledOnce();
    act(() =>
      listening.onresult?.({
        resultIndex: 0,
        results: [{ isFinal: true, 0: { transcript: " late speech" } }],
      }),
    );
    expect(useWorkbenchStore.getState().composerText).toBe("Draft");
  });

  it("shows dictation only when supported and adds final speech to the draft", async () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "sage-simulator",
          name: "Simulator",
          description: "Development",
          installState: "available",
          source: "development",
          accountModelDiscovery: false,
          transport: "legacySimulator",
          bindingId: null,
        },
      ],
      selectedHarnessId: "sage-simulator",
      composerText: "Start",
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-a",
    });
    const view = render(<ChatSurface />);
    expect(
      screen.queryByRole("button", { name: "Start dictation" }),
    ).toBeNull();

    class FakeRecognition {
      static current: FakeRecognition;
      onresult: ((event: unknown) => void) | null = null;
      onerror: ((event: unknown) => void) | null = null;
      onend: (() => void) | null = null;
      start = vi.fn();
      stop = vi.fn();
      abort = vi.fn();
      constructor() {
        FakeRecognition.current = this;
      }
    }
    vi.stubGlobal("webkitSpeechRecognition", FakeRecognition);
    view.rerender(<ChatSurface />);
    const mic = screen.getByRole("button", { name: "Start dictation" });
    const mouseDown = createEvent.mouseDown(mic);
    fireEvent(mic, mouseDown);
    expect(mouseDown.defaultPrevented).toBe(true);
    expect(mic).toHaveAttribute(
      "title",
      expect.stringMatching(/online service/i),
    );
    fireEvent.click(mic);
    expect(FakeRecognition.current.start).toHaveBeenCalledOnce();
    act(() =>
      FakeRecognition.current.onresult?.({
        resultIndex: 0,
        results: [{ isFinal: true, 0: { transcript: " hello world" } }],
      }),
    );
    expect(useWorkbenchStore.getState().composerText).toBe("Start hello world");
    expect(
      screen.getByRole("button", { name: "Stop dictation" }),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Stop dictation" }));
    expect(FakeRecognition.current.stop).toHaveBeenCalledOnce();
    expect(
      screen.getByRole("button", { name: "Start dictation" }),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Start dictation" }));
    const disabling = FakeRecognition.current;
    useWorkbenchStore.setState({ projectTransitioning: true });
    view.rerender(<ChatSurface />);
    await waitFor(() => expect(disabling.abort).toHaveBeenCalledOnce());
    act(() =>
      disabling.onresult?.({
        resultIndex: 0,
        results: [{ isFinal: true, 0: { transcript: " ignored" } }],
      }),
    );
    expect(useWorkbenchStore.getState().composerText).toBe("Start hello world");
    useWorkbenchStore.setState({ projectTransitioning: false });
    view.rerender(<ChatSurface />);
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Start dictation" }),
      ).toBeVisible(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Start dictation" }));
    const previous = FakeRecognition.current;
    act(() =>
      previous.onresult?.({
        resultIndex: 0,
        results: [{ isFinal: true, 0: { transcript: "😀".repeat(5000) } }],
      }),
    );
    expect(useWorkbenchStore.getState().composerText).toBe("Start hello world");
    expect(screen.getByRole("alert")).toHaveTextContent(/too long/i);
    view.rerender(<ChatSurface chatId="chat-b" />);
    expect(previous.abort).toHaveBeenCalledOnce();
    act(() =>
      previous.onresult?.({
        resultIndex: 0,
        results: [{ isFinal: true, 0: { transcript: " late" } }],
      }),
    );
    expect(useWorkbenchStore.getState().composerText).toBe("Start hello world");
    view.rerender(<ChatSurface />);
    fireEvent.click(screen.getByRole("button", { name: "Start dictation" }));
    const sending = FakeRecognition.current;
    fireEvent.submit(
      screen.getByRole("button", { name: "Send message" }).closest("form")!,
    );
    expect(sending.abort).toHaveBeenCalledOnce();
    const sentDraft = useWorkbenchStore.getState().composerText;
    act(() =>
      sending.onresult?.({
        resultIndex: 0,
        results: [{ isFinal: true, 0: { transcript: " stale" } }],
      }),
    );
    expect(useWorkbenchStore.getState().composerText).toBe(sentDraft);
  });
  it("omits an inert plus action when no context chips are attached", () => {
    const chatId = "chat-empty-context";
    hooks.page = { ...projection(), chatId: chatId as never };
    useWorkbenchStore.setState({
      tabs: [
        {
          id: chatId,
          kind: "chat",
          title: "Context",
          position: 0,
          pinned: true,
          dirty: false,
        },
      ],
      activeTabId: chatId,
      project: { id: "project-a", name: "A", roots: [] },
    });

    render(<ChatSurface chatId={chatId} />);

    expect(document.querySelector(".composer__attachment-row")).toBeNull();
    expect(screen.queryByLabelText("Composer actions")).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Composer options" }),
    ).toBeNull();
  });

  it("names the active project in the composer scope control", () => {
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "  micrograd  ", roots: [] },
      harnesses: [
        {
          id: "sage-simulator",
          name: "Simulator",
          description: "Development",
          installState: "available",
          source: "development",
          accountModelDiscovery: false,
          transport: "legacySimulator",
          bindingId: null,
        },
      ],
      selectedHarnessId: "sage-simulator",
    });
    render(<ChatSurface />);
    openComposerOptions();

    const scope = document.querySelector(".project-scope");
    expect(scope).not.toBeNull();
    expect(scope).toHaveTextContent("micrograd");
    expect(scope).toHaveAttribute(
      "title",
      "Project: micrograd (No other projects available)",
    );
    expect(screen.queryByLabelText("Composer context")).toBeNull();
  });

  it("shows Working for nonterminal ACP turns but hides it during project transition", async () => {
    render(<ChatSurface />);
    expect(screen.getByText("1 Working")).toBeVisible();
    expect(
      screen.getByRole("status", { name: "1 working turn" }),
    ).toBeVisible();

    await act(async () => {
      useWorkbenchStore.setState({ projectTransitioning: true });
    });
    expect(screen.queryByText("1 Working")).toBeNull();
    expect(screen.queryByRole("status", { name: "1 working turn" })).toBeNull();
  });
});
