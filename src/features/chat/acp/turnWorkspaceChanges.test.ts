import { afterEach, expect, it, vi } from "vitest";
import { win32 } from "node:path";
import { snapshotPage } from "../../../app/agentDiff.fixture";
import * as acpDesktop from "../../../services/acpDesktop";

import loadTurnWorkspaceChanges, {
  decodeTurnWorkspaceChanges,
  resetTurnWorkspaceChangesForTest,
} from "./turnWorkspaceChanges";

const signal = new AbortController().signal;

afterEach(() => {
  vi.restoreAllMocks();
  resetTurnWorkspaceChangesForTest();
});

it.each(["truncated"])(
  "retries %s coverage instead of caching it as final",
  async (kind) => {
    const page = snapshotPage();
    const load = vi
      .spyOn(acpDesktop, "getTurnWorkspaceChanges")
      .mockResolvedValue({
        rootId: "root-a",
        tracked: kind !== "untracked",
        truncated: kind === "truncated",
        changes: [],
      });
    expect(
      (await loadTurnWorkspaceChanges(page, "3", signal)).incompleteTurnIds.has(
        "turn-a",
      ),
    ).toBe(true);
    load.mockResolvedValue({
      rootId: "root-a",
      tracked: true,
      truncated: false,
      changes: [{ relativePath: "recovered.ts", operation: "created" }],
    });
    const recovered = await loadTurnWorkspaceChanges(page, "3", signal);
    expect(recovered.incompleteTurnIds.size).toBe(0);
    expect(recovered.changes[0]?.relativePath).toBe("recovered.ts");
    await loadTurnWorkspaceChanges(page, "3", signal);
    expect(load).toHaveBeenCalledTimes(2);
  },
);

it("fails closed beyond 32 waiters without queuing more work", async () => {
  const page = snapshotPage();
  let finish!: (value: unknown) => void;
  const reply = new Promise((resolve) => {
    finish = resolve;
  });
  const load = vi
    .spyOn(acpDesktop, "getTurnWorkspaceChanges")
    .mockReturnValue(reply);
  const controllers = Array.from({ length: 32 }, () => new AbortController());
  const pending = controllers.map((controller, index) =>
    loadTurnWorkspaceChanges(
      { ...page, chatId: `chat-${index}` as never },
      "3",
      controller.signal,
    ),
  );
  expect(load).toHaveBeenCalledTimes(5);
  const overflow = await loadTurnWorkspaceChanges(page, "3", signal);
  expect(overflow.changes).toEqual([]);
  expect(overflow.incompleteTurnIds.has("turn-a")).toBe(true);
  controllers.forEach((controller) => controller.abort());
  await Promise.all(pending);
  expect(load).toHaveBeenCalledTimes(5);
  finish({ rootId: "root-a", tracked: true, truncated: false, changes: [] });
  await new Promise((resolve) => setTimeout(resolve, 0));
});

it("stops superseded consumers before scheduling more history or waiting work", async () => {
  const page = snapshotPage();
  page.turns = Array.from({ length: 10 }, (_, index) => ({
    ...page.turns[0],
    id: `turn-${index}` as never,
    ordinal: index,
  }));
  const controller = new AbortController();
  let finish!: (value: unknown) => void;
  const reply = new Promise((resolve) => {
    finish = resolve;
  });
  const load = vi
    .spyOn(acpDesktop, "getTurnWorkspaceChanges")
    .mockReturnValue(reply);
  const first = loadTurnWorkspaceChanges(page, "3", controller.signal);
  const waiting = loadTurnWorkspaceChanges(
    { ...page, chatId: "other" as never },
    "3",
    controller.signal,
  );
  await Promise.resolve();
  expect(load).toHaveBeenCalledTimes(5);
  controller.abort();
  finish({ rootId: "root-a", tracked: true, truncated: false, changes: [] });
  await Promise.all([first, waiting]);
  expect(load).toHaveBeenCalledTimes(5);
  await loadTurnWorkspaceChanges(page, "4", controller.signal);
  expect(load).toHaveBeenCalledTimes(5);
});

it("releases canceled consumers while all five native requests remain stalled", async () => {
  const page = snapshotPage();
  page.turns = Array.from({ length: 5 }, (_, index) => ({
    ...page.turns[0],
    id: `turn-${index}` as never,
    ordinal: index,
  }));
  let finish!: (value: unknown) => void;
  const reply = new Promise((resolve) => {
    finish = resolve;
  });
  const load = vi
    .spyOn(acpDesktop, "getTurnWorkspaceChanges")
    .mockReturnValue(reply);
  const owner = new AbortController();
  const first = loadTurnWorkspaceChanges(page, "3", owner.signal);
  await Promise.resolve();
  expect(load).toHaveBeenCalledTimes(5);
  for (let index = 0; index < 40; index += 1) {
    const controller = new AbortController();
    const waiting = loadTurnWorkspaceChanges(
      { ...page, chatId: `chat-${index}` as never },
      "3",
      controller.signal,
    );
    controller.abort();
    await waiting;
  }
  owner.abort();
  await first;
  expect(load).toHaveBeenCalledTimes(5);
  finish({ rootId: "root-a", tracked: true, truncated: false, changes: [] });
  await new Promise((resolve) => setTimeout(resolve, 0));
});

