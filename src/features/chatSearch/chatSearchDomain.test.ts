import { describe, expect, it } from "vitest";

import { chatPlacement } from "../../app/openChat";
import {
  parseProjectChatSearchResponse,
  validateProjectChatSearchRequest,
  type ProjectChatSearchResponse,
} from "./chatSearchDomain";
import {
  admitChatSearchFolder,
  chatSearchFolderOptions,
} from "./chatSearchFanout";

const valid: ProjectChatSearchResponse = {
  projectId: "project-a",
  projectGeneration: 4,
  transitionEpoch: 3,
  query: "%_literal Ω",
  results: [
    {
      chatId: "chat-a",
      title: "Chat: %_literal Ω",
      attached: true,
      matchedTitle: true,
      snippet: null,
    },
    {
      chatId: "chat-b",
      title: "Chat: Archived",
      attached: false,
      matchedTitle: false,
      snippet: "durable %_literal Ω content",
    },
  ],
  hasMore: false,
  elapsedMs: 2,
};

describe("Project Chat search domain", () => {
  it("admits empty-query browsing without relaxing response identity or match validation", () => {
    expect(() =>
      validateProjectChatSearchRequest({
        projectId: "project-a",
        projectGeneration: 4,
        expectedTransitionEpoch: 3,
        query: "",
      }),
    ).not.toThrow();
    const browse = {
      ...valid,
      query: "",
      results: [{ ...valid.results[0], matchedTitle: true }],
    };
    expect(parseProjectChatSearchResponse(browse)).toEqual(browse);
    expect(() =>
      parseProjectChatSearchResponse({
        ...browse,
        results: [{ ...browse.results[0], matchedTitle: false }],
      }),
    ).toThrow("invalid Project Chat search response");
    expect(() =>
      validateProjectChatSearchRequest({
        projectId: "",
        projectGeneration: 4,
        expectedTransitionEpoch: 3,
        query: "",
      }),
    ).toThrow("valid Project ID");
  });
  it("accepts a bounded literal response with attached and archived results", () => {
    expect(parseProjectChatSearchResponse(valid)).toEqual(valid);
  });

  it("rejects duplicate, contradictory, oversized, or nonmatching results", () => {
    expect(() =>
      parseProjectChatSearchResponse({
        ...valid,
        results: [valid.results[0], valid.results[0]],
      }),
    ).toThrow("invalid Project Chat search response");
    expect(() =>
      parseProjectChatSearchResponse({
        ...valid,
        results: [{ ...valid.results[0], matchedTitle: false }],
      }),
    ).toThrow("invalid Project Chat search response");
    expect(() =>
      parseProjectChatSearchResponse({
        ...valid,
        results: [{ ...valid.results[1], snippet: "x".repeat(2_049) }],
      }),
    ).toThrow("invalid Project Chat search response");
    expect(() =>
      parseProjectChatSearchResponse({
        ...valid,
        results: Array.from({ length: 65 }, (_, index) => ({
          ...valid.results[0],
          chatId: `chat-${index}`,
        })),
      }),
    ).toThrow("invalid Project Chat search response");
  });

  it("measures the query in UTF-8 bytes at the exact 256-byte boundary", () => {
    expect(() =>
      validateProjectChatSearchRequest({
        projectId: "project-a",
        projectGeneration: 4,
        expectedTransitionEpoch: 3,
        query: "é".repeat(128),
      }),
    ).not.toThrow();
    expect(() =>
      validateProjectChatSearchRequest({
        projectId: "project-a",
        projectGeneration: 4,
        expectedTransitionEpoch: 3,
        query: `${"é".repeat(128)}a`,
      }),
    ).toThrow("at most 256 UTF-8 bytes");
  });
});

describe("Chat search folder scope", () => {
  it("admits rail folders and resolves foreign archived attachment", () => {
    expect(
      chatSearchFolderOptions([
        { id: "sage", name: "Sage" },
        { id: "cross", name: "Cross" },
        { id: "", name: "junk" },
      ]),
    ).toEqual([
      { id: "all-folders", name: "All folders" },
      { id: "sage", name: "Sage" },
      { id: "cross", name: "Cross" },
    ]);
    expect(
      chatPlacement(
        {
          project: { id: "sage", name: "Sage", roots: [] },
          tabs: [],
          archivedChats: [],
          projects: [
            {
              id: "cross",
              name: "Cross",
              rootCount: 1,
              lastOpenedAt: 1,
              railArchived: [{ id: "chat-old", title: "Research" }],
            },
          ],
        },
        "cross",
        "chat-old",
      ),
    ).toEqual({ attached: false, archived: true });
    expect(
      admitChatSearchFolder(
        [
          { id: "sage", name: "Sage" },
          { id: "cross", name: "Cross" },
        ],
        "cross",
        "sage",
      ),
    ).toBe("cross");
    expect(
      admitChatSearchFolder([{ id: "sage", name: "Sage" }], "missing", "sage"),
    ).toBe("sage");
  });
});
