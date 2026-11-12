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
  AvailableAcpCommand,
} from "../../services/acpCommandsDesktop";
import { ChatComposerRegion } from "./ChatComposerRegion";
import { pickedCommandFor } from "./slashCommandPicks";

const native = vi.hoisted(() => ({
  lists: [] as AcpCommandList[],
  requests: [] as unknown[],
  hold: null as Promise<AcpCommandList> | null,
  listeners: new Set<(event: AcpCommandsChangedV1) => void>(),
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
const mcp = vi.hoisted(() => ({
  read: vi.fn(),
  reload: vi.fn(),
  signIn: vi.fn(),
}));
vi.mock("../../services/acpMcpDesktop", () => ({ acpMcpDesktop: mcp }));
vi.mock("./acp/useAcpChatActions", async (original) => ({
  ...(await original<object>()),
  currentContext: () => ({
    scope: { projectId: "project-a", chatId: "chat-a" },
    ownershipEpoch: 1,
    commandProjectGeneration: "1",
    page: { projectionRevision: "4" },
  }),
}));

const commands: AvailableAcpCommand[] = [
  {
    name: "compact",
    description: "Compact the conversation",
    inputHint: null,
    source: "harness",
  },
  {
    name: "review",
    description: "Review a pull request",
    inputHint: "PR",
    source: "harness",
  },
  {
    name: "mcp:docs:search",
    description: "Search docs",
    inputHint: "query",
    source: "harness",
  },
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
    expect(names(listbox)).toEqual(["/compact", "/review", "/mcp:docs:search"]);
    expect(
      within(listbox).getByRole("option", {
        name: "/review, Review a pull request, Input: PR",
      }),
    ).toBeInTheDocument();
    expect(field).toHaveAttribute("aria-controls", listbox.id);
    expect(field).toHaveAttribute("aria-expanded", "true");
    expect(field).toHaveAttribute("aria-autocomplete", "list");
    // The status region lives beside the field, not inside the popover.
    await waitFor(() => expect(status()).toHaveTextContent("3 commands"));

    type("/s");
    // Prefix matches lead; other name matches follow.
    await waitFor(() => expect(names(listbox)).toEqual(["/mcp:docs:search"]));
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
    expect(active()).toHaveTextContent("/mcp:docs:search");
    // PageUp and PageDown move a page and stop at the ends.
    fireEvent.keyDown(field, { key: "PageUp" });
    expect(active()).toHaveTextContent("/compact");
    fireEvent.keyDown(field, { key: "PageDown" });
    expect(active()).toHaveTextContent("/mcp:docs:search");
    // Home and End move the caret in the composer.
    expect(fireEvent.keyDown(field, { key: "Home" })).toBe(true);
    expect(fireEvent.keyDown(field, { key: "End" })).toBe(true);
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
          {
            name: "pr-review",
            description: "Inner",
            inputHint: null,
            source: "harness",
          },
          {
            name: "review-all",
            description: "Prefix",
            inputHint: null,
            source: "harness",
          },
          {
            name: "review",
            description: "Exact",
            inputHint: null,
            source: "harness",
          },
        ],
      },
    ];
    render(<Composer />);
    const { listbox } = await openMenu("/review");
    await waitFor(() =>
      expect(names(listbox)).toEqual(["/review", "/review-all", "/pr-review"]),
    );
  });

  it("sections an unfiltered list by source and flattens it while typing", async () => {
    native.lists = [
      {
        revision: 1,
        commands: [
          {
            name: "ship",
            description: "Ship",
            inputHint: null,
            source: "harness",
          },
          {
            name: "lint",
            description: "Lint",
            inputHint: null,
            source: "skill",
          },
          {
            name: "compact",
            description: "Compact",
            inputHint: null,
            source: "builtin",
          },
          {
            name: "learn",
            description: "Learn",
            inputHint: null,
            source: "skill",
          },
        ],
      },
    ];
    render(<Composer />);
    const { field, listbox } = await openMenu("/");
    await waitFor(() =>
      expect(
        within(listbox)
          .getAllByRole("group")
          .map((group) => [group.getAttribute("aria-label"), names(group)]),
      ).toEqual([
        ["Commands", ["/compact"]],
        ["Skills", ["/lint", "/learn"]],
        ["Other commands", ["/ship"]],
      ]),
    );
    type("/l");
    await waitFor(() =>
      expect(names(screen.getByRole("listbox"))).toEqual(["/lint", "/learn"]),
    );
    expect(within(screen.getByRole("listbox")).queryByRole("group")).toBeNull();
    expect(field).toHaveAttribute("aria-expanded", "true");
  });

  it("keeps every source in view by showing the first rows of each section", async () => {
    native.lists = [
      {
        revision: 1,
        commands: [
          ...Array.from({ length: 10 }, (_, index) => ({
            name: `built-${index}`,
            description: "Built-in",
            inputHint: null,
            source: "builtin" as const,
          })),
          {
            name: "lint",
            description: "Lint",
            inputHint: null,
            source: "skill",
          },
        ],
      },
    ];
    render(<Composer />);
    const { listbox } = await openMenu("/");
    await waitFor(() =>
      expect(
        names(within(listbox).getByRole("group", { name: "Skills" })),
      ).toEqual(["/lint"]),
    );
    expect(
      within(listbox)
        .getByRole("group", { name: "Commands" })
        .querySelectorAll("[role='option']"),
    ).toHaveLength(7);
    expect(
      screen.getByText("3 more — keep typing to narrow"),
    ).toBeInTheDocument();
    type("/built-9");
    await waitFor(() =>
      expect(names(screen.getByRole("listbox"))).toEqual(["/built-9"]),
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

  it("opens a panel command in place of sending it, typed or picked", async () => {
    const notion = {
      name: "notion",
      title: "Notion",
      version: null,
      connection: "signInRequired",
      auth: "signedOut",
      tools: 0,
      resources: 0,
      error: null,
      origin: "https://mcp.notion.com",
    };
    const files = {
      ...notion,
      name: "files",
      title: null,
      connection: "connected",
      auth: "none",
      tools: 2,
      origin: null,
    };
    mcp.read.mockResolvedValue({ servers: [files, notion], truncated: false });
    mcp.reload.mockResolvedValue({ servers: [files], truncated: false });
    mcp.signIn.mockResolvedValue(undefined);
    native.lists = [
      {
        revision: 1,
        commands: [
          ...commands,
          {
            name: "status",
            description: "Show the session",
            inputHint: null,
            source: "builtin",
            panel: "status",
          },
          {
            name: "mcp",
            description: "List MCP tools",
            inputHint: null,
            source: "builtin",
            panel: "mcp",
          },
        ],
      },
    ];
    render(<Composer />);
    const { field } = await openMenu("/mcp");
    fireEvent.keyDown(field, { key: "Enter" });
    const panel = await screen.findByRole("dialog", { name: "MCP servers" });
    expect(onSubmit).not.toHaveBeenCalled();
    expect(field).toHaveValue("");
    expect(mcp.read).toHaveBeenCalledWith(
      expect.objectContaining({ sessionId: "session-a", runtimeEpoch: "3" }),
    );
    expect(await within(panel).findByText("Sign-in required")).toBeVisible();
    expect(within(panel).getByText("files")).toBeVisible();
    expect(within(panel).getByText("2 tools")).toBeVisible();
    expect(
      within(panel).getByText("notion · 0 tools · https://mcp.notion.com"),
    ).toBeVisible();
    // Only the server that needs it offers a sign-in.
    expect(
      within(panel).getAllByRole("button", { name: /^Sign in/u }),
    ).toHaveLength(1);
    fireEvent.click(
      within(panel).getByRole("button", { name: "Sign in to Notion" }),
    );
    expect(mcp.signIn).toHaveBeenCalledWith(
      expect.objectContaining({ name: "notion", sessionId: "session-a" }),
    );
    expect(
      await within(panel).findByText(
        "Finish signing in to Notion in your browser.",
      ),
    ).toBeVisible();
    fireEvent.click(within(panel).getByRole("button", { name: "Reload" }));
    await waitFor(() => expect(within(panel).queryByText("Notion")).toBeNull());
    expect(mcp.reload).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(panel, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    // Picked from the menu, `/status` opens its panel the same way.
    await openMenu("/sta");
    fireEvent.click(screen.getByRole("option", { name: /^\/status/u }));
    expect(
      await screen.findByRole("dialog", { name: "Session status" }),
    ).toBeInTheDocument();
    expect(field).toHaveValue("");
    expect(onSubmit).not.toHaveBeenCalled();
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
        commands: [
          {
            name: "init",
            description: "Set up",
            inputHint: null,
            source: "harness",
          },
        ],
      },
    ];
    // Another session's notice is ignored.
    announce("session-b");
    expect(native.requests).toHaveLength(1);
    announce();
    await waitFor(() => expect(names(listbox)).toEqual(["/init"]));
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
        commands: [
          {
            name: "init",
            description: "Set up",
            inputHint: null,
            source: "harness",
          },
        ],
      },
    ];
    announce();
    await waitFor(() => expect(native.requests).toHaveLength(2));
    // The first read finishes last with the older list; it is dropped.
    await act(async () => finishFirst({ revision: 1, commands }));
    const listbox = await screen.findByRole("listbox");
    await waitFor(() => expect(names(listbox)).toEqual(["/init"]));
  });

  it("follows a session switch while the menu is open", async () => {
    render(<Composer />);
    const { listbox } = await openMenu();
    native.lists = [
      {
        revision: 1,
        commands: [
          {
            name: "init",
            description: "Set up",
            inputHint: null,
            source: "harness",
          },
        ],
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
    await waitFor(() => expect(names(listbox)).toEqual(["/init"]));
    expect(native.requests.at(-1)).toMatchObject({ sessionId: "session-b" });
  });

  it("tells waiting, none advertised and no session apart", async () => {
    native.lists = [{ revision: 0, commands: [] }];
    render(<Composer />);
    const field = type("/");
    expect(
      await screen.findByText("Waiting for the session's commands…", {
        selector: TITLE,
      }),
    ).toBeInTheDocument();
    native.lists = [{ revision: 1, commands: [] }];
    announce("session-a", 1);
    expect(
      await screen.findByText("This harness advertises no slash commands.", {
        selector: TITLE,
      }),
    ).toBeInTheDocument();
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