it("caps native ledger reads at five across concurrent chats", async () => {
  const page = snapshotPage();
  page.turns = Array.from({ length: 10 }, (_, index) => ({
    ...page.turns[0],
    id: `turn-${index}` as never,
    ordinal: index,
  }));
  let active = 0;
  let peak = 0;
  const load = vi
    .spyOn(acpDesktop, "getTurnWorkspaceChanges")
    .mockImplementation(async () => {
      active += 1;
      peak = Math.max(peak, active);
      await Promise.resolve();
      active -= 1;
      return { rootId: "root-a", tracked: true, truncated: false, changes: [] };
    });
  const results = await Promise.all([
    loadTurnWorkspaceChanges(page, "3", signal),
    loadTurnWorkspaceChanges(
      { ...page, chatId: "chat-b" as never },
      "3",
      signal,
    ),
  ]);
  expect(load).toHaveBeenCalledTimes(20);
  expect(peak).toBe(5);
  expect(results.every((result) => result.incompleteTurnIds.size === 0)).toBe(
    true,
  );
});

it("shares pending ledger reads between consumers without returning an empty early result", async () => {
  const page = snapshotPage();
  let complete!: (value: unknown) => void;
  const load = vi
    .spyOn(acpDesktop, "getTurnWorkspaceChanges")
    .mockImplementation(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    );
  const first = loadTurnWorkspaceChanges(page, "3", signal);
  const second = loadTurnWorkspaceChanges(page, "3", signal);
  await Promise.resolve();
  complete({
    rootId: "root-a",
    tracked: true,
    truncated: false,
    changes: [{ relativePath: "shell.ts", operation: "deleted" }],
  });
  expect((await first).changes).toEqual([
    {
      turnId: "turn-a",
      rootId: "root-a",
      relativePath: "shell.ts",
      operation: "deleted",
    },
  ]);
  expect(await second).toEqual(await first);
  expect(load).toHaveBeenCalledOnce();
  await loadTurnWorkspaceChanges(page, "3", signal);
  expect(load).toHaveBeenCalledOnce();
  load.mockResolvedValue({
    rootId: "root-a",
    tracked: true,
    truncated: false,
    changes: [],
  });
  expect((await loadTurnWorkspaceChanges(page, "4", signal)).changes).toEqual(
    [],
  );
  expect(load).toHaveBeenCalledTimes(2);
});

it("loads retained terminal turns in batches of five and retries failed reads", async () => {
  const page = snapshotPage();
  const turn = page.turns[0];
  page.turns = Array.from({ length: 10 }, (_, index) => ({
    ...turn,
    id: `turn-${index}` as never,
    ordinal: index,
  }));
  page.turns = [
    ...page.turns,
    {
      ...turn,
      id: "old-session" as never,
      sessionId: "old" as never,
    },
    { ...turn, id: "running" as never, state: "running" },
  ];
  const load = vi
    .spyOn(acpDesktop, "getTurnWorkspaceChanges")
    .mockRejectedValue(new Error("Temporary read failure"));
  const failed = await loadTurnWorkspaceChanges(page, "3", signal);
  expect(failed.changes).toEqual([]);
  expect(failed.incompleteTurnIds.size).toBe(10);
  expect(load.mock.calls.map(([request]) => request.turnId)).toEqual([
    "turn-5",
    "turn-6",
    "turn-7",
    "turn-8",
    "turn-9",
    "turn-0",
    "turn-1",
    "turn-2",
    "turn-3",
    "turn-4",
  ]);
  load.mockResolvedValue({
    rootId: "root-a",
    tracked: true,
    truncated: false,
    changes: [],
  });
  await loadTurnWorkspaceChanges(page, "3", signal);
  expect(load).toHaveBeenCalledTimes(20);
});

it("decodes bounded native workspace changes for one turn", () => {
  expect(
    decodeTurnWorkspaceChanges(
      {
        rootId: "root-a",
        tracked: true,
        truncated: false,
        changes: [
          { relativePath: "README.md", operation: "updated" },
          { relativePath: "src/new.ts", operation: "created" },
        ],
      },
      "turn-1",
    )?.changes,
  ).toEqual([
    {
      turnId: "turn-1",
      rootId: "root-a",
      relativePath: "README.md",
      operation: "updated",
    },
    {
      turnId: "turn-1",
      rootId: "root-a",
      relativePath: "src/new.ts",
      operation: "created",
    },
  ]);
});

it.each([
  ["../escape.txt"],
  [win32.resolve("abs.txt")],
  ["/abs.txt"],
  ["a\\b.txt"],
  ["a//b.txt"],
])("rejects an unsafe path %s", (relativePath) => {
  expect(
    decodeTurnWorkspaceChanges(
      {
        rootId: "root-a",
        tracked: true,
        truncated: false,
        changes: [{ relativePath, operation: "updated" }],
      },
      "turn-1",
    ),
  ).toBeNull();
});

it("rejects unknown operations and oversized replies", () => {
  const reply = (changes: unknown[]) => ({
    rootId: "root-a",
    tracked: true,
    truncated: false,
    changes,
  });
  expect(
    decodeTurnWorkspaceChanges(
      reply([{ relativePath: "a.txt", operation: "renamed" }]),
      "t",
    ),
  ).toBeNull();
  expect(
    decodeTurnWorkspaceChanges(
      reply(
        Array.from({ length: 301 }, (_, index) => ({
          relativePath: `f${index}`,
          operation: "updated",
        })),
      ),
      "t",
    ),
  ).toBeNull();
  expect(decodeTurnWorkspaceChanges(null, "t")).toBeNull();
});

it("caches untracked legacy and non-Git turns without an incomplete notice", async () => {
  const load = vi
    .spyOn(acpDesktop, "getTurnWorkspaceChanges")
    .mockResolvedValue({
      rootId: "root-a",
      tracked: false,
      truncated: false,
      changes: [],
    });
  const first = await loadTurnWorkspaceChanges(snapshotPage(), "3", signal);
  expect(first.changes).toEqual([]);
  expect(first.incompleteTurnIds.size).toBe(0);
  await loadTurnWorkspaceChanges(snapshotPage(), "3", signal);
  expect(load).toHaveBeenCalledOnce();
});
