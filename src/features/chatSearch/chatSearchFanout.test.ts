import { describe, expect, it } from "vitest";

import { MAX_CHAT_SEARCH_RESULTS } from "./chatSearchDomain";
import {
  CHAT_SEARCH_ALL_FOLDERS_ID,
  MAX_CHAT_SEARCH_FANOUT_FOLDERS,
  chatSearchFanoutFolderIds,
  chatSearchFolderOptions,
  chatSearchResultFolderName,
  isChatSearchFanout,
  mergeChatSearchFanout,
} from "./chatSearchFanout";

describe("Chat search folder fan-out", () => {
  it("lists All folders and visits at most eight real folders", () => {
    expect(isChatSearchFanout(CHAT_SEARCH_ALL_FOLDERS_ID)).toBe(true);
    expect(isChatSearchFanout("cross")).toBe(false);
    const folders = chatSearchFolderOptions([
      { id: "sage", name: "Sage" },
      { id: "cross", name: "Cross" },
      { id: "", name: "junk" },
    ]);
    expect(folders[0]).toEqual({
      id: CHAT_SEARCH_ALL_FOLDERS_ID,
      name: "All folders",
    });
    expect(chatSearchFanoutFolderIds(folders)).toEqual(["sage", "cross"]);
    const many = chatSearchFolderOptions(
      Array.from({ length: 12 }, (_, index) => ({
        id: `project-${index}`,
        name: `P${index}`,
      })),
    );
    const visited = chatSearchFanoutFolderIds(many);
    expect(visited).toHaveLength(MAX_CHAT_SEARCH_FANOUT_FOLDERS);
    expect(visited).not.toContain(CHAT_SEARCH_ALL_FOLDERS_ID);
    expect(visited[0]).toBe("project-0");
    expect(visited[7]).toBe("project-7");
    expect(chatSearchResultFolderName(folders, "cross")).toBe("Cross");
    expect(
      chatSearchResultFolderName(folders, CHAT_SEARCH_ALL_FOLDERS_ID),
    ).toBeNull();
  });

  it("merges bounded pages and does not keep more than 64 results", () => {
    const page = (projectId: string, chatId: string, hasMore = false) => ({
      projectId,
      projectGeneration: 1,
      transitionEpoch: 1,
      query: "needle",
      results: [
        {
          chatId,
          title: `Chat: ${chatId}`,
          attached: false,
          matchedTitle: true,
          snippet: null,
        },
      ],
      hasMore,
      elapsedMs: 2,
    });
    const overflow = mergeChatSearchFanout(
      [
        {
          ...page("sage", "chat-a"),
          results: Array.from(
            { length: MAX_CHAT_SEARCH_RESULTS },
            (_, index) => ({
              chatId: `chat-${index}`,
              title: `Chat: ${index}`,
              attached: true,
              matchedTitle: true,
              snippet: null,
            }),
          ),
        },
        page("cross", "chat-extra", true),
      ],
      2,
      3,
    );
    expect(overflow.results).toHaveLength(MAX_CHAT_SEARCH_RESULTS);
    expect(overflow.results[0]?.projectId).toBe("sage");
    expect(overflow.hasMore).toBe(true);
    const leftover = mergeChatSearchFanout([page("cross", "chat-old")], 1, 3);
    expect(leftover.results).toEqual([
      {
        projectId: "cross",
        chatId: "chat-old",
        title: "Chat: chat-old",
        attached: false,
        matchedTitle: true,
        snippet: null,
      },
    ]);
    expect(leftover.hasMore).toBe(true);
  });
});
