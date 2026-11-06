import { afterAll, beforeAll, expect, it, vi } from "vitest";

import {
  MAX_CHAT_READ_MARKS,
  chatReadThrough,
  markChatRead,
  markChatUnread,
  resetChatReadMarks,
  restoreChatReadMarks,
} from "./chatReadMarks";

/**
 * A model of the settings store. Like settings_store/chat_read_marks.rs it
 * keeps the later mark and only the most recently read chats. Every call
 * waits until the test answers it, so the test decides the order in which
 * the store runs the snapshot query, applies writes, and delivers answers.
 */
const fake = vi.hoisted(() => ({
  stored: new Map<string, { readThrough: number; sequence: number }>(),
  sequence: 0,
  writes: [] as Array<{
    command: string;
    chatId: string;
    readThrough?: number;
    resolve: (value: unknown) => void;
    reject: (error: Error) => void;
  }>,
  loads: [] as Array<{
    snapshot: Array<{ chatId: string; readThrough: number }>;
    resolve: (value: unknown) => void;
    reject: (error: Error) => void;
  }>,
}));

function snapshot() {
  return [...fake.stored]
    .sort((left, right) => right[1].sequence - left[1].sequence)
    .slice(0, MAX_CHAT_READ_MARKS)
    .map(([chatId, { readThrough }]) => ({ chatId, readThrough }));
}

vi.mock("../../../ports/nativeInvoke", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  invokeNative: (
    command: string,
    args?: { chatId?: string; mark?: { chatId: string; readThrough: number } },
  ): Promise<unknown> =>
    new Promise((resolve, reject) => {
      // The query runs when it is issued; its answer arrives later.
      if (command === "chat_read_marks")
        fake.loads.push({ snapshot: snapshot(), resolve, reject });
      else
        fake.writes.push({
          command,
          chatId: args?.mark?.chatId ?? args!.chatId!,
          readThrough: args?.mark?.readThrough,
          resolve,
          reject,
        });
    }),
}));

function storeRead(chatId: string, readThrough: number) {
  const current = fake.stored.get(chatId)?.readThrough ?? 0;
  fake.stored.set(chatId, {
    readThrough: Math.max(current, readThrough),
    sequence: (fake.sequence += 1),
  });
  const ordered = [...fake.stored].sort(
    (left, right) => right[1].sequence - left[1].sequence,
  );
  for (const [pruned] of ordered.slice(MAX_CHAT_READ_MARKS))
    fake.stored.delete(pruned);
  return fake.stored.get(chatId)!.readThrough;
}

function answerWrite(ok: boolean) {
  const write = fake.writes.shift();
  if (!write) return;
  if (!ok) return write.reject(new Error("settings store is busy"));
  if (write.command === "chat_mark_unread") {
    fake.stored.delete(write.chatId);
    return write.resolve(null);
  }
  const readThrough = storeRead(write.chatId, write.readThrough!);
  write.resolve({ chatId: write.chatId, readThrough });
}

function answerLoad(ok: boolean) {
  const load = fake.loads.shift();
  if (!load) return;
  if (ok) load.resolve(load.snapshot);
  else load.reject(new Error("settings store is locked"));
}

type Step =
  | { kind: "restoreStart" }
  | { kind: "restoreFinish" | "restoreFail" }
  | { kind: "read" | "unread"; chatId: string }
  | { kind: "ackOk" | "ackRefused" };

function steps(chats: readonly string[]): Step[] {
  return [
    { kind: "restoreStart" },
    { kind: "restoreFinish" },
    { kind: "restoreFail" },
    ...chats.flatMap((chatId): Step[] => [
      { kind: "read", chatId },
      { kind: "unread", chatId },
    ]),
    { kind: "ackOk" },
    { kind: "ackRefused" },
  ];
}

const label = (step: Step) =>
  "chatId" in step ? `${step.kind}(${step.chatId})` : step.kind;

async function flush() {
  for (let index = 0; index < 20; index += 1) await Promise.resolve();
}

