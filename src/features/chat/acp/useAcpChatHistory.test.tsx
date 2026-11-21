import {
  act,
  cleanup,
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useAcpProjectionStore } from "../../../app/acpProjectionStore";
import { useWorkbenchStore } from "../../../app/store";
import type { AcpChatPage } from "../../../domain/acp";
import { decodeAcpGoal } from "../../../domain/acpGoal";
import { acpDesktop } from "../../../services/acpDesktop";
import { useAcpChatHistory } from "./useAcpChatHistory";
import { useAcpChatProjection } from "./useAcpChatProjection";
import { ChatTranscript } from "../ChatTranscript";

const page = {
  projectId: "project-1",
  chatId: "chat-1",
  title: "Durable chat",
  projectionRevision: "4",
  currentSessionId: null,
  sessions: [],
  turns: [],
  nextCursor: "cursor-1",
} as unknown as AcpChatPage;

beforeEach(() => {
  useAcpProjectionStore.getState().clear();
  useWorkbenchStore.setState({
    project: { id: "project-1" } as never,
    projectGeneration: 7,
    nativeTransitionEpoch: 3,
    projectTransitioning: false,
    activeTabId: "chat-1",
    tabs: [{ id: "chat-1", kind: "chat" }] as never,
  });
  const scope = {
    projectId: page.projectId,
    projectGeneration: "7" as never,
    chatId: page.chatId,
  };
  useAcpProjectionStore.getState().activateScope(scope);
  useAcpProjectionStore.getState().applyAuthoritativePage(scope, page);
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useAcpProjectionStore.getState().clear();
});

describe("useAcpChatHistory", () => {
  it("retries a failed first page through the real projection reload and displays the recovered history", async () => {
    useAcpProjectionStore.getState().clear();
    vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockResolvedValue(
      () => {},
    );
    vi.spyOn(acpDesktop, "subscribeSessionConfigChanges").mockResolvedValue(
      () => {},
    );
    vi.spyOn(acpDesktop, "getChatPage")
      .mockRejectedValueOnce(new Error("Store unavailable"))
      .mockResolvedValue({ ...page, nextCursor: null });
    function Transcript() {
      const projection = useAcpChatProjection();
      const history = useAcpChatHistory();
      return (
        <ChatTranscript
          transcriptRef={{ current: null }}
          contentRef={{ current: null }}
          endRef={{ current: null }}
          onScroll={() => {}}
          projectTransitioning={false}
          useNativeAcp
          page={projection}
          focused
          projectRoots={[]}
          retentionIncomplete={false}
          onOpenResource={() => {}}
          {...history}
          messages={[]}
          developmentSimulator={false}
          showJumpToLatest={false}
          onJumpToLatest={() => {}}
        />
      );
    }
    render(<Transcript />);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Chat history could not be loaded.",
    );
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    expect(
      await screen.findByRole("region", { name: "Agent chat Durable chat" }),
    ).toBeVisible();
  });
  it("reports bootstrap loading separately from earlier-history loading", () => {
    const { result } = renderHook(() => useAcpChatHistory());
    expect(result.current.historyLoading).toBe(false);
    let intent: ReturnType<typeof useAcpProjectionStore.getState>["activeLoad"];
    act(() => {
      intent = useAcpProjectionStore.getState().beginPageLoad(null);
    });
    expect(result.current.historyLoading).toBe(true);
    expect(result.current.loadingEarlier).toBe(false);
    act(() => {
      useAcpProjectionStore.getState().completePageLoad(intent!, page);
    });
    expect(result.current.historyLoading).toBe(false);
    act(() => {
      intent = useAcpProjectionStore.getState().beginPageLoad(page.nextCursor);
    });
    expect(result.current.historyLoading).toBe(true);
    expect(result.current.loadingEarlier).toBe(true);
    act(() => {
      useAcpProjectionStore.getState().failPageLoad(intent!, "Unavailable");
    });
    expect(result.current.historyLoading).toBe(false);
    expect(result.current.historyError).toBe("Unavailable");
  });

  it("reports a failed first page so the chat stops waiting for it", () => {
    const store = useAcpProjectionStore.getState();
    store.clear();
    store.activateScope({
      projectId: page.projectId,
      projectGeneration: "7" as never,
      chatId: page.chatId,
    });
    const { result } = renderHook(() => useAcpChatHistory());
    let intent: ReturnType<typeof useAcpProjectionStore.getState>["activeLoad"];
    act(() => {
      intent = useAcpProjectionStore.getState().beginPageLoad(null);
    });
    expect(result.current.firstPageError).toBeNull();
    act(() => {
      useAcpProjectionStore.getState().failPageLoad(intent!, "Unavailable");
    });
    expect(result.current.historyLoading).toBe(false);
    expect(result.current.firstPageError).toBe("Unavailable");
    expect(result.current.historyError).toBeNull();
  });

  it("loads only the user-selected bounded continuation cursor", async () => {
    const load = vi.spyOn(acpDesktop, "getChatPage").mockResolvedValue({
      ...page,
      nextCursor: null,
    });
    const { result } = renderHook(() => useAcpChatHistory());

    act(() => result.current.onLoadEarlier());
    await waitFor(() => expect(load).toHaveBeenCalledTimes(1));
    expect(load).toHaveBeenCalledWith(
      {
        projectId: "project-1",
        projectGeneration: "3",
        chatId: "chat-1",
        cursor: "cursor-1",
      },
      decodeAcpGoal,
    );
    await waitFor(() => expect(result.current.hasEarlier).toBe(false));
  });

  it("expects a page only for a resolved chat in a settled project", () => {
    const active = renderHook(() => useAcpChatHistory());
    expect(active.result.current.pageExpected).toBe(true);
    const bound = renderHook(() => useAcpChatHistory("chat-2"));
    expect(bound.result.current.pageExpected).toBe(true);
    act(() => {
      useWorkbenchStore.setState({
        activeTabId: "file-1",
        tabs: [{ id: "file-1", kind: "file" }] as never,
      });
    });
    expect(active.result.current.pageExpected).toBe(false);
    expect(bound.result.current.pageExpected).toBe(true);
    act(() => useWorkbenchStore.setState({ projectTransitioning: true }));
    expect(bound.result.current.pageExpected).toBe(false);
    act(() =>
      useWorkbenchStore.setState({
        projectTransitioning: false,
        project: null,
      }),
    );
    expect(bound.result.current.pageExpected).toBe(false);
  });
});
