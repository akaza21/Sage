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

import { createAcp04Ui } from "../../../scripts/source-functional-acp04-live-probe.mjs";
import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../../app/acpProjectionRegistry";
import {
  clearAgentChatOwner,
  pinAgentChatOwner,
} from "../../app/agentChatOwner";
import { useAcpProjectionStore } from "../../app/acpProjectionStore";
import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage, AcpItem } from "../../domain/acp";
import type { ResourceRef } from "../../domain/workbench";
import { acpDesktop } from "../../services/acpDesktop";
import type * as AcpModule from "./acp";

const hooks = vi.hoisted(() => ({
  page: null as AcpChatPage | null,
  actions: {
    busy: false,
    error: null as string | null,
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
            additions: 2,
            deletions: 1,
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
  useAcpProjectionStore.setState({ retentionIncomplete: false });
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
        accountModelDiscovery: true,
        transport: "acp",
        bindingId: "codex-app-server",
      },
    ],
    selectedHarnessId: "codex",
    harnessDiscoveryPhase: "ready",
    harnessDiscoveryError: null,
  });
});

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
  clearAcpProjectionRegistry();
  clearAgentChatOwner();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

describe("ChatSurface ACP resource integration", () => {
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

    await waitFor(() => {
      expect(
        Function(`return ${createAcp04Ui("codex 0.146.0").codexReady}`)(),
      ).toBe(true);
    });
  });

  it("follows an advancing ACP streaming projection to its latest content", () => {
    const scrollIntoView = vi.mocked(Element.prototype.scrollIntoView);
    const { rerender } = render(<ChatSurface />);
    scrollIntoView.mockClear();

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

    expect(scrollIntoView).toHaveBeenCalledWith({
      behavior: "smooth",
      block: "end",
    });
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
    const scrollIntoView = vi.mocked(Element.prototype.scrollIntoView);
    const { rerender } = render(<ChatSurface />);
    scrollIntoView.mockClear();

    hooks.page = {
      ...hooks.page!,
      projectionRevision: "3" as never,
    };
    rerender(<ChatSurface />);

    expect(scrollIntoView).toHaveBeenCalledWith({
      behavior: "auto",
      block: "end",
    });
  });

  it("stops following when the reader scrolls away and offers a return to latest", () => {
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      callback(0);
      return 1;
    });
    const scrollIntoView = vi.mocked(Element.prototype.scrollIntoView);
    const { rerender } = render(<ChatSurface />);
    const transcript = document.querySelector(".message-list");
    expect(transcript).not.toBeNull();
    Object.defineProperties(transcript!, {
      clientHeight: { configurable: true, value: 400 },
      scrollHeight: { configurable: true, value: 1_000 },
      scrollTop: { configurable: true, value: 100, writable: true },
    });

    fireEvent.scroll(transcript!);
    scrollIntoView.mockClear();
    hooks.page = {
      ...hooks.page!,
      projectionRevision: "4" as never,
    };
    rerender(<ChatSurface />);

    expect(scrollIntoView).not.toHaveBeenCalled();
    const returnToLatest = screen.getByRole("button", {
      name: "Jump to latest",
    });
    fireEvent.click(returnToLatest);
    expect(scrollIntoView).toHaveBeenCalledWith({
      behavior: "smooth",
      block: "end",
    });
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

    expect(screen.getByText("Unavailable")).toBeInTheDocument();
    expect(screen.queryByText("Live")).toBeNull();
    expect(screen.queryByRole("button", { name: "Attach context" })).toBeNull();
    expect(screen.queryByText("Read-only policy")).toBeNull();
    const harness = screen.getByRole("button", { name: "Harness" });
    fireEvent.click(harness);
    const options = screen.getAllByRole("option");
    expect(options).toHaveLength(2);
    expect(options[0]).toHaveTextContent("Codex — not installed");
    expect(options[0]).toBeDisabled();
    expect(options[1]).toHaveTextContent("Simulator — test only");
    expect(screen.queryByRole("option", { name: "Pi" })).toBeNull();
    expect(screen.queryByRole("option", { name: "Claude Code" })).toBeNull();
    expect(screen.queryByRole("option", { name: "Oh My Pi" })).toBeNull();
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

    const harness = screen.getByRole("button", { name: "Harness" });
    fireEvent.click(harness);
    const options = screen.getAllByRole("option");
    expect(options).toHaveLength(2);
    expect(options[0]).toHaveTextContent("Codex — unavailable");
    expect(options[0]).toBeDisabled();
    expect(options[1]).toHaveTextContent("Simulator — test only");
    expect(screen.queryByRole("option", { name: "Pi" })).toBeNull();
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

    expect(screen.getByText("No harness")).toBeInTheDocument();
    const harness = screen.getByRole("button", { name: "Harness" });
    fireEvent.click(harness);
    const options = screen.getAllByRole("option");
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
    expect(screen.queryByText("Read-only policy")).toBeNull();
    expect(
      screen.queryByText(
        "No reviewed local binding is available for this harness.",
      ),
    ).toBeNull();
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
    const dialog = screen.getByRole("dialog", { name: "Hand off this Chat" });
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
      description:
        /Send a message to the selected harness and model.*active Turn/i,
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
        name: "Unavailable harness — unavailableHarness is unavailable.",
      }),
    ).toBeDisabled();
    fireEvent.click(screen.getByRole("option", { name: "Codex" }));
    expect(selectHarness).toHaveBeenCalledExactlyOnceWith("codex");
    act(() =>
      useWorkbenchStore.setState({
        selectedHarnessId: "codex",
        selectedModelId: null,
      }),
    );
    expect(
      screen.getByText(/Checking Codex account and available models/),
    ).toBeVisible();
    expect(screen.getByLabelText("Model")).toBeDisabled();
    expect(
      screen.getByText("Select a ready harness to load models."),
    ).toBeVisible();
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
    const listbox = screen.getByRole("listbox", { name: "Model" });
    expect(screen.queryByRole("option", { name: /old model/i })).toBeNull();
    expect(within(listbox).getAllByRole("option")).toHaveLength(128);
    expect(
      within(listbox).getByRole("option", {
        name: "Fresh advertised model 1",
      }),
    ).toHaveAttribute("aria-selected", "true");
    const input = screen.getByRole("combobox", { name: "Model" });
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(selectModel).toHaveBeenCalledExactlyOnceWith("fresh-model-2");
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
      screen.getByText(/sign in to chatgpt to load models/i),
    ).toBeVisible();
    expect(screen.getByLabelText("Message agent")).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    expect(screen.getByText(/official openai sign-in page/i)).toBeVisible();
    expect(screen.getByText(/does not receive or retain them/i)).toBeVisible();
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
    fireEvent.click(
      await screen.findByRole("button", { name: "Open file src/main.ts" }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Open diff B / src/main.ts" }),
    );

    expect(openFile).toHaveBeenCalledWith({
      rootId: "root-a",
      relativePath: "src/main.ts",
    });
    expect(openDiff).toHaveBeenCalledWith({
      rootId: "root-b",
      relativePath: "src/main.ts",
    });

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

    expect(await screen.findByText("Showing 1 edited file")).toBeVisible();
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
          sending: false,
          error: null,
          requestGeneration: 0,
        },
        "chat-b": {
          composerText: "",
          sending: false,
          error: null,
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
    const composers = screen.getAllByPlaceholderText("Ask anything (Ctrl+L)");
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
        ?.querySelectorAll("[data-acp-announcement]").length ?? 0
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
    expect(document.querySelectorAll("[data-acp-announcement]")).toHaveLength(
      2,
    );
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
    expect(document.querySelectorAll("[data-acp-announcement]")).toHaveLength(
      2,
    );
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
    expect(composer).toHaveAttribute("placeholder", "Ask anything (Ctrl+L)");
    composer.blur();
    expect(composer).not.toHaveFocus();
    fireEvent.keyDown(window, { key: "l", ctrlKey: true });
    expect(composer).toHaveFocus();
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

describe("ChatSurface composer context", () => {
  it("shows Local and the project workspace name under the composer", () => {
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
    const context = screen.getByLabelText("Composer context");
    expect(context).toHaveTextContent("Local");
    expect(context).toHaveTextContent("micrograd");
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
