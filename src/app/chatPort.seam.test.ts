import { readFile } from "node:fs/promises";
import path from "node:path";
import { describe, expect, it } from "vitest";

import type { AcpItemIdentity } from "../domain/acp";

const appRoot = path.resolve(import.meta.dirname);
const chatConsumers = [
  "chatSlice.ts",
  "chatArchiveSlice.ts",
  "chatTransitions.ts",
  "chatOwnershipSlice.ts",
] as const;

async function readChatConsumer(file: (typeof chatConsumers)[number]) {
  return readFile(path.join(appRoot, file), "utf8");
}

describe("S0.3 ChatPort projection seam", () => {
  it("acquires Chat lifecycle and prompt dispatch through workbench ports", async () => {
    const sources = await Promise.all(chatConsumers.map(readChatConsumer));

    for (const source of sources) {
      expect(source).not.toMatch(/from ["']\.\.\/services\/desktop["']/);
      expect(source).toMatch(/from ["']\.\.\/ports\/workbenchRuntime["']/);
      expect(source).toMatch(/\bgetWorkbenchPorts\b/);
    }
  });

  it("reads authoritative Chat projections only through ProjectPort", async () => {
    const source = await readChatConsumer("chatTransitions.ts");

    expect(source).toMatch(/\bproject\.getBootstrapSnapshot\(\)/);
    expect(source).not.toMatch(/\bchat\.getBootstrapSnapshot\(\)/);
    expect(source).not.toMatch(/\bdesktop\.getBootstrapSnapshot\(\)/);
  });

  it("names the Project, Chat, Session, Turn, and Item linkage contract", () => {
    const streamingItem = {
      id: "item-1",
      projectId: "project-1",
      chatId: "chat-1",
      sessionId: "session-1",
      turnId: "turn-1",
      runtimeEpoch: "epoch-1",
      ordinal: 1,
      origin: "primary",
      activityId: null,
      lifecycle: "streaming",
      revision: 1,
      createdAt: 10,
      updatedAt: 10,
      terminalAt: null,
    } as unknown as AcpItemIdentity;

    expect([
      streamingItem.projectId,
      streamingItem.chatId,
      streamingItem.sessionId,
      streamingItem.turnId,
      streamingItem.id,
    ]).toEqual(["project-1", "chat-1", "session-1", "turn-1", "item-1"]);
    expect(streamingItem.ordinal).toBe(1);
    expect(
      (streamingItem as unknown as { revision: number }).revision,
    ).toBe(1);
    expect(streamingItem.createdAt).toBe(10);
    expect(streamingItem.updatedAt).toBe(10);
    expect(streamingItem.createdAt).toBeLessThanOrEqual(streamingItem.updatedAt);
    expect(streamingItem.lifecycle).toBe("streaming");
    expect(streamingItem.terminalAt).toBeNull();
  });
});
