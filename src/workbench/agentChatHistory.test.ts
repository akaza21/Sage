import { afterEach, describe, expect, it } from "vitest";

import {
  MAX_AGENT_CHAT_PLACES,
  goBackInAgentChats,
  goForwardInAgentChats,
  noteAgentChatVisit,
  resetAgentChatHistory,
  useAgentChatHistory,
  type AgentChatPlace,
} from "./agentChatHistory";

/** A window whose open call shows the chat unless it is listed as gone. */
function windowShowing(start: AgentChatPlace, gone = new Set<string>()) {
  let shown: AgentChatPlace | null = start;
  const opened: string[] = [];
  return {
    opened,
    open: async (projectId: string, chatId: string) => {
      opened.push(`${projectId}/${chatId}`);
      if (gone.has(chatId)) return;
      // A project opens on its own active chat before the target.
      noteAgentChatVisit({ projectId, chatId: "project-landing" });
      shown = { projectId, chatId };
    },
    shown: () => shown,
  };
}

const place = (projectId: string, chatId: string) => ({ projectId, chatId });

afterEach(resetAgentChatHistory);

describe("agent chat history", () => {
  it("goes back and forward through the chats shown, across projects", async () => {
    noteAgentChatVisit(place("sage", "a"));
    noteAgentChatVisit(place("angle", "b"));
    noteAgentChatVisit(place("sage", "c"));
    const view = windowShowing(place("sage", "c"));

    await goBackInAgentChats(view.open, view.shown);
    await goBackInAgentChats(view.open, view.shown);
    expect(view.opened).toEqual(["angle/b", "sage/a"]);
    expect(useAgentChatHistory.getState()).toEqual({
      canGoBack: false,
      canGoForward: true,
    });

    await goForwardInAgentChats(view.open, view.shown);
    expect(view.opened.at(-1)).toBe("angle/b");
    expect(useAgentChatHistory.getState()).toEqual({
      canGoBack: true,
      canGoForward: true,
    });
  });

  it("drops the forward trail when a new chat is opened after going back", async () => {
    noteAgentChatVisit(place("sage", "a"));
    noteAgentChatVisit(place("sage", "b"));
    const view = windowShowing(place("sage", "b"));
    await goBackInAgentChats(view.open, view.shown);
    noteAgentChatVisit(place("sage", "c"));
    expect(useAgentChatHistory.getState().canGoForward).toBe(false);
    await goBackInAgentChats(view.open, view.shown);
    expect(view.opened).toEqual(["sage/a", "sage/a"]);
  });

  it("drops a chat that can no longer be opened and stays put", async () => {
    noteAgentChatVisit(place("sage", "archived"));
    noteAgentChatVisit(place("sage", "b"));
    const view = windowShowing(place("sage", "b"), new Set(["archived"]));
    await goBackInAgentChats(view.open, view.shown);
    expect(view.shown()).toEqual(place("sage", "b"));
    expect(useAgentChatHistory.getState()).toEqual({
      canGoBack: false,
      canGoForward: false,
    });
  });

  it("does not note a chat seen twice in a row, and keeps a bounded trail", async () => {
    for (let index = 0; index <= MAX_AGENT_CHAT_PLACES + 5; index += 1) {
      noteAgentChatVisit(place("sage", `chat-${index}`));
      noteAgentChatVisit(place("sage", `chat-${index}`));
    }
    const view = windowShowing(place("sage", "last"));
    for (let step = 0; step < MAX_AGENT_CHAT_PLACES + 5; step += 1) {
      await goBackInAgentChats(view.open, view.shown);
    }
    expect(view.opened).toHaveLength(MAX_AGENT_CHAT_PLACES);
    expect(view.opened.at(-1)).toBe("sage/chat-5");
  });
});
