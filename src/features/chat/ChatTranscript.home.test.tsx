import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import type { AcpChatPage } from "../../domain/acp";
import { useWorkbenchStore } from "../../app/store";
import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
  pinAcpProjectionIds,
} from "../../app/acpProjectionRegistry";
import { useAcpProjectionStore } from "../../app/acpProjectionStore";
import { settleLazyModules } from "../../test/settleLazyModules";
import { page } from "./acp/acpProjection.fixture";
import { ChatTranscript } from "./ChatTranscript";

const initialState = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
  useAcpProjectionStore.getState().clear();
  clearAcpProjectionRegistry();
});

function emptyPage(): AcpChatPage {
  return {
    ...page("completed"),
    sessions: [],
    currentSessionId: null,
    turns: [],
  };
}

function transcriptProps() {
  return {
    transcriptRef: { current: null },
    contentRef: { current: null },
    endRef: { current: null },
    onScroll: vi.fn(),
    projectTransitioning: false,
    project: { id: "project-1", name: "Sage" },
    useNativeAcp: true,
    page: emptyPage() as AcpChatPage | null,
    focused: true,
    projectRoots: [],
    retentionIncomplete: false,
    onOpenResource: vi.fn(),
    hasEarlier: false,
    historyLoading: false,
    loadingEarlier: false,
    historyError: null as string | null,
    onLoadEarlier: vi.fn(),
    messages: [],
    developmentSimulator: false,
    showJumpToLatest: false,
    onJumpToLatest: vi.fn(),
  };
}

it("replaces the established empty transcript with the real project home without inventing a turn", async () => {
  const { container } = render(<ChatTranscript {...transcriptProps()} />);
  await settleLazyModules();
  expect(
    screen.getByRole("heading", { name: "What should we work on in Sage?" }),
  ).toBeVisible();
  expect(screen.getByText("Sage")).toBeVisible();
  expect(screen.queryByText("No turns in this chat yet.")).toBeNull();
  expect(container.querySelector(".acp-turn")).toBeNull();
  expect(container.querySelector(".chat-transcript-actions")).toBeNull();
});

it.each([
  ["initial history load", { page: null }],
  ["empty projection refresh", { historyLoading: true }],
  ["earlier history load", { loadingEarlier: true }],
  ["history failure", { historyError: "History unavailable" }],
  ["unloaded history", { hasEarlier: true }],
  [
    "continuation cursor",
    { page: { ...emptyPage(), nextCursor: "cursor" as never } },
  ],
  ["discarded history", { retentionIncomplete: true }],
  ["project transition", { projectTransitioning: true }],
  ["stale project page", { project: { id: "project-2", name: "Other" } }],
] as const)(
  "does not mistake %s for a new chat",
  async (_scenario, overrides) => {
    const { container } = render(
      <ChatTranscript {...transcriptProps()} {...overrides} />,
    );
    await settleLazyModules();
    expect(screen.queryByRole("region", { name: "New chat" })).toBeNull();
    expect(container.querySelector(".message-list--home")).toBeNull();
  },
);

it.each(["running", "completed", "cancelled", "failed"] as const)(
  "switches from home to %s history and back only after empty history settles",
  async (state) => {
    const props = transcriptProps();
    const { rerender } = render(<ChatTranscript {...props} />);
    expect(screen.getByRole("region", { name: "New chat" })).toBeVisible();
    rerender(<ChatTranscript {...props} page={page(state)} />);
    await settleLazyModules();
    expect(screen.queryByRole("region", { name: "New chat" })).toBeNull();
    expect(screen.getByText("Normalized answer")).toBeVisible();
    rerender(<ChatTranscript {...props} historyLoading />);
    expect(screen.queryByRole("region", { name: "New chat" })).toBeNull();
    rerender(<ChatTranscript {...props} />);
    expect(screen.getByRole("region", { name: "New chat" })).toBeVisible();
  },
);

it("shows an authoritative loading or failure notice before an empty page exists", async () => {
  const props = transcriptProps();
  const { rerender } = render(
    <ChatTranscript {...props} page={null} historyLoading />,
  );
  expect(screen.getByText("Loading chat history…")).toHaveAttribute(
    "role",
    "status",
  );
  rerender(
    <ChatTranscript
      {...props}
      page={null}
      historyError="History unavailable"
    />,
  );
  expect(screen.getByRole("alert")).toHaveTextContent(
    "Chat history could not be loaded: History unavailable",
  );
  expect(screen.queryByRole("region", { name: "New chat" })).toBeNull();
  await settleLazyModules();
});

