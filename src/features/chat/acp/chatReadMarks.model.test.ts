import { afterAll, beforeAll, expect, it, vi } from "vitest";

import {
  chatReadThrough,
  markChatRead,
  markChatUnread,
  resetChatReadMarks,
  restoreChatReadMarks,
} from "./chatReadMarks";

/**
 * A model of the settings store for one chat. Every call waits until the
 * test answers it, so the test decides the order in which the store runs
 * the snapshot query, applies writes, and delivers answers.
 */
const fake = vi.hoisted(() => ({
  // The chat's stored mark, or null for none.
  stored: null as number | null,
  writes: [] as Array<{
    command: string;
    readThrough?: number;
    resolve: (value: unknown) => void;
    reject: (error: Error) => void;
  }>,
  loads: [] as Array<{
    snapshot: number | null;
    resolve: (value: unknown) => void;
  }>,
}));

vi.mock("../../../ports/nativeInvoke", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  invokeNative: (
    command: string,
    args?: { mark?: { readThrough: number } },
  ): Promise<unknown> =>
    new Promise((resolve, reject) => {
      // The query runs when it is issued; its answer arrives later.
      if (command === "chat_read_marks")
        fake.loads.push({ snapshot: fake.stored, resolve });
      else
        fake.writes.push({
          command,
          readThrough: args?.mark?.readThrough,
          resolve,
          reject,
        });
    }),
}));

const CHAT = "chat-a";
type Step =
  "restoreStart" | "restoreFinish" | "read" | "unread" | "ackOk" | "ackRefused";
const STEPS: Step[] = [
  "restoreStart",
  "restoreFinish",
  "read",
  "unread",
  "ackOk",
  "ackRefused",
];

async function flush() {
  for (let index = 0; index < 20; index += 1) await Promise.resolve();
}

function answerWrite(ok: boolean) {
  const write = fake.writes.shift();
  if (!write) return;
  if (!ok) return write.reject(new Error("settings store is busy"));
  if (write.command === "chat_mark_unread") {
    fake.stored = null;
    return write.resolve(null);
  }
  fake.stored = Math.max(fake.stored ?? 0, write.readThrough!);
  write.resolve({ chatId: CHAT, readThrough: fake.stored });
}

function finishLoad() {
  const load = fake.loads.shift();
  load?.resolve(
    load.snapshot === null
      ? []
      : [{ chatId: CHAT, readThrough: load.snapshot }],
  );
}

async function run(steps: readonly Step[]) {
  let readAt = 0;
  for (const step of steps) {
    if (step === "restoreStart") void restoreChatReadMarks();
    else if (step === "restoreFinish") finishLoad();
    else if (step === "read") markChatRead(CHAT, (readAt += 100));
    else if (step === "unread") void markChatUnread(CHAT);
    else answerWrite(step === "ackOk");
    await flush();
  }
}

/** Lets every outstanding call land, as a running app eventually does. */
async function settle() {
  while (fake.writes.length || fake.loads.length) {
    answerWrite(true);
    finishLoad();
    await flush();
  }
  // The rail loads the stored marks on mount; make sure that happened.
  void restoreChatReadMarks();
  await flush();
  while (fake.writes.length || fake.loads.length) {
    answerWrite(true);
    finishLoad();
    await flush();
  }
}

function* sequences(depth: number): Generator<Step[]> {
  yield [];
  if (depth === 0) return;
  for (const rest of sequences(depth - 1))
    if (rest.length === depth - 1)
      for (const step of STEPS) yield [...rest, step];
}

beforeAll(() => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
});

afterAll(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  resetChatReadMarks();
});

it("shows what the store holds once every interleaving settles, and after a restart", async () => {
  let checked = 0;
  for (const initial of [null, 50]) {
    for (let depth = 0; depth <= 5; depth += 1) {
      for (const steps of sequences(depth)) {
        if (steps.length !== depth) continue;
        resetChatReadMarks();
        fake.stored = initial;
        fake.writes = [];
        fake.loads = [];
        await run(steps);
        await settle();
        const label = `${initial} ${steps.join(" ")}`;
        expect(chatReadThrough(CHAT) ?? null, label).toBe(fake.stored);
        // A restart starts from the store alone.
        resetChatReadMarks();
        void restoreChatReadMarks();
        await flush();
        finishLoad();
        await flush();
        expect(chatReadThrough(CHAT) ?? null, `${label} restart`).toBe(
          fake.stored,
        );
        checked += 1;
      }
    }
  }
  expect(checked).toBe(2 * (1 + 6 + 36 + 216 + 1296 + 7776));
}, 120_000);
