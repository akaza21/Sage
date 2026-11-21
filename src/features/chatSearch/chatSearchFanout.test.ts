import { describe, expect, it } from "vitest";

import { MAX_CHAT_SEARCH_RESULTS } from "./chatSearchDomain";
import {
  CHAT_SEARCH_ALL_FOLDERS_ID,
  MAX_CHAT_SEARCH_FANOUT_FOLDERS,
  chatSearchFanoutFolderIds,
  chatSearchFolderOptions,
  chatSearchResultFolderName,
  isChatSearchFanout,
} from "./chatSearchFanout";
import {
  mergeChatSearchFanout,
  runScopedChatSearch,
} from "./chatSearchExecution";

describe("Chat search folder fan-out", () => {
  it("keeps newer results from later folders even after the first folder fills the result cap", async () => {
    const result = await runScopedChatSearch({
      searchProjectId: CHAT_SEARCH_ALL_FOLDERS_ID,
      folders: [{ id: "older" }, { id: "newer" }],
      projectGeneration: 1,
      expectedTransitionEpoch: 1,
      query: "",
      search: async (request) => ({
        projectId: request.projectId,
        projectGeneration: 1,
        transitionEpoch: 1,
        query: "",
        hasMore: false,
        elapsedMs: 1,
        results: Array.from({ length: 64 }, (_, index) => ({
          chatId: `${request.projectId}-${index}`,
          title: "Chat",
          attached: true,
          matchedTitle: true,
          snippet: null,
          updatedAt: request.projectId === "newer" ? 100 + index : index,
        })),
      }),
    });
    expect(result.results).toHaveLength(64);
    expect(result.results[0].chatId).toBe("newer-63");
    expect(result.results[63].chatId).toBe("newer-0");
    expect(result.hasMore).toBe(true);
  });
  it("interleaves searched folders by chat recency before keeping the newest 64", () => {
    const page = (projectId: string, updatedAt: number) => ({
      projectId,
      projectGeneration: 1,
      transitionEpoch: 1,
      query: "",
      results: [
        {
          chatId: "chat",
          title: "Chat",
          attached: true,
          matchedTitle: true,
          snippet: null,
          updatedAt,
        },
      ],
      hasMore: false,
      elapsedMs: 1,
    });
    const merged = mergeChatSearchFanout(
      [page("older", 10), page("newer", 30), page("middle", 20)],
      3,
      3,
    );
    expect(merged.results.map((result) => result.projectId)).toEqual([
      "newer",
      "middle",
      "older",
    ]);
    expect(merged.hasMore).toBe(false);
  });
  it("bounds private ownership to 64 groups even when all public folders are empty", async () => {
    const result = await runScopedChatSearch({
      searchProjectId: CHAT_SEARCH_ALL_FOLDERS_ID,
      folders: chatSearchFolderOptions(
        [{ id: "public", name: "Public" }],
        true,
      ),
      privateFolders: Array.from({ length: 65 }, (_, index) => ({
        id: `private-${index}`,
      })),
      projectGeneration: 1,
      expectedTransitionEpoch: 1,
      query: "needle",
      search: async (request) => ({
        projectId: request.projectId,
        projectGeneration: 1,
        transitionEpoch: 1,
        query: "needle",
        results:
          request.projectId === "public"
            ? []
            : [
                {
                  chatId: `chat-${request.projectId}`,
                  title: "needle",
                  attached: true,
                  matchedTitle: true,
                  snippet: null,
                },
              ],
        hasMore: false,
        elapsedMs: 1,
      }),
    });
    expect(result.results).toHaveLength(64);
    expect(result.results[0].projectId).toBe("private-0");
    expect(result.results[63].projectId).toBe("private-63");
    expect(result.hasMore).toBe(true);
    expect(chatSearchFolderOptions([], true)).toEqual([
      { id: CHAT_SEARCH_ALL_FOLDERS_ID, name: "All folders" },
    ]);
  });
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
