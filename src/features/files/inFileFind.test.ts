import { afterEach, describe, expect, it, vi } from "vitest";

import {
  MAX_EDITABLE_CONTENT_BYTES,
  MAX_FIND_QUERY_LENGTH,
} from "./editorContent";
import {
  MAX_FIND_MATCHES,
  REGEX_FIND_TIMEOUT_MS,
  createFindWorkerClient,
  findMatches,
  type FindOptions,
} from "./inFileFind";

const LITERAL: FindOptions = {
  caseSensitive: false,
  wholeWord: false,
  regex: false,
};
const REGEX: FindOptions = { ...LITERAL, regex: true };

function find(content: string, query: string, options = LITERAL) {
  return findMatches({ content, query, options });
}

describe("findMatches", () => {
  it("returns no matches for an empty query", () => {
    expect(find("one two", "")).toEqual({
      ranges: [],
      truncated: false,
      contentLimitExceeded: false,
    });
  });

  it("finds non-overlapping literal matches with optional case folding", () => {
    expect(
      find("Needle needle NEEDLE", "needle", {
        ...LITERAL,
        caseSensitive: true,
      }).ranges,
    ).toEqual([7, 13]);
    expect(find("Needle needle NEEDLE", "needle").ranges).toEqual([
      0, 6, 7, 13, 14, 20,
    ]);
    expect(find("a.b axb", "a.b").ranges).toEqual([0, 3]);
  });

  it("keeps whole-word matches only, for literals and patterns alike", () => {
    const options = { ...LITERAL, wholeWord: true };
    expect(find("cat concat cat_ cats (cat)", "cat", options).ranges).toEqual([
      0, 3, 22, 25,
    ]);
    expect(find("été étés", "été", options).ranges).toEqual([0, 3]);
    expect(
      find("run1 run rerun", "run\\d?", { ...REGEX, wholeWord: true }).ranges,
    ).toEqual([0, 4, 5, 8]);
  });

  it("matches patterns line by line, with ^ and $ at each line's ends", () => {
    expect(find("ab\r\nab\nxab", "^ab$", REGEX).ranges).toEqual([0, 2, 4, 6]);
    expect(find("a\nb", "a\\sb", REGEX).ranges).toEqual([]);
    expect(find("Foo foo", "foo", REGEX).ranges).toEqual([0, 3, 4, 7]);
    expect(
      find("Foo foo", "foo", { ...REGEX, caseSensitive: true }).ranges,
    ).toEqual([4, 7]);
  });

  it("refuses bad patterns without echoing them", () => {
    expect(find("a", "(", REGEX).error).toBe("invalid");
    expect(find("a", "a*", REGEX).error).toBe("empty");
    expect(find("a", "\\b", REGEX).error).toBe("empty");
    expect(find("A", "(?i:a)", { ...REGEX, caseSensitive: true }).error).toBe(
      "inlineFlag",
    );
    expect(find("A", "(?-i:a)", REGEX).error).toBe("inlineFlag");
    expect(find("(?i:a)", "\\(\\?i:a\\)", REGEX).ranges).toEqual([0, 6]);
    expect(find("a(", "(", LITERAL).ranges).toEqual([1, 2]);
  });

  it("expands replacements for patterns and keeps literals as typed", () => {
    const replace = (
      content: string,
      query: string,
      replacement: string,
      options = REGEX,
    ) => findMatches({ content, query, options, replacement }).inserts;
    expect(
      replace("key=value", "(\\w+)=(?<v>\\w+)", "$2:$1 $<v> $& $$"),
    ).toEqual(["value:key value key=value $"]);
    expect(replace("a b", "(a) (b)", "$1\\n$2\\t\\\\$3")).toEqual([
      "a\nb\t\\$3",
    ]);
    expect(replace("a1 a2", "a(\\d)", "<$1>")).toEqual(["<1>", "<2>"]);
    expect(replace("a.b", "a.b", "$1\\n", LITERAL)).toEqual(["$1\\n"]);
  });

  it("caps the query and stored matches", () => {
    const long = "a".repeat(MAX_FIND_QUERY_LENGTH + 1);
    expect(find(long, long).ranges).toEqual([]);

    const result = find("x ".repeat(MAX_FIND_MATCHES + 1), "x");
    expect(result.ranges).toHaveLength(MAX_FIND_MATCHES * 2);
    expect(result.truncated).toBe(true);
    const lines = find("x\n".repeat(MAX_FIND_MATCHES + 1), "^x", REGEX);
    expect(lines.ranges).toHaveLength(MAX_FIND_MATCHES * 2);
    expect(lines.truncated).toBe(true);
  });

  it("accepts the 1 MiB editor ceiling and rejects plus one before matching", () => {
    const exactAscii = `${"a".repeat(MAX_EDITABLE_CONTENT_BYTES - 1)}z`;
    expect(find(exactAscii, "z")).toEqual({
      ranges: [MAX_EDITABLE_CONTENT_BYTES - 1, MAX_EDITABLE_CONTENT_BYTES],
      truncated: false,
      contentLimitExceeded: false,
    });
    expect(find(`${exactAscii}z`, "z")).toEqual({
      ranges: [],
      truncated: false,
      contentLimitExceeded: true,
    });

    const exactUtf8 = "é".repeat(MAX_EDITABLE_CONTENT_BYTES / 2);
    expect(find(exactUtf8, "z")).toMatchObject({
      contentLimitExceeded: false,
    });
    expect(find(`${exactUtf8}é`, "z").contentLimitExceeded).toBe(true);
  });
});

