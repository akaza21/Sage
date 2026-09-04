import { afterEach, describe, expect, it } from "vitest";

import type { AcpChatPage, AcpItem, TurnProjection } from "../../../domain/acp";
import {
  browseSessionForOwner,
  clearBrowseSessions,
} from "../../preview/harnessBrowse";
import { dispatchBrowseFromAcpPage } from "./dispatchBrowse";
import type { AgentArtifactOwner } from "../../../domain/agentArtifact";

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
    sessions: [],
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

function ownerFor(chatId: string): AgentArtifactOwner {
  return {
    projectId: "project-1",
    chatId,
    sessionId: "session-1",
  } as AgentArtifactOwner;
}

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
        kind: "web",
        label: "_sage/browse",
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

  it("respects the two-chat browse cap", () => {
    dispatchBrowseFromAcpPage(
      page([webTool("http://127.0.0.1:5173/")], "chat-a"),
    );
    dispatchBrowseFromAcpPage(
      page([webTool("http://127.0.0.1:4173/")], "chat-b"),
    );
    dispatchBrowseFromAcpPage(
      page([webTool("http://127.0.0.1:3000/")], "chat-c"),
    );
    expect(browseSessionForOwner(ownerFor("chat-a"))).toBeNull();
    expect(browseSessionForOwner(ownerFor("chat-c"))?.href).toBe(
      "http://127.0.0.1:3000/",
    );
  });
});
