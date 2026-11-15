import { describe, expect, it, vi } from "vitest";

import type { FileGitBase, ResourceRef } from "../../../domain/workbench";
import { createGutterBaseCache } from "./gitGutterBase";

const file = (relativePath: string): ResourceRef => ({
  rootId: "root",
  relativePath,
});

describe("gutter base cache", () => {
  it("asks with the blob it holds and reuses its text", async () => {
    const read = vi.fn(
      async (
        _resource: ResourceRef,
        known: string | null,
      ): Promise<FileGitBase> =>
        known === "oid-a"
          ? { state: "unchanged", oid: "oid-a" }
          : { state: "ready", oid: "oid-a", content: "text" },
    );
    const cache = createGutterBaseCache(read);
    expect(await cache.load(file("a"))).toBe("text");
    expect(await cache.load(file("a"))).toBe("text");
    expect(read.mock.calls.map((call) => call[1])).toEqual([null, "oid-a"]);
  });

  it("evicts the least recently used text past its byte cap", async () => {
    const contents: Record<string, string> = {
      a: "a".repeat(10),
      b: "b".repeat(10),
      c: "c".repeat(10),
    };
    const read = vi.fn(
      async (
        resource: ResourceRef,
        known: string | null,
      ): Promise<FileGitBase> =>
        known === resource.relativePath
          ? { state: "unchanged", oid: known }
          : {
              state: "ready",
              oid: resource.relativePath,
              content: contents[resource.relativePath]!,
            },
    );
    // Room for two 10-character texts.
    const cache = createGutterBaseCache(read, 40);
    await cache.load(file("a"));
    await cache.load(file("b"));
    await cache.load(file("a"));
    await cache.load(file("c"));
    expect(cache.bytes()).toBe(40);
    read.mockClear();
    await cache.load(file("b"));
    expect(read.mock.calls[0]![1]).toBeNull();
  });

  it("forgets a File that has no staged text", async () => {
    const read = vi.fn(async (): Promise<FileGitBase> => ({
      state: "unavailable",
    }));
    const cache = createGutterBaseCache(read);
    expect(await cache.load(file("new.txt"))).toBeNull();
  });
});
