import { afterEach, beforeEach, expect, it, vi } from "vitest";

import {
  chatReadThrough,
  markChatRead,
  markChatUnread,
  resetChatReadMarks,
  restoreChatReadMarks,
} from "./chatReadMarks";
import { sessionRailUnreadVisible } from "./sessionRail";

// A stand-in for the Rust settings store: it keeps the marks across a reload
// of this module's projection, so the tests observe what survives.
const store = vi.hoisted(() => ({ marks: new Map<string, number>() }));
vi.mock("../../../ports/nativeInvoke", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  invokeNative: async (
    command: string,
    args?: { mark?: { chatId: string; readThrough: number }; chatId?: string },
  ) => {
    if (command === "chat_read_marks")
      return [...store.marks].reverse().map(([chatId, readThrough]) => ({
        chatId,
        readThrough,
      }));
    if (command === "chat_mark_read" && args?.mark) {
      const { chatId, readThrough } = args.mark;
      store.marks.set(
        chatId,
        Math.max(store.marks.get(chatId) ?? 0, readThrough),
      );
      return { chatId, readThrough: store.marks.get(chatId) };
    }
    if (command === "chat_mark_unread" && args?.chatId) {
      store.marks.delete(args.chatId);
      return null;
    }
    throw new Error(`unexpected command ${command}`);
  },
}));

beforeEach(() => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  store.marks.clear();
  resetChatReadMarks();
});

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  resetChatReadMarks();
});

/** What the rail shows for a finished chat that is not on screen. */
const unread = (chatId: string, updatedAt: number) =>
  sessionRailUnreadVisible({
    focused: false,
    status: "complete",
    updatedAt,
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
