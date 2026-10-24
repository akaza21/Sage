import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage, TurnProjection } from "../../domain/acp";
import { settleLazyModules } from "../../test/settleLazyModules";
import { page, textItem } from "./acp/acpProjection.fixture";
import { ChatTranscript } from "./ChatTranscript";
// The fork history and the fork transcript both import this chunk lazily at
// once; the test module runner can hand the second import a namespace that
// is still evaluating, so the chunk is evaluated up front (a browser import
// always resolves after evaluation).
import "./acp/AcpChatPresentation";

// Only the parent page read is stubbed; every view below is the real one.
const getGoalChatPage = vi.hoisted(() => vi.fn());
vi.mock("../../services/acpGoalDesktop", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  getGoalChatPage,
}));

afterEach(() => {
  cleanup();
  getGoalChatPage.mockReset();
});

function turns(chatId: string, count: number, text: string): TurnProjection[] {
  const base = page("completed").turns[0];
  return Array.from({ length: count }, (_, index) => ({
    ...base,
    id: `${chatId}-turn-${index}`,
    ordinal: index + 1,
    items: [
      {
        ...textItem(),
        id: `${chatId}-item-${index}`,
        chatId,
        turnId: `${chatId}-turn-${index}`,
        payload: { role: "assistant", text: `${text} ${index}` },
      },
    ],
  })) as unknown as TurnProjection[];
}

it("inherited history mounts no second navigator, find or virtual list", async () => {
  // 45 parent Turns would virtualise a full projection (over 40).
  getGoalChatPage.mockResolvedValue({
    ...page("completed"),
    chatId: "chat-parent",
    turns: turns("chat-parent", 45, "Parent answer"),
  });
  useWorkbenchStore.setState({
    project: { id: "project-1", name: "P", roots: [] } as never,
    nativeTransitionEpoch: 3,
    tabs: [{ id: "chat-parent", kind: "chat" }] as never,
  });
  const fork: AcpChatPage = {
    ...page("completed"),
    chatId: "chat-fork" as AcpChatPage["chatId"],
    turns: turns("chat-fork", 2, "Fork answer"),
    forkedFrom: "chat-parent" as AcpChatPage["chatId"],
    forkedFromTurnId: "chat-parent-turn-44" as AcpChatPage["forkedFromTurnId"],
  };
  render(
    <section className="chat-surface" aria-label="Fork pane">
      <ChatTranscript
        transcriptRef={{ current: null }}
        contentRef={{ current: null }}
        endRef={{ current: null }}
        onScroll={vi.fn()}
        projectTransitioning={false}
        useNativeAcp
        page={fork}
        focused
        projectRoots={[]}
        retentionIncomplete={false}
        onOpenResource={vi.fn()}
        hasEarlier={false}
        loadingEarlier={false}
        historyError={null}
        onLoadEarlier={vi.fn()}
        messages={[]}
        developmentSimulator={false}
        showJumpToLatest={false}
        onJumpToLatest={vi.fn()}
      />
    </section>,
  );
  const history = await screen.findByRole("group", {
    name: "History from the parent chat",
  });
  await settleLazyModules();
  expect(await screen.findByText("Parent answer 44")).toBeInTheDocument();
  expect(history.querySelectorAll(".acp-turn")).toHaveLength(20);
  expect(screen.getByText("Earlier history isn't shown.")).toBeInTheDocument();
  expect(screen.getByText("Fork answer 1")).toBeInTheDocument();
  // Inherited answers are read-only: not quotable, not focusable. The fork's
  // own final answers stay selectable.
  expect(history.querySelector("[data-selectable-answer]")).toBeNull();
  expect(history.querySelector('[tabindex="0"]')).toBeNull();
  expect(
    document.querySelectorAll(".message-list [data-selectable-answer]").length,
  ).toBeGreaterThan(0);
  // One navigator, the fork's own, and no virtual list anywhere.
  expect(
    document.querySelectorAll('[aria-label="Turn navigator"]').length,
  ).toBe(1);
  expect(history.querySelector('[aria-label="Turn navigator"]')).toBeNull();
  expect(document.querySelectorAll(".acp-virtual-turns")).toHaveLength(0);
  await settleLazyModules();
  // Ctrl+F opens exactly one find box, the fork chat's.
  const list = document.querySelector(".message-list")!;
  fireEvent.keyDown(list, { key: "f", ctrlKey: true });
  await act(async () => {});
  expect(screen.getAllByLabelText("Search chat")).toHaveLength(1);
});
