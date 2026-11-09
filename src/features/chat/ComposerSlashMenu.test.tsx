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
import {
  agentSessionFixture,
  capabilitySetFixture,
} from "../../domain/acpProjection.fixture";
import type {
  AcpCommandList,
  AcpCommandsChangedV1,
} from "../../services/acpCommandsDesktop";
import { useWorkbenchStore } from "../../app/store";
import { ChatComposerRegion } from "./ChatComposerRegion";
import { pickedCommandFor } from "./slashCommandPicks";

const native = vi.hoisted(() => ({
  lists: [] as AcpCommandList[],
  requests: [] as unknown[],
  hold: null as Promise<AcpCommandList> | null,
  listeners: new Set<(event: AcpCommandsChangedV1) => void>(),
  run: vi.fn(),
}));
vi.mock("../../services/acpCommandsDesktop", async (original) => ({
  ...(await original<object>()),
  acpCommandsDesktop: {
    listCommands: vi.fn(async (request: unknown) => {
      native.requests.push(request);
      if (native.hold) return native.hold;
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
  chatActionController: () => ({ runSessionAction: native.run }),
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
  const field = screen.getByRole("combobox", { name: "Message agent" });
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
  native.hold = null;
  native.listeners.clear();
  acpProjectionStoreFor("chat-a").setState({ page: livePage() });
});
afterEach(() => {
  cleanup();
  clearAcpProjectionRegistry();
  onSubmit.mockClear();
});

function announce(sessionId = "session-a", revision = 2) {
  act(() =>
    native.listeners.forEach((listener) =>
      listener({
        sageProjectId: "project-a",
        sageChatId: "chat-a",
        sageAgentSessionId: sessionId,
        runtimeEpoch: "3",
        revision,
      }),
    ),
  );
}

const TITLE = ".resource-state__title";
const status = () =>
  document.querySelector(".composer__input-area > [role='status']");

describe("composer slash commands", () => {
  it("lists the session's commands for its live runtime and filters by name", async () => {
    render(<Composer />);
    const field = screen.getByRole("combobox", { name: "Message agent" });
    expect(field).not.toHaveAttribute("aria-autocomplete");
    expect(field).toHaveAttribute("aria-expanded", "false");
    const { listbox } = await openMenu();
    expect(native.requests[0]).toMatchObject({
      projectId: "project-a",
      chatId: "chat-a",
      sessionId: "session-a",
      runtimeEpoch: "3",
    });
    // The chat's own commands follow the harness's.
    expect(names(listbox)).toEqual([
      "/compact",
      "/review",
      "/mcp:docs:search",
      "/init",
      "/status",
    ]);
    expect(
      within(listbox).getByRole("option", {
        name: "/review, Review a pull request, Input: PR",
      }),
    ).toBeInTheDocument();
    expect(field).toHaveAttribute("aria-controls", listbox.id);
    expect(field).toHaveAttribute("aria-expanded", "true");
    expect(field).toHaveAttribute("aria-autocomplete", "list");
    // The status region lives beside the field, not inside the popover.
    await waitFor(() => expect(status()).toHaveTextContent("5 commands"));

    type("/s");
    // Prefix matches lead; other name matches follow.
    await waitFor(() =>
      expect(names(listbox)).toEqual(["/status", "/mcp:docs:search"]),
    );
    type("/re");
    await waitFor(() => expect(names(listbox)).toEqual(["/review"]));
    type("/zz");
    await screen.findByText("No commands match “/zz”.", { selector: TITLE });
    expect(within(listbox).queryAllByRole("option")).toHaveLength(0);
    // With the caret past the name the menu closes, and inside it reopens.
    type("/review x");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(field).not.toHaveAttribute("aria-autocomplete");
    (field as HTMLTextAreaElement).setSelectionRange(3, 3);
    fireEvent.select(field);
    await waitFor(() =>
      expect(names(screen.getByRole("listbox"))).toEqual(["/review"]),
    );
    // The menu hides while the focus is elsewhere and returns with it.
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
    expect(active()).toHaveTextContent("/status");
    fireEvent.keyDown(field, { key: "ArrowUp" });
    fireEvent.keyDown(field, { key: "ArrowUp" });
    expect(active()).toHaveTextContent("/mcp:docs:search");
    expect(listbox).toBeInTheDocument();

    fireEvent.keyDown(field, { key: "Enter" });
    expect(field).toHaveValue("/mcp:docs:search ");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(onSubmit).not.toHaveBeenCalled();

    await openMenu("/rev");
    // Shift+Tab leaves the field as usual; the menu does not take it.
    expect(fireEvent.keyDown(field, { key: "Tab", shiftKey: true })).toBe(true);
    expect(field).toHaveValue("/rev");
    fireEvent.keyDown(field, { key: "Tab" });
    expect(field).toHaveValue("/review ");

    await openMenu("/co");
    fireEvent.keyDown(field, { key: "Escape" });
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(field).toHaveValue("/co");
    // Escape holds until the caret leaves the command name.
    type("/com");
    expect(screen.queryByRole("listbox")).toBeNull();
    type("");
    await openMenu("/");
  });

  it("leaves keys to an IME composition", async () => {
    render(<Composer />);
    const { field } = await openMenu("/re");
    expect(fireEvent.keyDown(field, { key: "Enter", isComposing: true })).toBe(
      true,
    );
    fireEvent.keyDown(field, { key: "Tab", isComposing: true });
    // Safari and WebView2 end a composition with keyCode 229 alone.
    expect(fireEvent.keyDown(field, { key: "Enter", keyCode: 229 })).toBe(true);
    fireEvent.keyDown(field, { key: "Tab", keyCode: 229 });
    expect(field).toHaveValue("/re");
    expect(screen.getByRole("listbox")).toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();

    // The composer's own Enter is held by both signals too.
    type("hello");
    expect(screen.queryByRole("listbox")).toBeNull();
    fireEvent.keyDown(field, { key: "Enter", isComposing: true });
    fireEvent.keyDown(field, { key: "Enter", keyCode: 229 });
    expect(onSubmit).not.toHaveBeenCalled();
    fireEvent.keyDown(field, { key: "Enter" });
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });

  it("ranks the exact name first, then prefixes, then other matches", async () => {
    native.lists = [
      {
        revision: 1,
        commands: [
          { name: "pr-review", description: "Inner", inputHint: null },
          { name: "review-all", description: "Prefix", inputHint: null },
          { name: "review", description: "Exact", inputHint: null },
        ],
      },
    ];
    render(<Composer />);
    const { listbox } = await openMenu("/review");
    await waitFor(() =>
      expect(names(listbox)).toEqual(["/review", "/review-all", "/pr-review"]),
    );
  });

  it("opens again after Escape once the focus leaves and returns", async () => {
    render(<Composer />);
    const { field } = await openMenu("/co");
    fireEvent.keyDown(field, { key: "Escape" });
    expect(screen.queryByRole("listbox")).toBeNull();
    act(() => field.blur());
    act(() => field.focus());
    expect(
      await screen.findByRole("listbox", { name: "Slash commands" }),
    ).toBeInTheDocument();
  });

  it("is a combobox that keeps the composer's name and description", async () => {
    render(<Composer />);
    const field = screen.getByRole("combobox", { name: "Message agent" });
    expect(field.tagName).toBe("TEXTAREA");
    expect(field).toHaveAccessibleName("Message agent");
    expect(field).toHaveAccessibleDescription(/Send a message/u);
    expect(field).toHaveAttribute("aria-expanded", "false");
    const { listbox } = await openMenu("/");
    expect(field).toHaveAttribute("aria-expanded", "true");
    expect(field).toHaveAttribute("aria-autocomplete", "list");
    expect(field).toHaveAttribute("aria-controls", listbox.id);
    await waitFor(() =>
      expect(field.getAttribute("aria-activedescendant")).toMatch(/compact$/u),
    );
    expect(field).toHaveAccessibleName("Message agent");
  });

  it("picks with the mouse, keeps typed input, shows the hint and sends the pick as a command", async () => {
    render(<Composer />);
    const { field } = await openMenu("/re");
    fireEvent.click(screen.getByRole("option", { name: /^\/review/u }));
    expect(field).toHaveValue("/review ");
    expect(field).toHaveFocus();
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
    // The send route sees the pick, so a withdrawn command is refused.
    expect(pickedCommandFor("chat-a")).toBe("review");

    // A pick replaces only the name: the input after it stays.
    type("/co 42");
    (field as HTMLTextAreaElement).setSelectionRange(3, 3);
    fireEvent.select(field);
    await waitFor(() =>
      expect(names(screen.getByRole("listbox"))).toEqual(["/compact"]),
    );
    fireEvent.keyDown(field, { key: "Enter" });
    expect(field).toHaveValue("/compact 42");
  });

  it("runs a fully typed name on the first Enter, as the Claude Code CLI does", async () => {
    render(<Composer />);
    const { field } = await openMenu("/compact");
    fireEvent.keyDown(field, { key: "Enter" });
    expect(field).toHaveValue("/compact");
    expect(onSubmit).toHaveBeenCalledTimes(1);
    // Not picked from the menu: the send path matches it against the list.
    expect(pickedCommandFor("chat-a")).toBeUndefined();
  });

  it("does not send a half-typed name while the list is loading", async () => {
    native.hold = new Promise(() => undefined);
    render(<Composer />);
    const field = type("/comp");
    await screen.findByText("Loading commands…", { selector: TITLE });
    fireEvent.keyDown(field, { key: "Enter" });
    expect(onSubmit).not.toHaveBeenCalled();
    expect(field).toHaveValue("/comp");
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
    announce("session-b");
    expect(native.requests).toHaveLength(1);
    announce();
    await waitFor(() => expect(names(listbox)).toEqual(["/init", "/status"]));
  });

  it("keeps the newer list when an update lands during a read", async () => {
    let finishFirst: (list: AcpCommandList) => void = () => undefined;
    native.hold = new Promise((resolve) => (finishFirst = resolve));
    render(<Composer />);
    type("/");
    await waitFor(() => expect(native.requests).toHaveLength(1));
    native.hold = null;
    native.lists = [
      {
        revision: 2,
        commands: [{ name: "init", description: "Set up", inputHint: null }],
      },
    ];
    announce();
    await waitFor(() => expect(native.requests).toHaveLength(2));
    // The first read finishes last with the older list; it is dropped.
    await act(async () => finishFirst({ revision: 1, commands }));
    const listbox = await screen.findByRole("listbox");
    await waitFor(() => expect(names(listbox)).toEqual(["/init", "/status"]));
  });

  it("follows a session switch while the menu is open", async () => {
    render(<Composer />);
    const { listbox } = await openMenu();
    native.lists = [
      {
        revision: 1,
        commands: [{ name: "init", description: "Set up", inputHint: null }],
      },
    ];
    const page = livePage();
    act(() =>
      acpProjectionStoreFor("chat-a").setState({
        page: {
          ...page,
          currentSessionId: "session-b" as AcpChatPage["currentSessionId"],
          sessions: [{ ...page.sessions[0]!, id: "session-b" as never }],
        },
      }),
    );
    await waitFor(() => expect(names(listbox)).toEqual(["/init", "/status"]));
    expect(native.requests.at(-1)).toMatchObject({ sessionId: "session-b" });
  });

  it("lists the chat's own commands while the harness advertises none", async () => {
    native.lists = [{ revision: 0, commands: [] }];
    render(<Composer />);
    const { field, listbox } = await openMenu();
    expect(names(listbox)).toEqual(["/init", "/status"]);
    native.lists = [{ revision: 1, commands: [] }];
    announce("session-a", 1);
    await waitFor(() => expect(native.requests).toHaveLength(2));
    expect(names(listbox)).toEqual(["/init", "/status"]);
    act(() =>
      acpProjectionStoreFor("chat-a").setState({ page: livePage(null) }),
    );
    expect(
      await screen.findByText("Commands appear once the session is running.", {
        selector: TITLE,
      }),
    ).toBeInTheDocument();
    // No list can load before the session starts, so Enter sends the text
    // as it stands; the send path starts the session and checks the list.
    fireEvent.keyDown(field, { key: "Enter" });
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });
});

describe("composer session commands", () => {
  const run = native.run;
  function negotiate(page = livePage()) {
    const session = page.sessions[0]!;
    acpProjectionStoreFor("chat-a").setState({
      page: {
        ...page,
        sessions: [
          {
            ...session,
            capabilities: capabilitySetFixture({
              nativeSessionCompact: true,
              nativeReview: true,
            }),
          },
        ],
      },
    });
  }
  const { harnesses, repositories } = useWorkbenchStore.getState();
  beforeEach(() => {
    run.mockReset();
    negotiate();
  });
  afterEach(() => useWorkbenchStore.setState({ harnesses, repositories }));

  it("lists a negotiated action in place of the harness command of that name", async () => {
    render(<Composer />);
    const { listbox } = await openMenu();
    expect(names(listbox)).toEqual([
      "/review",
      "/compact",
      "/mcp:docs:search",
      "/init",
      "/status",
    ]);
    expect(
      within(listbox).getByRole("option", {
        name: /^\/review, Review changes/u,
      }),
    ).toBeInTheDocument();
  });

  it("lists no action the session did not negotiate", async () => {
    render(<Composer />);
    const { listbox } = await openMenu();
    expect(names(listbox)[0]).toBe("/review");
    act(() => acpProjectionStoreFor("chat-a").setState({ page: livePage() }));
    // The next keystroke re-reads the negotiated actions: the harness's own
    // /compact and /review return.
    type("/r");
    type("/");
    await waitFor(() =>
      expect(names(listbox)).toEqual([
        "/compact",
        "/review",
        "/mcp:docs:search",
        "/init",
        "/status",
      ]),
    );
  });

  it("runs /compact from a pick or a fully typed name, and clears the field", async () => {
    render(<Composer />);
    const { field } = await openMenu("/comp");
    fireEvent.keyDown(field, { key: "Enter" });
    expect(run).toHaveBeenCalledWith({ kind: "compact" });
    expect(field).toHaveValue("");
    await openMenu("/compact");
    fireEvent.keyDown(field, { key: "Enter" });
    expect(run).toHaveBeenCalledTimes(2);
    expect(field).toHaveValue("");
    expect(onSubmit).not.toHaveBeenCalled();
    expect(pickedCommandFor("chat-a")).toBeUndefined();
  });

  it("writes the /init prompt into the field without sending it", async () => {
    render(<Composer />);
    const { field } = await openMenu("/ini");
    fireEvent.keyDown(field, { key: "Enter" });
    expect((field as HTMLTextAreaElement).value).toMatch(
      /^Create an AGENTS\.md file at the root of this project/u,
    );
    expect(onSubmit).not.toHaveBeenCalled();
    expect(run).not.toHaveBeenCalled();
    expect(pickedCommandFor("chat-a")).toBeUndefined();
  });

  it("starts a review of each target from the /review picker", async () => {
    useWorkbenchStore.setState({
      repositories: {
        roots: [
          {
            rootId: "root",
            state: "repository",
            branch: "topic",
            branches: ["main", "topic", "release"],
            recentCommits: [{ sha: "abc1234def", subject: "Fix the parser" }],
            changes: [],
            truncated: false,
            elapsedMs: 0,
            message: null,
          },
        ],
        truncated: false,
        elapsedMs: 0,
      },
    });
    render(<Composer />);
    const review = async () => {
      const { field } = await openMenu("/rev");
      fireEvent.keyDown(field, { key: "Enter" });
      expect(field).toHaveValue("");
      return screen.findByRole("dialog", { name: "Review" });
    };
    const start = () =>
      fireEvent.click(screen.getByRole("button", { name: "Start review" }));

    await review();
    start();
    expect(run).toHaveBeenLastCalledWith({
      kind: "review",
      target: { kind: "uncommittedChanges" },
    });
    expect(screen.queryByRole("dialog")).toBeNull();

    let dialog = await review();
    fireEvent.click(within(dialog).getByLabelText("Against a base branch"));
    // The current branch is not offered.
    const base = within(dialog).getByRole("combobox", { name: "Base branch" });
    expect(
      within(base)
        .getAllByRole("option")
        .map((option) => option.textContent),
    ).toEqual(["main", "release"]);
    fireEvent.change(base, { target: { value: "release" } });
    start();
    expect(run).toHaveBeenLastCalledWith({
      kind: "review",
      target: { kind: "baseBranch", branch: "release" },
    });

    dialog = await review();
    fireEvent.click(within(dialog).getByLabelText("A commit"));
    const sha = within(dialog).getByRole("combobox", { name: "Commit SHA" });
    fireEvent.change(sha, { target: { value: "not a sha" } });
    expect(screen.getByRole("button", { name: "Start review" })).toBeDisabled();
    fireEvent.change(sha, { target: { value: "ABC1234" } });
    start();
    expect(run).toHaveBeenLastCalledWith({
      kind: "review",
      target: { kind: "commit", sha: "ABC1234", title: "Fix the parser" },
    });

    dialog = await review();
    fireEvent.click(within(dialog).getByLabelText("Custom instructions"));
    const instructions = within(dialog).getByRole("textbox", {
      name: "Review instructions",
    });
    expect(screen.getByRole("button", { name: "Start review" })).toBeDisabled();
    fireEvent.change(instructions, {
      target: { value: "  Check the error paths  " },
    });
    start();
    expect(run).toHaveBeenLastCalledWith({
      kind: "review",
      target: { kind: "custom", instructions: "Check the error paths" },
    });
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("shows the session's model, effort and access in the /status panel", async () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "codex",
          name: "Codex",
          description: "Codex",
          source: "builtIn",
          accountModelDiscovery: false,
          transport: "acp",
          installState: "available",
          bindingId: "binding-a",
          accessModes: [
            {
              id: "readOnly",
              label: "Read only",
              description: "Reads files",
              fallback: true,
            },
          ],
        },
      ],
    });
    negotiate({
      ...livePage(),
      sessionConfig: {
        sageAgentSessionId: "session-a",
        runtimeEpoch: "3",
        configRevision: "1",
        confirmedModel: { id: "gpt-5.5", name: "GPT-5.5" },
        confirmedEffort: "high",
        modes: null,
        configOptions: [],
      },
    });
    render(<Composer />);
    const { field } = await openMenu("/stat");
    fireEvent.keyDown(field, { key: "Enter" });
    const panel = await screen.findByRole("dialog", { name: "Session status" });
    expect(
      within(panel)
        .getAllByRole("term")
        .map((term) => `${term.textContent}: ${term.nextSibling?.textContent}`),
    ).toEqual(["Model: GPT-5.5", "Effort: High", "Access: Read only"]);
    expect(field).toHaveValue("");
    fireEvent.keyDown(panel, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(field).toHaveFocus();
  });
});
