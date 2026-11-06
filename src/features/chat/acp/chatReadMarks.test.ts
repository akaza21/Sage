import { afterEach, beforeEach, expect, it, vi } from "vitest";

import {
  MAX_CHAT_READ_MARKS,
  chatReadThrough,
  markChatRead,
  markChatUnread,
  resetChatReadMarks,
  restoreChatReadMarks,
} from "./chatReadMarks";
import { sessionRailUnreadVisible } from "./sessionRail";

// A stand-in for the Rust settings store: it keeps the marks across a reload
// of this module's projection, so the tests observe what survives.
const store = vi.hoisted(() => ({
  marks: new Map<string, number>(),
  failWrites: false,
  writes: 0,
  // Resolves the stored snapshot when the test says so.
  holdLoad: null as Promise<void> | null,
  failLoad: false,
  failUnread: false,
  // Keeps deletes from landing until the test says so.
  holdUnread: null as Promise<void> | null,
}));
vi.mock("../../../ports/nativeInvoke", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  invokeNative: async (
    command: string,
    args?: { mark?: { chatId: string; readThrough: number }; chatId?: string },
  ) => {
    if (command === "chat_read_marks") {
      if (store.failLoad) throw new Error("settings store is locked");
      const snapshot = [...store.marks];
      if (store.holdLoad) await store.holdLoad;
      return snapshot
        .reverse()
        .map(([chatId, readThrough]) => ({ chatId, readThrough }));
    }
    if (command === "chat_mark_read" && args?.mark) {
      store.writes += 1;
      if (store.failWrites) throw new Error("settings store is busy");
      const { chatId, readThrough } = args.mark;
      store.marks.set(
        chatId,
        Math.max(store.marks.get(chatId) ?? 0, readThrough),
      );
      return { chatId, readThrough: store.marks.get(chatId) };
    }
    if (command === "chat_mark_unread" && args?.chatId) {
      if (store.failUnread) throw new Error("settings store is busy");
      if (store.holdUnread) await store.holdUnread;
      store.marks.delete(args.chatId);
      return null;
    }
    throw new Error(`unexpected command ${command}`);
  },
}));

beforeEach(() => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  store.marks.clear();
  store.failWrites = false;
  store.writes = 0;
  store.holdLoad = null;
  store.failLoad = false;
  store.failUnread = false;
  store.holdUnread = null;
  resetChatReadMarks();
});

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  resetChatReadMarks();
});

/** What the rail shows for a finished chat that is not on screen. */
const unread = (chatId: string, finishedAt: number) =>
  sessionRailUnreadVisible({
    focused: false,
    status: "complete",
    finishedAt,
    readThrough: chatReadThrough(chatId),
  });

async function reload() {
  resetChatReadMarks();
  await restoreChatReadMarks();
}

it("keeps a viewed finished chat read after a reload (B-19)", async () => {
  markChatRead("chat-a", 1_000);
  await vi.waitFor(() => expect(store.marks.get("chat-a")).toBe(1_000));
  await reload();
  expect(unread("chat-a", 1_000)).toBe(false);
  // A later update is new to the user again.
  expect(unread("chat-a", 2_000)).toBe(true);
});

it("keeps the later time when a mark lands before the stored ones load", async () => {
  store.marks.set("chat-a", 1_000);
  markChatRead("chat-a", 3_000);
  await restoreChatReadMarks();
  expect(chatReadThrough("chat-a")).toBe(3_000);
});

it("keeps a chat marked unread Unread after a reload", async () => {
  markChatRead("chat-a", 1_000);
  await vi.waitFor(() => expect(store.marks.has("chat-a")).toBe(true));
  markChatUnread("chat-a");
  await vi.waitFor(() => expect(store.marks.has("chat-a")).toBe(false));
  await reload();
  expect(unread("chat-a", 1_000)).toBe(true);
});

it("writes a failed read mark again when the chat is viewed again", async () => {
  store.failWrites = true;
  markChatRead("chat-a", 1_000);
  await vi.waitFor(() => expect(store.writes).toBe(1));
  // Let the rejected write settle.
  await new Promise((resolve) => setTimeout(resolve, 0));
  store.failWrites = false;
  // Viewing the chat again records the same Turn.
  markChatRead("chat-a", 1_000);
  await vi.waitFor(() => expect(store.marks.get("chat-a")).toBe(1_000));
  await reload();
  expect(unread("chat-a", 1_000)).toBe(false);
});

it("does not write a mark the store already holds", async () => {
  markChatRead("chat-a", 1_000);
  await vi.waitFor(() => expect(store.marks.get("chat-a")).toBe(1_000));
  markChatRead("chat-a", 1_000);
  await Promise.resolve();
  expect(store.writes).toBe(1);
});

it("keeps a mark made while a full stored snapshot loads", async () => {
  for (let index = 0; index < MAX_CHAT_READ_MARKS; index += 1)
    store.marks.set(`stored-${index}`, 1);
  let release!: () => void;
  store.holdLoad = new Promise((resolve) => (release = resolve));
  const loading = restoreChatReadMarks();
  markChatRead("chat-new", 2_000);
  release();
  await loading;
  expect(chatReadThrough("chat-new")).toBe(2_000);
  // The oldest stored mark gave way; the most recent stored one stayed.
  expect(chatReadThrough("stored-0")).toBeUndefined();
  expect(chatReadThrough(`stored-${MAX_CHAT_READ_MARKS - 1}`)).toBe(1);
});

it("shows the stored mark again when Mark unread is refused", async () => {
  markChatRead("chat-a", 1_000);
  await vi.waitFor(() => expect(store.marks.get("chat-a")).toBe(1_000));
  store.failUnread = true;
  markChatUnread("chat-a");
  expect(chatReadThrough("chat-a")).toBeUndefined();
  // The store still holds the mark, so the rail shows the chat read again.
  await vi.waitFor(() => expect(chatReadThrough("chat-a")).toBe(1_000));
  await reload();
  expect(unread("chat-a", 1_000)).toBe(false);
});

it("bounds the Mark unread requests held while the stored marks fail to load", async () => {
  store.marks.set("chat-0", 1);
  store.marks.set("chat-1", 1);
  store.failLoad = true;
  await restoreChatReadMarks();
  let release!: () => void;
  store.holdUnread = new Promise((resolve) => (release = resolve));
  // One request more than the bound: chat-0 first, chat-1 last.
  markChatUnread("chat-0");
  for (let index = 0; index < MAX_CHAT_READ_MARKS - 1; index += 1)
    markChatUnread(`other-${index}`);
  markChatUnread("chat-1");
  store.failLoad = false;
  // The snapshot predates the deletes still on their way to the store.
  await restoreChatReadMarks();
  release();
  expect(chatReadThrough("chat-1")).toBeUndefined();
  // Only the oldest request beyond the bound gives way.
  expect(chatReadThrough("chat-0")).toBe(1);
});
