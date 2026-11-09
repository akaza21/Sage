import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage } from "../../domain/acp";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { settleLazyModules } from "../../test/settleLazyModules";
import { page } from "./acp/acpProjection.fixture";
import { ChatTranscriptFooter } from "./ChatTranscriptFooter";
import { ForkChatDialog, ForkDivider, forkChat } from "./ForkChat";

const getGoalChatPage = vi.hoisted(() => vi.fn());
const fakeCommit = vi.hoisted(() => ({
  next: null as null | (() => Promise<boolean>),
}));
const inheritComposerModel = vi.hoisted(() => vi.fn());
vi.mock("../../app/chatTransitions", async (importOriginal) => {
  const actual = (await importOriginal()) as {
    commitIdentityChatResult: (input: unknown) => Promise<boolean>;
  };
  return {
    ...actual,
    commitIdentityChatResult: (input: unknown) =>
      fakeCommit.next
        ? fakeCommit.next()
        : actual.commitIdentityChatResult(input),
  };
});
vi.mock("./inheritedComposerModel", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  inheritComposerModel,
}));
vi.mock("../../services/acpGoalDesktop", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  getGoalChatPage,
}));
// Only InheritedTurns is loaded from this module here. Spreading the real
// module in would resolve it through an RPC that vi.dynamicImportSettled does
// not track, so settleLazyModules could return before the history renders.
vi.mock("./acp/AcpChatPresentation", () => ({
  InheritedTurns: ({ page }: { page: AcpChatPage }) => (
    <ol>
      {page.turns.map((turn) => (
        <li key={turn.id}>{turn.id}</li>
      ))}
    </ol>
  ),
}));

const initial = useWorkbenchStore.getState();

beforeEach(() => {
  useWorkbenchStore.setState(initial, true);
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  fakeCommit.next = null;
  inheritComposerModel.mockReset();
});

function forkablePage(
  overrides: { fork?: boolean; residency?: "hot" | "cold" } = {},
): AcpChatPage {
  const next = page("completed");
  return {
    ...next,
    sessions: [
      {
        id: "session-1",
        runtimeEpoch: "2",
        residency: overrides.residency ?? "hot",
        capabilities: { nativeSessionFork: overrides.fork ?? true },
      },
    ] as never,
  };
}

async function renderFooter(acpPage: AcpChatPage) {
  render(<ChatTranscriptFooter useNativeAcp page={acpPage} messages={[]} />);
  await settleLazyModules();
}

describe("answer More actions", () => {
  it("offers exactly one item, Branch in new chat, when the session can fork", async () => {
    await renderFooter(forkablePage());
    const actions = screen.getByLabelText("Message actions");
    const buttons = within(actions).getAllByRole("button");
    expect(buttons.map((button) => button.getAttribute("aria-label"))).toEqual([
      "Copy",
      "More actions",
    ]);
    const more = within(actions).getByRole("button", { name: "More actions" });
    expect(more).toHaveAttribute("aria-haspopup", "menu");
    fireEvent.click(more);
    const menu = await screen.findByRole("menu", { name: "Answer actions" });
    const items = within(menu).getAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual([
      "Branch in new chat",
    ]);
    fireEvent.click(items[0]);
    const dialog = await screen.findByRole("dialog", {
      name: "Fork chat from here",
    });
    const options = within(dialog).getAllByRole("menuitem");
    expect(options).toHaveLength(1);
    expect(options[0]).toHaveTextContent(
      "Fork in this workspaceFork from this message in the current workspace",
    );
    expect(within(dialog).queryByText(/worktree/i)).toBeNull();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(
      screen.queryByRole("dialog", { name: "Fork chat from here" }),
    ).toBeNull();
  });

  it.each([
    ["a pointer or scripted click", false, "true"],
    ["Enter on the menu item", true, null],
  ])(
    "pre-fills the dialog option only when Enter or Space opened it, not after %s (s2/f0055)",
    async (_, byKey, fresh) => {
      await renderFooter(forkablePage());
      fireEvent.click(screen.getByRole("button", { name: "More actions" }));
      const item = await screen.findByRole("menuitem", {
        name: "Branch in new chat",
      });
      // A scripted click focuses programmatically, which can match
      // :focus-visible; only a real Enter or Space counts as keyboard.
      const matches = vi
        .spyOn(Element.prototype, "matches")
        .mockReturnValue(true);
      if (byKey) fireEvent.keyDown(item, { key: "Enter" });
      fireEvent.click(item);
      const option = await screen.findByRole("menuitem", {
        name: /Fork in this workspace/,
      });
      matches.mockRestore();
      expect(option.getAttribute("data-fresh")).toBe(fresh);
    },
  );

  it.each([
    ["the capability is false", forkablePage({ fork: false })],
    ["the runtime is parked", forkablePage({ residency: "cold" })],
    [
      "the chat is a temporary side chat",
      { ...forkablePage(), chatId: "side-1" } as AcpChatPage,
    ],
    [
      "the latest Turn did not complete",
      {
        ...forkablePage(),
        turns: [{ ...forkablePage().turns[0], state: "failed" }],
      } as AcpChatPage,
    ],
  ])(
    "still opens, offering only Copy, when %s (no action that can't be done)",
    async (_, acpPage) => {
      const writeText = vi.fn().mockResolvedValue(undefined);
      vi.stubGlobal("navigator", { clipboard: { writeText } });
      await renderFooter(acpPage);
      const more = screen.getByRole("button", { name: "More actions" });
      fireEvent.click(more);
      const menu = await screen.findByRole("menu", { name: "Answer actions" });
      const items = within(menu).getAllByRole("menuitem");
      expect(items.map((item) => item.textContent)).toEqual(["Copy"]);
      await act(async () => {
        fireEvent.click(items[0]);
      });
      expect(writeText).toHaveBeenCalledWith("Normalized answer");
      expect(screen.queryByRole("menu", { name: "Answer actions" })).toBeNull();
      expect(more).toHaveFocus();
      vi.unstubAllGlobals();
    },
  );
});

