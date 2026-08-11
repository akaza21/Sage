import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage, AcpItem } from "../../domain/acp";
import type { ResourceRef } from "../../domain/workbench";
import type * as AcpModule from "./acp";

const hooks = vi.hoisted(() => ({
  page: null as AcpChatPage | null,
  actions: {
    busy: false,
    error: null,
    cancellableTurnId: null as string | null,
    submit: vi.fn(),
    cancel: vi.fn(),
    close: vi.fn(),
    clearError: vi.fn(),
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
          kind: "file",
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
    currentSessionId: null,
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

beforeEach(() => {
  hooks.page = projection();
  hooks.actions.busy = false;
  hooks.actions.error = null;
  hooks.actions.cancellableTurnId = null;
  Element.prototype.scrollIntoView = vi.fn();
  useWorkbenchStore.setState({
    harnesses: [
      {
        id: "codex",
        name: "Codex",
        description: "ACP",
        installState: "available",
        source: "builtIn",
        transport: "acp",
        bindingId: "codex-app-server",
      },
    ],
    selectedHarnessId: "codex",
  });
});

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
  vi.clearAllMocks();
});

describe("ChatSurface ACP resource integration", () => {
  it("keeps selected unavailable Codex truthful and hides later built-in harnesses", () => {
    hooks.page = null;
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "codex",
          name: "Codex",
          description: "ACP",
          installState: "notInstalled",
          source: "builtIn",
          transport: "acp",
          bindingId: "codex-app-server",
        },
        {
          id: "pi",
          name: "Pi",
          description: "Later harness",
          installState: "notInstalled",
          source: "builtIn",
          transport: "acp",
          bindingId: null,
        },
        {
          id: "claude-code",
          name: "Claude Code",
          description: "Later harness",
          installState: "notInstalled",
          source: "builtIn",
          transport: "acp",
          bindingId: null,
        },
        {
          id: "oh-my-pi",
          name: "Oh My Pi",
          description: "Later harness",
          installState: "notInstalled",
          source: "builtIn",
          transport: "acp",
          bindingId: null,
        },
        {
          id: "sage-simulator",
          name: "Simulator",
          description: "Development",
          installState: "available",
          source: "development",
          transport: "legacySimulator",
          bindingId: null,
        },
      ],
      selectedHarnessId: "codex",
    });

    render(<ChatSurface />);

    expect(screen.getByText("Unavailable")).toBeInTheDocument();
    expect(screen.queryByText("Live")).toBeNull();
    expect(screen.queryByRole("button", { name: "Attach context" })).toBeNull();
    expect(screen.queryByText("Read-only policy")).toBeNull();
    const harness = screen.getByRole("combobox", { name: "Harness" });
    const options = within(harness).getAllByRole("option");
    expect(options).toHaveLength(2);
    expect(options[0]).toHaveTextContent("Codex — unavailable");
    expect(options[0]).toBeDisabled();
    expect(options[1]).toHaveTextContent("Simulator — test only");
    expect(screen.queryByRole("option", { name: "Pi" })).toBeNull();
    expect(screen.queryByRole("option", { name: "Claude Code" })).toBeNull();
    expect(screen.queryByRole("option", { name: "Oh My Pi" })).toBeNull();
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
          transport: "acp",
          bindingId: null,
        },
        {
          id: "sage-simulator",
          name: "Simulator",
          description: "Development",
          installState: "available",
          source: "development",
          transport: "legacySimulator",
          bindingId: null,
        },
      ],
      selectedHarnessId: "removed-harness",
    });

    render(<ChatSurface />);

    expect(screen.getByText("No harness")).toBeInTheDocument();
    const harness = screen.getByRole("combobox", { name: "Harness" });
    const options = within(harness).getAllByRole("option");
    expect(options).toHaveLength(1);
    expect(options[0]).toHaveTextContent("Simulator — test only");
    expect(screen.queryByRole("option", { name: "Pi" })).toBeNull();
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
    expect(screen.queryByText("Read-only policy")).toBeNull();
  });

  it("shows an available ACP binding as ready before a session starts", () => {
    render(<ChatSurface />);

    expect(screen.getByText("Ready")).toBeInTheDocument();
    expect(screen.queryByText("Live")).toBeNull();
    expect(screen.getByText("Read-only policy")).toBeInTheDocument();
  });

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
      ],
      activeTabId: "chat-a",
      harnesses: [
        {
          id: "sage-simulator",
          name: "Simulator",
          description: "Development",
          installState: "available",
          source: "development",
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
    expect(
      screen.getByRole("dialog", { name: "Hand off this Chat" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Target Chat" })).toHaveValue(
      "chat-b",
    );
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
    act(() => useWorkbenchStore.setState({ projectTransitioning: false }));
    fireEvent.click(trigger);
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
        targetChatId: "chat-b",
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

  it("includes only primary ACP text in the Chat Handoff preview", () => {
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
    const preview = screen.getByRole("textbox", { name: "Handoff prompt" });
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
    const dialog = screen.getByRole("dialog", { name: "Hand off this Chat" });
    const first = screen.getByRole("combobox", { name: "Target Chat" });
    const last = screen.getByRole("button", { name: "Place in target" });
    first.focus();
    fireEvent.keyDown(first, { key: "Tab", shiftKey: true });
    expect(last).toHaveFocus();
    fireEvent.keyDown(last, { key: "Tab" });
    expect(first).toHaveFocus();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
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
    fireEvent.click(screen.getByRole("button", { name: "Place in target" }));
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

  it("separates active Turn prompting from the optional Cancel capability", () => {
    useWorkbenchStore.setState({ composerText: "Follow up" });
    const { rerender } = render(<ChatSurface />);
    expect(document.querySelector(".message-list")).not.toHaveAttribute(
      "aria-live",
    );
    const composer = screen.getByRole("textbox", {
      name: "Message agent",
      description: "Send a message to the selected harness and model",
    });

    expect(composer).toBeDisabled();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Cancel turn" })).toBeNull();

    hooks.actions.cancellableTurnId = "turn-a";
    rerender(<ChatSurface />);
    expect(screen.queryByRole("button", { name: "Send message" })).toBeNull();
    const cancel = screen.getByRole("button", { name: "Cancel turn" });
    expect(cancel).toBeEnabled();
    fireEvent.click(cancel);
    expect(hooks.actions.cancel).toHaveBeenCalledWith("turn-a");

    hooks.actions.cancellableTurnId = null;
    setTurnState("cancelling");
    rerender(<ChatSurface />);
    expect(composer).toBeDisabled();
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

  it("keeps ACP prompting disabled until authoritative Chat state loads", () => {
    hooks.page = null;
    render(<ChatSurface />);

    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    expect(screen.getByLabelText("Message agent")).toBeDisabled();
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

  it("routes duplicate-path ACP file and diff actions by canonical ResourceRef", () => {
    const openFile = vi.fn().mockResolvedValue(undefined);
    const openDiff = vi.fn().mockResolvedValue(undefined);
    useWorkbenchStore.setState({
      projectTransitioning: false,
      openFile,
      openDiff,
    });

    render(<ChatSurface />);
    fireEvent.click(
      screen.getByRole("button", { name: "Open file src/main.ts" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Open diff root-b / src/main.ts" }),
    );

    expect(openFile).toHaveBeenCalledWith({
      rootId: "root-a",
      relativePath: "src/main.ts",
    });
    expect(openDiff).toHaveBeenCalledWith({
      rootId: "root-b",
      relativePath: "src/main.ts",
    });
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
