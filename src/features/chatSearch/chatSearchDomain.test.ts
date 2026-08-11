import { describe, expect, it } from "vitest";

import {
  parseProjectChatSearchResponse,
  validateProjectChatSearchRequest,
  type ProjectChatSearchResponse,
} from "./chatSearchDomain";

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
    ).toThrow("1-256 UTF-8 bytes");
  });
});
