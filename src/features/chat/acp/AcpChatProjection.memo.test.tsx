import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import { createAcpProjectionStore } from "../../../app/acpProjectionStore";
import {
  clearBrowseSessions,
  setHarnessBrowseOwner,
} from "../../preview/harnessBrowse";
import type { HarnessBrowseOwner } from "../../preview/harnessBrowse";
import { page, textItem } from "./acpProjection.fixture";

const browseOwner = {
  projectId: "project-1",
  projectGeneration: "1",
  transitionEpoch: 1,
  chatId: "chat-1",
  sessionId: "session-1",
  runtimeEpoch: "2",
} as HarnessBrowseOwner;

beforeEach(() => setHarnessBrowseOwner(browseOwner, [browseOwner.chatId]));

afterEach(() => {
  cleanup();
  clearBrowseSessions();
});

describe("AcpChatProjection", () => {
  it("replaces only a revised immutable Turn row", async () => {
    const chatMarkdown = vi.fn(({ text }: { text: string }) => (
      <span>{text}</span>
    ));
    const turnEditSummary = vi.fn(() => null);
    const turnHeader = vi.fn(() => null);
    vi.resetModules();
    vi.doMock("./ChatMarkdown", () => ({ ChatMarkdown: chatMarkdown }));
    vi.doMock("./TurnHeader", () => ({ TurnHeader: turnHeader }));
    vi.doMock("./TurnEditSummary", () => ({
      TurnEditSummary: turnEditSummary,
    }));
    const { AcpChatProjection: MemoProjection } =
      await import("./AcpChatProjection");
    const initial = page("completed");
    initial.turns = Array.from({ length: 14 }, (_, index) => {
      const turn = structuredClone(initial.turns[0]);
      turn.id = `turn-${index + 1}` as never;
      turn.ordinal = index + 1;
      turn.revision = "1" as never;
      turn.items = [
        {
          ...textItem(),
          id: `item-${index + 1}` as never,
          turnId: turn.id,
          revision: "1" as never,
          payload: { role: "assistant", text: `Retained row ${index + 1}` },
        } as AcpItem,
        {
          ...textItem(),
          id: `retained-item-${index + 1}` as never,
          turnId: turn.id,
          revision: "1" as never,
          payload: {
            role: "assistant",
            text: `Unchanged sibling ${index + 1}`,
          },
        } as AcpItem,
      ];
      return turn;
    });
    const store = createAcpProjectionStore();
    const scope = {
      projectId: initial.projectId,
      chatId: initial.chatId,
      projectGeneration: "1" as never,
    };
    store.getState().activateScope(scope);
    expect(store.getState().applyAuthoritativePage(scope, initial)).toBe(
      "accepted",
    );
    const { container, rerender } = render(
      <MemoProjection
        page={store.getState().page!}
        active
        editSummaryContainer={document.body}
      />,
    );
    await screen.findByText("Unchanged sibling 14");
    for (const work of container.querySelectorAll<HTMLDetailsElement>(
      "details.chat-transcript-work",
    )) {
      expect(work).not.toHaveAttribute("open");
      work.open = true;
      fireEvent(work, new Event("toggle"));
    }
    await screen.findByText("Retained row 14");
    chatMarkdown.mockClear();
    turnHeader.mockClear();
    turnEditSummary.mockClear();
    const before = Array.from(
      container.querySelectorAll<HTMLElement>(".acp-turn"),
    );
    const revised = structuredClone(initial);
    const changed = revised.turns[6]!;
    changed.revision = "2" as never;
    changed.items = changed.items.map((item, index) =>
      index === 0
        ? ({
            ...item,
            revision: "2" as never,
            payload: { role: "assistant", text: "Revised row 7" },
          } as AcpItem)
        : item,
    );
    revised.projectionRevision = String(
      Number(initial.projectionRevision) + 1,
    ) as never;
    expect(store.getState().applyAuthoritativePage(scope, revised)).toBe(
      "accepted",
    );
    rerender(
      <MemoProjection
        page={store.getState().page!}
        active
        editSummaryContainer={document.body}
      />,
    );
    await screen.findByText("Revised row 7");
    const after = Array.from(
      container.querySelectorAll<HTMLElement>(".acp-turn"),
    );
    expect(chatMarkdown).toHaveBeenCalledOnce();
    expect(turnHeader).toHaveBeenCalledOnce();
    expect(turnEditSummary).toHaveBeenCalledOnce();
    expect(after[0]).toBe(before[0]);
    expect(after[6]).toBe(before[6]);
    expect(after[6]).toHaveTextContent("Revised row 7");
    expect(after[13]).toBe(before[13]);
    vi.doUnmock("./ChatMarkdown");
    vi.doUnmock("./TurnHeader");
    vi.doUnmock("./TurnEditSummary");
    vi.resetModules();
  });
});
