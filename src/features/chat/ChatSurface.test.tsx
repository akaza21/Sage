import { cleanup, fireEvent, render, screen } from "@testing-library/react";
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
    sessions: [],
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
      screen.getByRole("button", { name: "Open diff src/main.ts" }),
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

  it("retains the bounded legacy edit summary beside an ACP projection", () => {
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
      screen.getByRole("region", { name: "Files edited in this turn" }),
    ).toBeInTheDocument();
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