async function run(sequence: readonly Step[]) {
  let readAt = 1_000;
  for (const step of sequence) {
    if (step.kind === "restoreStart") void restoreChatReadMarks();
    else if (step.kind === "restoreFinish") answerLoad(true);
    else if (step.kind === "restoreFail") answerLoad(false);
    else if (step.kind === "read") markChatRead(step.chatId, (readAt += 100));
    else if (step.kind === "unread") void markChatUnread(step.chatId);
    else answerWrite(step.kind === "ackOk");
    await flush();
  }
}

async function landEverything() {
  while (fake.writes.length || fake.loads.length) {
    answerWrite(true);
    answerLoad(true);
    await flush();
  }
}

/** Lets every outstanding call land, as a running app eventually does. */
async function settle() {
  await landEverything();
  // The rail loads the stored marks on mount and retries a failed load.
  void restoreChatReadMarks();
  await flush();
  await landEverything();
}

function* sequences(
  alphabet: readonly Step[],
  depth: number,
): Generator<Step[]> {
  if (depth === 0) {
    yield [];
    return;
  }
  for (const rest of sequences(alphabet, depth - 1))
    for (const step of alphabet) yield [...rest, step];
}

function expectRailMatchesStore(chats: readonly string[], context: string) {
  for (const chatId of chats)
    expect(chatReadThrough(chatId), `${context}: ${chatId}`).toBe(
      fake.stored.get(chatId)?.readThrough,
    );
}

async function checkAll(input: {
  chats: readonly string[];
  /** Marks already in the store, oldest first. */
  initial: ReadonlyArray<readonly [string, number]>;
  depth: number;
  /** Chats to compare besides the ones the steps touch. */
  watched?: readonly string[];
}) {
  const alphabet = steps(input.chats);
  const watched = [...input.chats, ...(input.watched ?? [])];
  let checked = 0;
  for (let depth = 0; depth <= input.depth; depth += 1) {
    for (const sequence of sequences(alphabet, depth)) {
      resetChatReadMarks();
      fake.stored = new Map();
      fake.sequence = 0;
      for (const [chatId, readThrough] of input.initial)
        storeRead(chatId, readThrough);
      fake.writes = [];
      fake.loads = [];
      await run(sequence);
      await settle();
      const context = sequence.map(label).join(" ") || "(no steps)";
      expectRailMatchesStore(watched, context);
      // A restart starts from the store alone.
      resetChatReadMarks();
      void restoreChatReadMarks();
      await flush();
      answerLoad(true);
      await flush();
      expectRailMatchesStore(watched, `${context}, after a restart`);
      checked += 1;
    }
  }
  return checked;
}

beforeAll(() => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
});

afterAll(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  resetChatReadMarks();
});

const total = (width: number, depth: number) =>
  Array.from({ length: depth + 1 }, (_, level) => width ** level).reduce(
    (sum, count) => sum + count,
  );

it("shows what the store holds for one chat once every interleaving settles, and after a restart", async () => {
  let checked = 0;
  for (const initial of [[], [["chat-a", 50]]] as const)
    checked += await checkAll({ chats: ["chat-a"], initial, depth: 5 });
  expect(checked).toBe(2 * total(7, 5));
}, 300_000);

it("keeps every stored mark at the capacity boundary across two chats", async () => {
  // The store is one mark short of full, and chat-b starts unmarked.
  const fillers = Array.from(
    { length: MAX_CHAT_READ_MARKS - 1 },
    (_, index) => [`filler-${index}`, 1] as const,
  );
  const checked = await checkAll({
    chats: ["chat-a", "chat-b"],
    initial: [...fillers, ["chat-a", 50]],
    depth: 4,
    // The oldest stored marks are the ones a new mark pushes out.
    watched: ["filler-0", "filler-1", `filler-${MAX_CHAT_READ_MARKS - 2}`],
  });
  expect(checked).toBe(total(9, 4));
}, 300_000);
