import { afterEach, describe, expect, it } from "vitest";

import {
  MAX_AGENT_CHAT_PLACES,
  goBackInAgentChats,
  goForwardInAgentChats,
  noteAgentChatVisit,
  resetAgentChatHistory,
  useAgentChatHistory,
  type AgentChatPlace,
  type AgentChatTrip,
} from "./agentChatHistory";

const place = (projectId: string, chatId: string) => ({ projectId, chatId });
const key = ({ projectId, chatId }: AgentChatPlace) => `${projectId}/${chatId}`;

/**
 * A window that opens what it is asked to unless told otherwise. A project
 * opens on its own active chat first, which the history must not record.
 */
function fakeWindow(outcomes: Record<string, AgentChatTrip> = {}) {
  const opened: string[] = [];
  return {
    opened,
    open: async (target: AgentChatPlace): Promise<AgentChatTrip> => {
      opened.push(key(target));
      noteAgentChatVisit(place(target.projectId, "project-landing"));
      return outcomes[key(target)] ?? { kind: "opened" };
    },
  };
}

const state = () => useAgentChatHistory.getState();

afterEach(resetAgentChatHistory);

describe("agent chat history", () => {
  it("goes back and forward through the chats shown, across projects", async () => {
    noteAgentChatVisit(place("sage", "a"));
    noteAgentChatVisit(place("angle", "b"));
    noteAgentChatVisit(place("sage", "c"));
    const view = fakeWindow();

    await goBackInAgentChats(view.open);
    await goBackInAgentChats(view.open);
    expect(view.opened).toEqual(["angle/b", "sage/a"]);
    expect(state()).toEqual({ canGoBack: false, canGoForward: true });

    await goForwardInAgentChats(view.open);
    await goForwardInAgentChats(view.open);
    expect(view.opened.slice(2)).toEqual(["angle/b", "sage/c"]);
    expect(state()).toEqual({ canGoBack: true, canGoForward: false });
  });

  it("drops the forward trail when a new chat is opened after going back", async () => {
    noteAgentChatVisit(place("sage", "a"));
    noteAgentChatVisit(place("sage", "b"));
    const view = fakeWindow();
    await goBackInAgentChats(view.open);
    noteAgentChatVisit(place("sage", "c"));
    expect(state().canGoForward).toBe(false);
    await goBackInAgentChats(view.open);
    expect(view.opened).toEqual(["sage/a", "sage/a"]);
  });

  it("keeps the place when the window cannot move, and goes there once it can", async () => {
    noteAgentChatVisit(place("sage", "a"));
    noteAgentChatVisit(place("angle", "b"));
    const blocked = fakeWindow({ "sage/a": { kind: "blocked" } });
    await goBackInAgentChats(blocked.open);
    await goBackInAgentChats(async () => {
      throw new Error("native refusal");
    });
    expect(state()).toEqual({ canGoBack: true, canGoForward: false });

    const free = fakeWindow();
    await goBackInAgentChats(free.open);
    expect(free.opened).toEqual(["sage/a"]);
    expect(state()).toEqual({ canGoBack: false, canGoForward: true });
  });

  it("drops a chat that no longer exists and stays put", async () => {
    noteAgentChatVisit(place("sage", "archived"));
    noteAgentChatVisit(place("sage", "b"));
    await goBackInAgentChats(
      fakeWindow({ "sage/archived": { kind: "missing" } }).open,
    );
    expect(state()).toEqual({ canGoBack: false, canGoForward: false });
  });

  it("records where a partial project switch lands, so Forward returns", async () => {
    noteAgentChatVisit(place("angle", "gone"));
    noteAgentChatVisit(place("angle", "kept"));
    noteAgentChatVisit(place("sage", "start"));
    const view = fakeWindow({
      "angle/kept": {
        kind: "landedElsewhere",
        place: place("angle", "landing"),
        targetGone: false,
      },
    });
    await goBackInAgentChats(view.open);
    // The window moved to Angle, so Forward leads back to where it started,
    // and the chat it could not open stays for Back to try again.
    await goForwardInAgentChats(view.open);
    expect(view.opened).toEqual(["angle/kept", "sage/start"]);
    await goBackInAgentChats(view.open);
    expect(view.opened.at(-1)).toBe("angle/landing");
    await goBackInAgentChats(view.open);
    expect(view.opened.at(-1)).toBe("angle/kept");
  });

  it("does not note a chat seen twice in a row, and keeps a bounded trail", async () => {
    for (let index = 0; index <= MAX_AGENT_CHAT_PLACES + 5; index += 1) {
      noteAgentChatVisit(place("sage", `chat-${index}`));
      noteAgentChatVisit(place("sage", `chat-${index}`));
    }
    const view = fakeWindow();
    for (let step = 0; step < MAX_AGENT_CHAT_PLACES + 5; step += 1) {
      await goBackInAgentChats(view.open);
    }
    expect(view.opened).toHaveLength(MAX_AGENT_CHAT_PLACES);
    expect(view.opened.at(-1)).toBe("sage/chat-5");
  });
});
