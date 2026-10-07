import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type { AcpChatPage, AcpItem, TurnProjection } from "../../../domain/acp";
import {
  browseSessionForOwner,
  clearBrowseSessions,
  setHarnessBrowseOwner,
  type HarnessBrowseOwner,
} from "../../preview/harnessBrowse";
import { dispatchBrowseFromAcpPage } from "./dispatchBrowse";

afterEach(() => {
  clearBrowseSessions();
});

function page(items: AcpItem[], chatId = "chat-1"): AcpChatPage {
  return {
    projectId: "project-1",
    chatId,
    title: "Browse chat",
    projectionRevision: "1",
    currentSessionId: "session-1",
    sessions: [{ id: "session-1", runtimeEpoch: "1" }] as never,
    nextCursor: null,
    turns: [
      {
        id: "turn-1",
        sessionId: "session-1",
        runtimeEpoch: "1",
        ordinal: 1,
        state: "running",
        revision: "1",
        createdAt: 1,
        updatedAt: 1,
        terminalAt: null,
        items: items.map((item) => ({ ...item, chatId }) as AcpItem),
        activities: [],
      } as unknown as TurnProjection,
    ],
  } as unknown as AcpChatPage;
}

function ownerFor(chatId: string): HarnessBrowseOwner {
  return {
    projectId: "project-1",
    projectGeneration: "1",
    transitionEpoch: 1,
    chatId,
    sessionId: "session-1",
    runtimeEpoch: "1",
  } as HarnessBrowseOwner;
}

beforeEach(() =>
  setHarnessBrowseOwner(ownerFor("chat-1"), ["chat-1", "chat-a"]),
);

function webTool(
  summary: string | null,
  overrides: Partial<AcpItem> = {},
): AcpItem {
  return {
    id: "item-web",
    projectId: "project-1",
    chatId: "chat-1",
    sessionId: "session-1",
    turnId: "turn-1",
    runtimeEpoch: "1",
    ordinal: 1,
    origin: "primary",
    activityId: null,
    lifecycle: "streaming",
    revision: "1",
    createdAt: 1,
    updatedAt: 1,
    terminalAt: null,
    kind: "toolCall",
    toolState: "inProgress",
    payload: {
      presentation: {
        kind: "fetch",
        label: "_sage/browse",
        url: null,
        summary,
      },
      inputSummary: null,
    },
    ...overrides,
  } as AcpItem;
}

describe("ACP curated browse dispatch", () => {
  it("opens an admitted URL from a harness tool item without a click", () => {
    dispatchBrowseFromAcpPage(
      page([webTool("Opened http://127.0.0.1:5173/app")]),
    );
    expect(browseSessionForOwner(ownerFor("chat-1"))?.href).toBe(
      "http://127.0.0.1:5173/app",
    );
  });

  it("keeps rejected URLs closed", () => {
    dispatchBrowseFromAcpPage(page([webTool("Opened http://127.0.0.1:5173/")]));
    dispatchBrowseFromAcpPage(page([webTool("see https://example.com/docs")]));
    expect(browseSessionForOwner(ownerFor("chat-1"))?.href).toBe(
      "http://127.0.0.1:5173/",
    );
  });

  it("scopes browse to the current chat and session", () => {
    setHarnessBrowseOwner(ownerFor("chat-a"), ["chat-a"]);
    dispatchBrowseFromAcpPage(
      page([webTool("http://127.0.0.1:4173/a")], "chat-a"),
    );
    const foreign = page([webTool("http://127.0.0.1:3000/b")], "chat-a");
    foreign.turns[0].items[0].sessionId =
      "session-other" as AcpItem["sessionId"];
    dispatchBrowseFromAcpPage(foreign);
    expect(browseSessionForOwner(ownerFor("chat-a"))?.href).toBe(
      "http://127.0.0.1:4173/a",
    );
    expect(browseSessionForOwner(ownerFor("chat-b"))).toBeNull();
  });

  it("rejects a historical runtime item from the current session", () => {
    const current = page([webTool("http://127.0.0.1:4173/current")]);
    current.turns[0].items[0].runtimeEpoch = "0" as AcpItem["runtimeEpoch"];
    dispatchBrowseFromAcpPage(current);
    expect(browseSessionForOwner(ownerFor("chat-1"))).toBeNull();

    const staleTurn = page([webTool("http://127.0.0.1:4173/stale-turn")]);
    staleTurn.turns[0].runtimeEpoch = "0" as TurnProjection["runtimeEpoch"];
    dispatchBrowseFromAcpPage(staleTurn);
    expect(browseSessionForOwner(ownerFor("chat-1"))).toBeNull();
  });

  it("keeps browse sessions isolated across Chats", () => {
    setHarnessBrowseOwner(ownerFor("chat-a"), ["chat-a", "chat-b"]);
    dispatchBrowseFromAcpPage(
      page([webTool("http://127.0.0.1:5173/")], "chat-a"),
    );
    setHarnessBrowseOwner(ownerFor("chat-b"), ["chat-a", "chat-b"]);
    dispatchBrowseFromAcpPage(
      page([webTool("http://127.0.0.1:4173/")], "chat-b"),
    );
    dispatchBrowseFromAcpPage(
      page([webTool("http://127.0.0.1:3000/")], "chat-c"),
    );
    expect(browseSessionForOwner(ownerFor("chat-a"))?.href).toBe(
      "http://127.0.0.1:5173/",
    );
    expect(browseSessionForOwner(ownerFor("chat-c"))).toBeNull();
  });
});
