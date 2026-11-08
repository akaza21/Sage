import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { useState, type ComponentProps } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../../app/acpProjectionRegistry";
import type {
  AcpChatPage,
  ProjectionRevision,
  RuntimeEpoch,
  SageChatId,
  SageProjectId,
} from "../../domain/acp";
import { agentSessionFixture } from "../../domain/acpProjection.fixture";
import type {
  AcpCommandList,
  AcpCommandsChangedV1,
} from "../../services/acpCommandsDesktop";
import { ChatComposerRegion } from "./ChatComposerRegion";

const native = vi.hoisted(() => ({
  lists: [] as AcpCommandList[],
  requests: [] as unknown[],
  listeners: new Set<(event: AcpCommandsChangedV1) => void>(),
}));
vi.mock("../../services/acpCommandsDesktop", async (original) => ({
  ...(await original<object>()),
  acpCommandsDesktop: {
    listCommands: vi.fn(async (request: unknown) => {
      native.requests.push(request);
      // The last list stays current, as the native list does.
      return native.lists.length > 1 ? native.lists.shift()! : native.lists[0]!;
    }),
    subscribeCommandChanges: vi.fn(
      async (listener: (event: AcpCommandsChangedV1) => void) => {
        native.listeners.add(listener);
        return () => native.listeners.delete(listener);
      },
    ),
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

const commands = [
  { name: "compact", description: "Compact the conversation", inputHint: null },
  { name: "review", description: "Review a pull request", inputHint: "PR" },
  { name: "mcp:docs:search", description: "Search docs", inputHint: "query" },
];

function livePage(runtimeEpoch: string | null = "3"): AcpChatPage {
  return {
    projectId: "project-a" as SageProjectId,
    chatId: "chat-a" as SageChatId,
    title: "Chat",
    projectionRevision: "4" as ProjectionRevision,
    currentSessionId: agentSessionFixture().id,
    sessions: [
      agentSessionFixture({
        residency: "hot",
        runtimeEpoch: runtimeEpoch as RuntimeEpoch | null,
      }),
    ],
    turns: [],
    nextCursor: null,
  };
}

const onSubmit = vi.fn((event: { preventDefault: () => void }) =>
  event.preventDefault(),
);

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
    onSubmit,
    onTextChange: setText,
    onRemoveContext: vi.fn(),
    onCancelTurn: null,
  } satisfies ComponentProps<typeof ChatComposerRegion>;
  return <ChatComposerRegion {...props} />;
}

function type(text: string) {
  const field = screen.getByRole("textbox", { name: "Message agent" });
  if (field.ownerDocument.activeElement !== field) act(() => field.focus());
  fireEvent.change(field, { target: { value: text } });
  return field;
}

async function openMenu(text = "/") {
  const field = type(text);
  const listbox = await screen.findByRole("listbox", {
    name: "Slash commands",
  });
  await waitFor(() =>
    expect(within(listbox).queryAllByRole("option").length).toBeGreaterThan(0),
  );
  return { field, listbox };
}

const names = (listbox: HTMLElement) =>
  within(listbox)
    .getAllByRole("option")
    .map((option) => option.getAttribute("aria-label")?.split(", ")[0]);

beforeEach(() => {
  native.lists = [{ revision: 1, commands }];
  native.requests = [];
  native.listeners.clear();
  acpProjectionStoreFor("chat-a").setState({ page: livePage() });
});
afterEach(() => {
  cleanup();
  clearAcpProjectionRegistry();
  onSubmit.mockClear();
});

describe("composer slash commands", () => {
  it("lists the session's commands for its live runtime and filters by name", async () => {
    render(<Composer />);
    const { field, listbox } = await openMenu();
    expect(native.requests[0]).toMatchObject({
      projectId: "project-a",
      chatId: "chat-a",
      sessionId: "session-a",
      runtimeEpoch: "3",
    });
    expect(names(listbox)).toEqual(["/compact", "/review", "/mcp:docs:search"]);
    expect(
      within(listbox).getByRole("option", {
        name: "/review, Review a pull request, Input: PR",
      }),
    ).toBeInTheDocument();
    expect(field).toHaveAttribute("aria-controls", listbox.id);

    type("/s");
    // Prefix matches lead; other name matches follow.
    await waitFor(() => expect(names(listbox)).toEqual(["/mcp:docs:search"]));
    type("/re");
    await waitFor(() => expect(names(listbox)).toEqual(["/review"]));
    type("/zz");
    await screen.findByText("No commands match “/zz”.");
    expect(within(listbox).queryAllByRole("option")).toHaveLength(0);
    // A space ends the command name and closes the menu.
    type("/review x");
    expect(screen.queryByRole("listbox")).toBeNull();
    // The menu hides while the focus is elsewhere and returns with it.
    type("/");
    await screen.findByRole("listbox", { name: "Slash commands" });
    act(() => field.blur());
    expect(screen.queryByRole("listbox")).toBeNull();
    act(() => field.focus());
    expect(
      await screen.findByRole("listbox", { name: "Slash commands" }),
    ).toBeInTheDocument();
  });

  it("moves with the arrows, picks with Enter or Tab and closes with Escape", async () => {
    render(<Composer />);
    const { field, listbox } = await openMenu();
    const active = () =>
      field.ownerDocument.getElementById(
        field.getAttribute("aria-activedescendant") ?? "",
      );
    await waitFor(() => expect(active()).toHaveTextContent("/compact"));
    fireEvent.keyDown(field, { key: "ArrowDown" });
    expect(active()).toHaveTextContent("/review");
    expect(active()).toHaveAttribute("aria-selected", "true");
    fireEvent.keyDown(field, { key: "ArrowUp" });
    fireEvent.keyDown(field, { key: "ArrowUp" });
    expect(active()).toHaveTextContent("/mcp:docs:search");
    expect(listbox).toBeInTheDocument();

    fireEvent.keyDown(field, { key: "Enter" });
    expect(field).toHaveValue("/mcp:docs:search ");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(onSubmit).not.toHaveBeenCalled();

    await openMenu("/rev");
    fireEvent.keyDown(field, { key: "Tab" });
    expect(field).toHaveValue("/review ");

    await openMenu("/c");
    fireEvent.keyDown(field, { key: "Escape" });
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(field).toHaveValue("/c");
    // Escape holds until the composer leaves command-name text.
    type("/co");
    expect(screen.queryByRole("listbox")).toBeNull();
    type("");
    await openMenu("/");
  });

  it("shows the picked command's input hint until input is typed and sends through the prompt path", async () => {
    render(<Composer />);
    const { field } = await openMenu("/re");
    fireEvent.click(screen.getByRole("option", { name: /^\/review/u }));
    expect(field).toHaveValue("/review ");
    const hint = document.getElementById(
      field.getAttribute("aria-describedby")!.split(" ").at(-1)!,
    );
    expect(hint).toHaveTextContent("/review expects PR");
    expect(document.querySelector(".composer__command-hint")).toHaveTextContent(
      "/review PR",
    );
    type("/review 42");
    expect(document.querySelector(".composer__command-hint")).toBeNull();
    fireEvent.keyDown(field, { key: "Enter" });
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });

  it("re-reads the list when the session announces a new revision", async () => {
    render(<Composer />);
    const { listbox } = await openMenu();
    native.lists = [
      {
        revision: 2,
        commands: [{ name: "init", description: "Set up", inputHint: null }],
      },
    ];
    // Another session's notice is ignored.
    act(() =>
      native.listeners.forEach((listener) =>
        listener({
          sageProjectId: "project-a",
          sageChatId: "chat-a",
          sageAgentSessionId: "session-b",
          runtimeEpoch: "3",
          revision: 2,
        }),
      ),
    );
    expect(native.requests).toHaveLength(1);
    act(() =>
      native.listeners.forEach((listener) =>
        listener({
          sageProjectId: "project-a",
          sageChatId: "chat-a",
          sageAgentSessionId: "session-a",
          runtimeEpoch: "3",
          revision: 2,
        }),
      ),
    );
    await waitFor(() => expect(names(listbox)).toEqual(["/init"]));
  });

  it("says when the harness advertises none or the session is not running", async () => {
    native.lists = [{ revision: 1, commands: [] }];
    render(<Composer />);
    type("/");
    expect(
      await screen.findByText("This harness advertises no slash commands."),
    ).toBeInTheDocument();
    act(() =>
      acpProjectionStoreFor("chat-a").setState({ page: livePage(null) }),
    );
    expect(
      await screen.findByText("Commands appear once the session is running."),
    ).toBeInTheDocument();
    // With nothing to pick, Enter still sends the typed text.
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Message agent" }), {
      key: "Enter",
    });
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });
});