it("retains earlier-history retry and discarded-history disclosure after an empty page's continuation fails", async () => {
  const props = transcriptProps();
  render(
    <ChatTranscript
      {...props}
      page={{ ...emptyPage(), nextCursor: "cursor" as never }}
      hasEarlier
      retentionIncomplete
      historyError="History unavailable"
    />,
  );
  await settleLazyModules();
  expect(screen.queryByRole("region", { name: "New chat" })).toBeNull();
  expect(screen.getByRole("alert")).toHaveTextContent("History unavailable");
  expect(screen.getByText(/Some Chat history was discarded/)).toBeVisible();
  const retry = screen.getByRole("button", { name: "Load earlier history" });
  expect(retry).toBeEnabled();
  fireEvent.click(retry);
  expect(props.onLoadEarlier).toHaveBeenCalledOnce();
});

it("switches projects through the existing keyboard-accessible picker and discards the old home", async () => {
  const switchProject = vi.fn().mockResolvedValue(undefined);
  useWorkbenchStore.setState({
    project: { id: "project-1", name: "Sage", roots: [] },
    projects: [
      { id: "project-1", name: "Sage", rootCount: 0, lastOpenedAt: 1 },
      { id: "project-2", name: "Other", rootCount: 0, lastOpenedAt: 1 },
    ],
    switchProject,
  });
  const props = transcriptProps();
  const { rerender } = render(<ChatTranscript {...props} />);
  const chip = screen.getByRole("button", { name: "Project" });
  chip.focus();
  fireEvent.click(chip);
  const picker = screen.getByRole("combobox", { name: "Project" });
  fireEvent.keyDown(picker, { key: "End" });
  fireEvent.keyDown(picker, { key: "Enter" });
  expect(switchProject).toHaveBeenCalledExactlyOnceWith("project-2");
  rerender(<ChatTranscript {...props} projectTransitioning />);
  expect(screen.queryByRole("button", { name: "Project" })).toBeNull();
  useWorkbenchStore.setState({
    project: { id: "project-2", name: "Other", roots: [] },
  });
  rerender(
    <ChatTranscript
      {...props}
      project={{ id: "project-2", name: "Other" }}
      page={{ ...emptyPage(), projectId: "project-2" as never }}
    />,
  );
  expect(
    screen.getByRole("heading", { name: "What should we work on in Other?" }),
  ).toBeVisible();
  expect(screen.queryByText("Sage")).toBeNull();
  await settleLazyModules();
});

it("keeps an empty unfocused second pane isolated from a live first pane", async () => {
  const props = transcriptProps();
  const { rerender, container } = render(
    <>
      <ChatTranscript {...props} page={page("running")} />
      <ChatTranscript
        {...props}
        page={{ ...emptyPage(), chatId: "chat-2" as never }}
        focused={false}
      />
    </>,
  );
  await settleLazyModules();
  const panes = container.querySelectorAll(".message-list");
  expect(
    within(panes[0] as HTMLElement).queryByRole("region", { name: "New chat" }),
  ).toBeNull();
  expect(
    within(panes[1] as HTMLElement).getByRole("region", { name: "New chat" }),
  ).toBeVisible();
  expect(container.querySelectorAll("[data-acp-announcement]")).toHaveLength(2);
  rerender(
    <>
      <ChatTranscript {...props} page={page("completed")} />
      <ChatTranscript
        {...props}
        page={{ ...page("failed"), chatId: "chat-2" as never }}
        focused={false}
      />
    </>,
  );
  await settleLazyModules();
  expect(screen.queryByRole("region", { name: "New chat" })).toBeNull();
  expect(screen.getAllByText("Normalized answer")).toHaveLength(2);
});

it.each([
  ["completed", "Completed"],
  ["failed", "Failed"],
] as const)(
  "announces a first turn arriving already %s from the empty home exactly once",
  async (state, label) => {
    const props = transcriptProps();
    const { container, rerender } = render(<ChatTranscript {...props} />);
    const region = container.querySelector('[data-acp-announcement="polite"]')!;
    const announcements: string[] = [];
    const observer = new MutationObserver(() => {
      const name = region.getAttribute("aria-label");
      if (name) announcements.push(name);
    });
    observer.observe(region, {
      attributes: true,
      attributeFilter: ["aria-label"],
    });
    try {
      const settled = { ...page(state), projectionRevision: "13" as never };
      rerender(<ChatTranscript {...props} page={settled} />);
      await waitFor(() => expect(announcements).toEqual([label]));
      rerender(<ChatTranscript {...props} page={{ ...settled }} />);
      await settleLazyModules();
      expect(announcements).toEqual([label]);
    } finally {
      observer.disconnect();
    }
  },
);

