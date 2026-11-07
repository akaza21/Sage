import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import {
  MAX_CHAT_READ_MARKS,
  chatReadThrough,
  markChatRead,
  markChatUnread,
  resetChatReadMarks,
  restoreChatReadMarks,
  useChatReadMarks,
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
  // Whether each next delete lands, in order; empty means it does.
  unreadOutcomes: [] as boolean[],
  holdRead: null as Promise<void> | null,
  sequence: 0,
  // Chats a read evicts, by the chat read; the store reports them.
  evicts: new Map<string, string[]>(),
  // Holds a read's answer, by chat, after the store has applied it.
  holdAnswer: new Map<string, Promise<void>>(),
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
      if (store.holdRead) await store.holdRead;
      if (store.failWrites) throw new Error("settings store is busy");
      const { chatId, readThrough } = args.mark;
      store.marks.set(
        chatId,
        Math.max(store.marks.get(chatId) ?? 0, readThrough),
      );
      const evicted = store.evicts.get(chatId) ?? [];
      for (const pruned of evicted) store.marks.delete(pruned);
      const answer = {
        chatId,
        readThrough: store.marks.get(chatId),
        evicted,
        sequence: (store.sequence += 1),
      };
      const hold = store.holdAnswer.get(chatId);
      if (hold) await hold;
      return answer;
    }
    if (command === "chat_mark_unread" && args?.chatId) {
      if (store.holdUnread) await store.holdUnread;
      if (store.failUnread || store.unreadOutcomes.shift() === false)
        throw new Error("settings store is busy");
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
  store.unreadOutcomes = [];
  store.holdRead = null;
  store.sequence = 0;
  store.evicts.clear();
  store.holdAnswer.clear();
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
  // The store has not answered any of these yet.
  const first = markChatUnread("chat-0");
  for (let index = 0; index < MAX_CHAT_READ_MARKS - 1; index += 1)
    void markChatUnread(`other-${index}`);
  // One request beyond the bound is refused rather than held.
  await expect(markChatUnread("chat-1")).resolves.toBe(false);
  store.failLoad = false;
  // The snapshot predates the deletes still on their way to the store.
  await restoreChatReadMarks();
  expect(chatReadThrough("chat-0")).toBeUndefined();
  expect(chatReadThrough("chat-1")).toBe(1);
  release();
  await expect(first).resolves.toBe(true);
  expect(store.marks.has("chat-0")).toBe(false);
});

it("keeps a chat read when it is reopened before Mark unread reaches the store", async () => {
  let release!: () => void;
  store.holdLoad = null;
  // The first read is still on its way when the user marks the chat unread
  // and opens it again.
  const held = new Promise<void>((resolve) => (release = resolve));
  store.holdRead = held;
  markChatRead("chat-a", 1_000);
  const unreadRequest = markChatUnread("chat-a");
  markChatRead("chat-a", 1_000);
  release();
  await expect(unreadRequest).resolves.toBe(true);
  await vi.waitFor(() => expect(store.writes).toBe(2));
  expect(store.marks.get("chat-a")).toBe(1_000);
  await reload();
  expect(unread("chat-a", 1_000)).toBe(false);
});

it("shows the stored mark when Mark unread made while it loads is refused", async () => {
  store.marks.set("chat-a", 1_000);
  let releaseLoad!: () => void;
  store.holdLoad = new Promise((resolve) => (releaseLoad = resolve));
  let releaseDelete!: () => void;
  store.holdUnread = new Promise((resolve) => (releaseDelete = resolve));
  const loading = restoreChatReadMarks();
  store.failUnread = true;
  const request = markChatUnread("chat-a");
  releaseLoad();
  await loading;
  // The pending delete hides the stored mark meanwhile.
  expect(chatReadThrough("chat-a")).toBeUndefined();
  releaseDelete();
  await expect(request).resolves.toBe(false);
  // The store still holds the mark, and the rail says so.
  expect(chatReadThrough("chat-a")).toBe(1_000);
  expect(store.marks.get("chat-a")).toBe(1_000);
});

it("keeps the newest Mark unread when an earlier one is refused", async () => {
  markChatRead("chat-a", 1_000);
  await vi.waitFor(() => expect(store.marks.get("chat-a")).toBe(1_000));
  store.unreadOutcomes = [false, true];
  let release!: () => void;
  store.holdUnread = new Promise((resolve) => (release = resolve));
  const first = markChatUnread("chat-a");
  const second = markChatUnread("chat-a");
  release();
  // The second request is the one the chat shows.
  await expect(first).resolves.toBe(true);
  await expect(second).resolves.toBe(true);
  expect(chatReadThrough("chat-a")).toBeUndefined();
  expect(store.marks.has("chat-a")).toBe(false);
});

it("loads the stored marks again after a failed load while the rail is mounted", async () => {
  vi.useFakeTimers();
  try {
    store.marks.set("chat-a", 1_000);
    store.failLoad = true;
    const rail = renderHook(() => useChatReadMarks());
    await vi.advanceTimersByTimeAsync(0);
    expect(chatReadThrough("chat-a")).toBeUndefined();
    // The store recovers; the next attempt backs off, then lands.
    store.failLoad = false;
    await vi.advanceTimersByTimeAsync(1_000);
    expect(chatReadThrough("chat-a")).toBe(1_000);
    rail.unmount();
  } finally {
    vi.useRealTimers();
  }
});

it("drops a mark the store evicted even when that chat's own answer arrives later", async () => {
  let release!: () => void;
  store.holdAnswer.set("chat-a", new Promise((resolve) => (release = resolve)));
  // The store applies chat-a's read, then chat-b's read evicts chat-a, but
  // chat-b's answer reaches the rail first.
  markChatRead("chat-a", 1_000);
  await vi.waitFor(() => expect(store.marks.get("chat-a")).toBe(1_000));
  store.evicts.set("chat-b", ["chat-a"]);
  markChatRead("chat-b", 2_000);
  await vi.waitFor(() => expect(store.marks.has("chat-a")).toBe(false));
  release();
  await vi.waitFor(() => expect(chatReadThrough("chat-b")).toBe(2_000));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(chatReadThrough("chat-a")).toBeUndefined();
  await reload();
  expect(chatReadThrough("chat-a")).toBeUndefined();
});