describe("Fork chat from here", () => {
  it("dims while pending and closes once the fork commits", async () => {
    let finish: (reason: string | null) => void = () => {};
    const fork = vi.fn(
      () => new Promise<string | null>((resolve) => (finish = resolve)),
    );
    const onClose = vi.fn();
    render(
      <ForkChatDialog
        chatId="chat-1"
        turnId="turn-1"
        onClose={onClose}
        fork={fork}
      />,
    );
    const option = screen.getByRole("menuitem", {
      name: /Fork in this workspace/,
    });
    expect(option).toHaveFocus();
    fireEvent.click(option);
    expect(fork).toHaveBeenCalledWith({ chatId: "chat-1", turnId: "turn-1" });
    expect(screen.getByRole("menu")).toHaveAttribute("aria-busy", "true");
    expect(option).toBeDisabled();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
    await act(async () => finish(null));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("opens on the 12% scrim with its first option focused but not pre-filled (Codex s2/f0055)", () => {
    render(
      <ForkChatDialog
        chatId="chat-1"
        turnId="turn-1"
        onClose={() => {}}
        fork={async () => null}
      />,
    );
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveClass("dialog--scrim");
    const option = screen.getByRole("menuitem", {
      name: /Fork in this workspace/,
    });
    expect(option).toHaveFocus();
    expect(option).toHaveAttribute("data-fresh", "true");
    fireEvent.keyDown(option, { key: "ArrowDown" });
    expect(option).not.toHaveAttribute("data-fresh");
  });

  it("gives the committed fork the parent's confirmed model and effort (Codex s2/f0066)", async () => {
    useWorkbenchStore.setState({
      project: { id: "project-1" } as never,
      tabs: [
        { id: "chat-1", kind: "chat", title: "Chat: X", position: 0 },
      ] as never,
      activeTabId: "chat-1",
      projectTransitioning: false,
    });
    const parentConfig = { sageAgentSessionId: "parent-session" };
    acpProjectionStoreFor("chat-1").setState({
      page: { ...page("completed"), sessionConfig: parentConfig } as never,
    });
    fakeCommit.next = async () => {
      useWorkbenchStore.setState({ activeTabId: "fork-1" });
      return true;
    };
    await expect(
      forkChat({ chatId: "chat-1", turnId: "turn-1" }),
    ).resolves.toBe(null);
    expect(inheritComposerModel).toHaveBeenCalledWith("fork-1", parentConfig);
  });

  it("reports a failed fork inline and stays open for another try", async () => {
    const onClose = vi.fn();
    render(
      <ForkChatDialog
        chatId="chat-1"
        turnId="turn-1"
        onClose={onClose}
        fork={async () => "This chat is too long to fork."}
      />,
    );
    await act(async () => {
      fireEvent.click(
        screen.getByRole("menuitem", { name: /Fork in this workspace/ }),
      );
    });
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Couldn't fork this chat. This chat is too long to fork.",
    );
    expect(onClose).not.toHaveBeenCalled();
    expect(
      screen.getByRole("menuitem", { name: /Fork in this workspace/ }),
    ).toBeEnabled();
  });

  it("asks the browser port for the fork and adopts its snapshot", async () => {
    useWorkbenchStore.setState({
      project: { id: "project-1" } as never,
      tabs: [
        { id: "chat-1", kind: "chat", title: "Chat: X", position: 0 },
      ] as never,
      activeTabId: "chat-1",
      nativeTransitionEpoch: 4,
      projectTransitioning: false,
    });
    const port = vi.fn(async () => {
      throw new Error("This chat can't be forked from this answer");
    });
    getWorkbenchPorts().chat.forkChat = port;
    try {
      await expect(
        forkChat({ chatId: "chat-1", turnId: "turn-1" }),
      ).resolves.toBe("This chat can't be forked from this answer");
      expect(port).toHaveBeenCalledWith(
        expect.objectContaining({
          chatId: "chat-1",
          turnId: "turn-1",
          expectedTransitionEpoch: 4,
        }),
      );
      expect(useWorkbenchStore.getState().error).toBeNull();
      expect(useWorkbenchStore.getState().projectTransitioning).toBe(false);
    } finally {
      delete getWorkbenchPorts().chat.forkChat;
    }
  });
});

describe("Continued from chat", () => {
  it("shows the parent's history through the fork Turn, read-only, above the divider (P2)", async () => {
    const parent = page("completed");
    const turns = ["turn-a", "turn-fork", "turn-later"].map((id) => ({
      ...parent.turns[0],
      id,
    }));
    getGoalChatPage.mockResolvedValue({
      ...parent,
      chatId: "chat-parent",
      turns,
    });
    useWorkbenchStore.setState({
      project: { id: "project-1" } as never,
      nativeTransitionEpoch: 6,
      tabs: [{ id: "chat-parent", kind: "chat" }] as never,
    });
    render(<ForkDivider parentChatId="chat-parent" forkTurnId="turn-fork" />);
    const history = await screen.findByRole("group", {
      name: "History from the parent chat",
    });
    await settleLazyModules();
    // Each inherited Turn is inert itself, so its answer row stays usable.
    expect(history).not.toHaveAttribute("inert");
    expect(
      (await within(history).findAllByRole("listitem")).map(
        (item) => item.textContent,
      ),
    ).toEqual(["turn-a", "turn-fork"]);
    expect(getGoalChatPage).toHaveBeenCalledWith(
      expect.objectContaining({
        chatId: "chat-parent",
        projectGeneration: "6",
        cursor: null,
      }),
    );
    // The divider follows the inherited history.
    const divider = screen.getByRole("button", { name: "Continued from chat" });
    expect(
      history.compareDocumentPosition(divider) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("shows no inherited history when the fork Turn is outside the parent's page", async () => {
    getGoalChatPage.mockResolvedValue({ ...page("completed"), chatId: "p" });
    useWorkbenchStore.setState({ project: { id: "project-1" } as never });
    render(<ForkDivider parentChatId="p" forkTurnId="turn-gone" />);
    await act(async () => undefined);
    expect(
      screen.queryByRole("group", { name: "History from the parent chat" }),
    ).toBeNull();
  });

  it("caps inherited Turns, says when earlier history isn't shown, and keeps it through a re-fetch", async () => {
    const parent = page("completed");
    const turns = Array.from({ length: 25 }, (_, index) => ({
      ...parent.turns[0],
      id: `turn-${index}`,
    }));
    let answer!: (value: unknown) => void;
    getGoalChatPage
      .mockResolvedValueOnce({ ...parent, chatId: "p", turns })
      .mockImplementationOnce(
        () => new Promise((resolve) => (answer = resolve)),
      );
    useWorkbenchStore.setState({
      project: { id: "project-1" } as never,
      nativeTransitionEpoch: 1,
    });
    render(<ForkDivider parentChatId="p" forkTurnId="turn-22" />);
    const history = await screen.findByRole("group", {
      name: "History from the parent chat",
    });
    const shown = (await within(history).findAllByRole("listitem")).map(
      (item) => item.textContent,
    );
    expect(shown).toHaveLength(20);
    expect(shown.at(0)).toBe("turn-3");
    expect(shown.at(-1)).toBe("turn-22");
    expect(
      within(history).getByText("Earlier history isn't shown."),
    ).toBeVisible();
    // A Project transition re-fetches; the last render stays meanwhile.
    act(() => useWorkbenchStore.setState({ nativeTransitionEpoch: 2 }));
    expect(getGoalChatPage).toHaveBeenCalledTimes(2);
    expect(
      screen.getByRole("group", { name: "History from the parent chat" }),
    ).toBeVisible();
    await act(async () =>
      answer({
        ...parent,
        chatId: "p",
        nextCursor: null,
        turns: turns.slice(20, 23),
      }),
    );
    expect(
      await within(
        screen.getByRole("group", { name: "History from the parent chat" }),
      ).findAllByRole("listitem"),
    ).toHaveLength(3);
    expect(screen.queryByText("Earlier history isn't shown.")).toBeNull();
  });

  it("returns to an open parent and names an archived one in plain text", () => {
    const activateTab = vi.fn(async () => {});
    useWorkbenchStore.setState({
      tabs: [{ id: "chat-parent", kind: "chat" }] as never,
      activateTab,
    });
    const { rerender } = render(<ForkDivider parentChatId="chat-parent" />);
    fireEvent.click(
      screen.getByRole("button", { name: "Continued from chat" }),
    );
    expect(activateTab).toHaveBeenCalledWith("chat-parent");
    rerender(<ForkDivider parentChatId="chat-archived" />);
    expect(
      screen.queryByRole("button", { name: "Continued from chat" }),
    ).toBeNull();
    expect(
      screen.getByText("Continued from an archived chat"),
    ).toBeInTheDocument();
  });
});
