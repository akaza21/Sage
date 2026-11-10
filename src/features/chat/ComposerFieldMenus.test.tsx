import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { useState, type ComponentProps } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
import type {
  AcpChatPage,
  ProjectionRevision,
  RuntimeEpoch,
  SageChatId,
  SageProjectId,
} from "../../domain/acp";
import { agentSessionFixture } from "../../domain/acpProjection.fixture";
import { setQuickOpenInvoke } from "../quickOpen/quickOpenGateway";
import { ChatComposerRegion } from "./ChatComposerRegion";

vi.mock("../../services/acpCommandsDesktop", async (original) => ({
  ...(await original<object>()),
  acpCommandsDesktop: {
    listCommands: vi.fn(async () => ({
      revision: 1,
      commands: [{ name: "review", description: "Review", inputHint: null }],
    })),
    subscribeCommandChanges: vi.fn(async () => () => {}),
  },
}));
vi.mock("./acp/useAcpChatActions", async (original) => ({
  ...(await original<object>()),
  currentContext: () => ({
    scope: { projectId: "project-a", chatId: "chat-a" },
    ownershipEpoch: 1,
    commandProjectGeneration: "1",
    page: { projectionRevision: "4" },
  }),
}));

const initial = useWorkbenchStore.getState();
const quickOpen = vi.fn(async () => ({
  state: "ready",
  results: [{ rootId: "root-a", relativePath: "src/main.rs", positions: [] }],
  indexedFiles: 1,
  truncated: false,
  refreshing: false,
}));

function livePage(): AcpChatPage {
  return {
    projectId: "project-a" as SageProjectId,
    chatId: "chat-a" as SageChatId,
    title: "Chat",
    projectionRevision: "4" as ProjectionRevision,
    currentSessionId: agentSessionFixture().id,
    sessions: [
      agentSessionFixture({
        residency: "hot",
        runtimeEpoch: "3" as RuntimeEpoch,
      }),
    ],
    turns: [],
    nextCursor: null,
  };
}

function Composer() {
  const [text, setText] = useState("");
  const props = {
    composerRef: { current: null },
    fieldId: "message",
    dictationScope: "chat-a",
    commandChatId: "chat-a",
    descriptionId: "description",
    disabledReasonId: "disabled-reason",
    text,
    attachedContext: [],
    enabled: true,
    actionBusy: false,
    activeTurn: false,
    projectTransitioning: false,
    workbenchError: null,
    chatError: null,
    availableBinding: true,
    canPrompt: true,
    discoveryPhase: "ready",
    sessionHarnessId: null,
    sessionModelId: null,
    selectedHarnessId: null,
    selectedModelId: null,
    sessionPromptUnavailable: false,
    sessionConfig: null,
    modePending: false,
    modeError: null,
    focused: true,
    onSetSessionMode: vi.fn(),
    onSetSessionConfigOption: vi.fn(),
    onSelectHarness: vi.fn(),
    onSelectModel: vi.fn(),
    missingBindingNotice: false,
    disabledReason: null,
    project: { name: "Project" },
    usage: null,
    goal: { kind: "unsupported" },
    onSubmit: vi.fn(),
    onTextChange: setText,
    onRemoveContext: vi.fn(),
    onCancelTurn: null,
  } satisfies ComponentProps<typeof ChatComposerRegion>;
  return <ChatComposerRegion {...props} />;
}

function field() {
  return screen.getByRole<HTMLTextAreaElement>("combobox", {
    name: "Message agent",
  });
}

async function type(value: string) {
  const input = field();
  if (document.activeElement !== input) act(() => input.focus());
  await act(async () => {
    fireEvent.change(input, { target: { value } });
    input.setSelectionRange(value.length, value.length);
    fireEvent.keyUp(input);
  });
}

const OWNED = [
  "aria-autocomplete",
  "aria-controls",
  "aria-activedescendant",
] as const;

beforeEach(() => {
  useWorkbenchStore.setState({
    project: {
      id: "project-a",
      name: "Project",
      roots: [{ id: "root-a", name: "app", ordinal: 0 }],
    } as never,
    nativeTransitionEpoch: 4,
  });
  setQuickOpenInvoke(quickOpen as never);
  acpProjectionStoreFor("chat-a").setState({ page: livePage() });
});
afterEach(() => {
  cleanup();
  setQuickOpenInvoke(null);
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initial, true);
});

describe("composer / and @ menus on one field", () => {
  it("lets each menu own the field's combobox attributes only while it is active", async () => {
    render(<Composer />);
    await type("/");
    const commands = await screen.findByRole("listbox", {
      name: "Slash commands",
    });
    await waitFor(() =>
      expect(field()).toHaveAttribute("aria-controls", commands.id),
    );
    expect(field()).toHaveAttribute("aria-expanded", "true");

    // The caret moves into an `@` token: the `/` menu lets go.
    await type("/review @ma");
    const files = await screen.findByRole("listbox", {
      name: "Mention a file",
    });
    expect(
      screen.queryByRole("listbox", { name: "Slash commands" }),
    ).toBeNull();
    await waitFor(() =>
      expect(field()).toHaveAttribute("aria-controls", files.id),
    );
    expect(field()).toHaveAttribute("aria-expanded", "true");
    expect(field()).toHaveAttribute("aria-autocomplete", "list");
    await screen.findByRole("option", { name: "src/main.rs" });
    expect(field().getAttribute("aria-activedescendant")).toBe(
      screen.getByRole("option", { name: "src/main.rs" }).id,
    );

    // Neither menu is active: the field is back to the composer's own state.
    await type("/review @src/main.rs now");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(field()).toHaveAttribute("aria-expanded", "false");
    for (const name of OWNED) expect(field()).not.toHaveAttribute(name);

    // And the `/` menu takes the field again.
    await type("/");
    const again = await screen.findByRole("listbox", {
      name: "Slash commands",
    });
    await waitFor(() =>
      expect(field()).toHaveAttribute("aria-controls", again.id),
    );
    expect(field()).toHaveAttribute("aria-expanded", "true");
  });
});
