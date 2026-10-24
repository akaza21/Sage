import { expect, it } from "vitest";
import {
  isSideChatId,
  SIDE_CHAT_SPLIT,
  useSideChatState,
} from "./sideChatState";

it("recognises only Rust-minted temporary Chat ids", () => {
  expect(isSideChatId("side-0b6f2b8e-6a3c-4b71-9d1e-2f9b3e7c5a10")).toBe(true);
  expect(isSideChatId("side-fixture-1")).toBe(true);
  expect(isSideChatId("chat-2")).toBe(false);
  expect(isSideChatId("aside-1")).toBe(false);
  expect(isSideChatId(undefined)).toBe(false);
  expect(isSideChatId(null)).toBe(false);
});

it("starts with no side chats and the Codex 43 / 57 split", () => {
  expect(useSideChatState.getState()).toMatchObject({
    sides: [],
    pendingClose: null,
    split: SIDE_CHAT_SPLIT,
  });
  expect(SIDE_CHAT_SPLIT).toBe(43);
});