it("keeps first-turn announcements continuous through an empty history refresh", async () => {
  const props = transcriptProps();
  const { container, rerender } = render(<ChatTranscript {...props} />);
  const region = container.querySelector('[data-acp-announcement="polite"]')!;
  rerender(<ChatTranscript {...props} historyLoading />);
  expect(screen.queryByRole("region", { name: "New chat" })).toBeNull();
  expect(screen.getByText("Loading chat history…")).toBeVisible();
  rerender(
    <ChatTranscript
      {...props}
      page={{ ...page("completed"), projectionRevision: "13" as never }}
    />,
  );
  await waitFor(() => expect(region).toHaveAccessibleName("Completed"));
});

it("uses the authoritative project name even when the project list still has an older label", () => {
  useWorkbenchStore.setState({
    project: { id: "project-1", name: "Renamed", roots: [] },
    projects: [
      { id: "project-1", name: "Old name", rootCount: 0, lastOpenedAt: 1 },
    ],
  });
  render(
    <ChatTranscript
      {...transcriptProps()}
      project={{ id: "project-1", name: "Renamed" }}
    />,
  );
  expect(screen.getByRole("button", { name: "Project" })).toHaveTextContent(
    "Renamed",
  );
  expect(screen.queryByText("Old name")).toBeNull();
});

it.each([
  ["first-prompt dispatch", { sending: true }],
  ["another pane's dispatch", { chatUiById: { "chat-2": { sending: true } } }],
  [
    "another pane's mode negotiation",
    { chatUiById: { "chat-2": { modeChanging: true } } },
  ],
] as const)(
  "disables the home project picker during %s and reenables it after settlement",
  (_scenario, busy) => {
    const switchProject = vi.fn();
    useWorkbenchStore.setState({
      project: { id: "project-1", name: "Sage", roots: [] },
      projects: [
        { id: "project-1", name: "Sage", rootCount: 0, lastOpenedAt: 1 },
        { id: "project-2", name: "Other", rootCount: 0, lastOpenedAt: 1 },
      ],
      switchProject,
      ...(busy as Partial<ReturnType<typeof useWorkbenchStore.getState>>),
    });
    render(<ChatTranscript {...transcriptProps()} />);
    const chip = screen.getByRole("button", { name: "Project" });
    expect(chip).toBeDisabled();
    fireEvent.click(chip);
    expect(screen.queryByRole("combobox", { name: "Project" })).toBeNull();
    expect(switchProject).not.toHaveBeenCalled();
    act(() => useWorkbenchStore.setState({ sending: false, chatUiById: {} }));
    expect(chip).toBeEnabled();
  },
);

it.each(["primary", "focused", "unfocused"] as const)(
  "blocks an empty second pane's project action until the %s chat's active turn settles",
  (owner) => {
    useWorkbenchStore.setState({
      project: { id: "project-1", name: "Sage", roots: [] },
      projectGeneration: 1,
      projects: [
        { id: "project-1", name: "Sage", rootCount: 0, lastOpenedAt: 1 },
      ],
      activeTabId: owner === "unfocused" ? "chat-2" : "chat-1",
      tabs: [
        { id: "chat-1", kind: "chat" },
        { id: "chat-2", kind: "chat" },
      ] as never,
    });
    const store =
      owner === "primary"
        ? useAcpProjectionStore
        : acpProjectionStoreFor("chat-1");
    pinAcpProjectionIds(["chat-1", "chat-2"]);
    const live = page("running");
    const scope = {
      projectId: live.projectId,
      chatId: live.chatId,
      projectGeneration: "1" as never,
    };
    store.getState().activateScope(scope);
    store.getState().applyAuthoritativePage(scope, live);
    render(
      <ChatTranscript
        {...transcriptProps()}
        page={{ ...emptyPage(), chatId: "chat-2" as never }}
        focused={false}
      />,
    );
    const chip = screen.getByRole("button", { name: "Project" });
    expect(chip).toBeDisabled();
    act(() =>
      store.getState().applyAuthoritativePage(scope, {
        ...page("completed"),
        projectionRevision: "13" as never,
      }),
    );
    expect(chip).toBeEnabled();
  },
);