class FakeWorker {
  static spawned: FakeWorker[] = [];
  onmessage: ((event: MessageEvent) => void) | null = null;
  posted: unknown[] = [];
  terminated = false;
  constructor() {
    FakeWorker.spawned.push(this);
  }
  postMessage(message: unknown) {
    this.posted.push(message);
  }
  onerror: (() => void) | null = null;
  terminate() {
    this.terminated = true;
  }
  reply(data: unknown) {
    this.onmessage?.({ data } as MessageEvent);
  }
}

describe("createFindWorkerClient", () => {
  afterEach(() => {
    vi.useRealTimers();
    FakeWorker.spawned = [];
  });
  const spawn = () => new FakeWorker() as unknown as Worker;
  const request = { content: "aaaa", query: "(a+)+$", options: REGEX };

  it("stops a pattern that runs past the deadline and starts afresh", async () => {
    vi.useFakeTimers();
    const client = createFindWorkerClient(spawn);
    const result = client.run(request);
    vi.advanceTimersByTime(REGEX_FIND_TIMEOUT_MS);
    await expect(result).resolves.toMatchObject({
      ranges: [],
      error: "timeout",
    });
    expect(FakeWorker.spawned[0]!.terminated).toBe(true);

    const next = client.run(request);
    expect(FakeWorker.spawned).toHaveLength(2);
    FakeWorker.spawned[1]!.reply({ ranges: [0, 4] });
    await expect(next).resolves.toEqual({ ranges: [0, 4] });
    expect(vi.getTimerCount()).toBe(0);
  });

  it("reuses one worker and keeps only the latest waiting request", async () => {
    vi.useFakeTimers();
    const client = createFindWorkerClient(spawn);
    const worker = () => FakeWorker.spawned[0]!;
    const first = client.run({ ...request, query: "1" });
    const second = client.run({ ...request, query: "2" });
    const third = client.run({ ...request, query: "3" });
    await expect(second).resolves.toBeNull();
    expect(worker().posted).toHaveLength(1);

    worker().reply({ ranges: [1] });
    await expect(first).resolves.toEqual({ ranges: [1] });
    expect(worker().posted).toMatchObject([{ query: "1" }, { query: "3" }]);
    worker().reply({ ranges: [3] });
    await expect(third).resolves.toEqual({ ranges: [3] });
    expect(FakeWorker.spawned).toHaveLength(1);
    expect(worker().terminated).toBe(false);

    const slow = client.run(request);
    const queued = client.run(request);
    vi.advanceTimersByTime(REGEX_FIND_TIMEOUT_MS);
    await expect(slow).resolves.toMatchObject({ error: "timeout" });
    expect(FakeWorker.spawned).toHaveLength(2);
    client.dispose();
    await expect(queued).resolves.toBeNull();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("answers at once when a worker cannot start or load", async () => {
    vi.useFakeTimers();
    const refused = createFindWorkerClient(() => {
      throw new Error("blocked");
    });
    await expect(refused.run(request)).resolves.toMatchObject({
      error: "unavailable",
    });

    const client = createFindWorkerClient(spawn);
    const result = client.run(request);
    FakeWorker.spawned[0]!.onerror?.();
    await expect(result).resolves.toMatchObject({ error: "unavailable" });
    expect(FakeWorker.spawned[0]!.terminated).toBe(true);
    expect(vi.getTimerCount()).toBe(0);
  });
});
