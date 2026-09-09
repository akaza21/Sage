import { createHash, webcrypto } from "node:crypto";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { GetAgentDiffRequest } from "../domain/agentDiff";
import { parseAgentDiffSnapshot } from "./acpFileSnapshot";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { acpDesktop } from "./acpDesktop";

function fixture(oldText = "old", newText = "new") {
  const old = Buffer.from(oldText),
    next = Buffer.from(newText);
  const oldLength = Buffer.alloc(8),
    newLength = Buffer.alloc(8);
  oldLength.writeBigUInt64BE(BigInt(old.length));
  newLength.writeBigUInt64BE(BigInt(next.length));
  const contentDigest = createHash("sha256")
    .update("sage-file-diff-v1\0")
    .update(oldLength)
    .update(old)
    .update(newLength)
    .update(next)
    .digest("hex");
  const request = {
    projectId: "project",
    chatId: "chat",
    sessionId: "session",
    runtimeEpoch: "1",
    turnId: "turn",
    itemId: "item",
    diffOrdinal: 0,
    snapshotRevision: "1",
    rootId: "root",
    relativePath: "src/a.ts",
    contentDigest,
    projectGeneration: "0",
    expectedProjectionRevision: "7",
  } as GetAgentDiffRequest;
  return {
    request,
    response: {
      ...request,
      projectionRevision: "7",
      oldText,
      newText,
      oldBytes: old.length,
      newBytes: next.length,
    },
  };
}

beforeEach(() => vi.stubGlobal("crypto", webcrypto));
afterEach(() => {
  vi.unstubAllGlobals();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  invoke.mockReset();
});

describe("bounded immutable snapshot desktop response", () => {
  it("uses only the narrow native snapshot command with sanitized metadata", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    const { request, response } = fixture();
    invoke.mockResolvedValue(response);
    const result = await acpDesktop.getFileSnapshot({
      ...request,
      oldText: "must not leave renderer",
    } as GetAgentDiffRequest);
    expect(result.oldText).toBe("old");
    expect(invoke).toHaveBeenCalledExactlyOnceWith("get_acp_file_snapshot", {
      request,
    });
    invoke.mockClear();
    await expect(
      acpDesktop.getFileSnapshot({ ...request, relativePath: "../secret" }),
    ).rejects.toThrow("Invalid immutable");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("returns the bytes actually hashed even if the transport object changes while hashing", async () => {
    const { request, response } = fixture();
    const pending = parseAgentDiffSnapshot(response, request);
    response.oldText = "bad";
    response.newText = "bad";
    const result = await pending;
    expect(result?.oldText).toBe("old");
    expect(result?.newText).toBe("new");
  });
  it("admits full raw sides including exact byte limits without clipping", async () => {
    const { request, response } = fixture(
      '"'.repeat(262144),
      "é".repeat(131072),
    );
    const value = await parseAgentDiffSnapshot(response, request);
    expect(value?.oldText).toBe(response.oldText);
    expect(value?.newText).toBe(response.newText);
    expect(value?.oldBytes).toBe(262144);
    expect(value).not.toHaveProperty("projectGeneration");
  });

  it("rejects each owner, revision, resource and digest mismatch", async () => {
    const { request, response } = fixture();
    for (const field of [
      "projectId",
      "chatId",
      "sessionId",
      "runtimeEpoch",
      "turnId",
      "itemId",
      "snapshotRevision",
      "rootId",
      "relativePath",
      "projectionRevision",
      "contentDigest",
    ] as const) {
      expect(
        await parseAgentDiffSnapshot(
          { ...response, [field]: "different" },
          request,
        ),
      ).toBeNull();
    }
    expect(
      await parseAgentDiffSnapshot({ ...response, diffOrdinal: 1 }, request),
    ).toBeNull();
  });

  it("rejects byte-length lies, corrupted bytes and either raw-side plus one", async () => {
    const { request, response } = fixture();
    expect(
      await parseAgentDiffSnapshot({ ...response, oldText: "bad" }, request),
    ).toBeNull();
    expect(
      await parseAgentDiffSnapshot({ ...response, newBytes: 2 }, request),
    ).toBeNull();
    for (const [old, next] of [
      ["x".repeat(262145), ""],
      ["", "x".repeat(262145)],
    ]) {
      const value = fixture(old, next);
      expect(
        await parseAgentDiffSnapshot(value.response, value.request),
      ).toBeNull();
    }
  });
});
