import { beforeEach, describe, expect, it } from "vitest";

import type { ChatMessage } from "../domain/workbench";
import {
  appendBrowserSimulatorTurn,
  browserDesktop,
  resetBrowserDesktopForTests,
} from "./browserDesktop";

function messages(count: number, text = "x"): ChatMessage[] {
  return Array.from({ length: count }, (_, index) => ({
    id: `message-${index}`,
    sessionId: "session",
    sequence: index + 1,
    role: "user",
    text,
    createdAt: 1,
  }));
}

const request = { sessionId: "session", modelId: "sim-fast", text: "hello" };

describe("browser simulator history bounds", () => {
  beforeEach(() => resetBrowserDesktopForTests());

  it("returns the authoritative simulator session for a Chat-addressed prompt", async () => {
    const snapshot = await browserDesktop.getBootstrapSnapshot();
    expect(snapshot.sessionId).toBeNull();
    const result = await browserDesktop.sendPrompt({
      chatId: snapshot.activeTabId!,
      harnessId: "sage-simulator",
      modelId: "sim-fast",
      text: "continue",
    });

    expect(result.sessionId).toEqual(expect.any(String));
    expect(result.messages.at(-1)).toMatchObject({
      sessionId: result.sessionId,
      role: "assistant",
    });
    expect((await browserDesktop.getBootstrapSnapshot()).sessionId).toBe(
      result.sessionId,
    );
  });

  it("exposes the four curated harnesses in product admission order", async () => {
    const snapshot = await browserDesktop.getBootstrapSnapshot();
    expect(snapshot.harnesses.slice(0, 4).map((harness) => harness.id)).toEqual(
      ["codex", "pi", "claude-code", "oh-my-pi"],
    );
  });

  it("mints a new simulator attempt when the selected model changes", async () => {
    expect(await browserDesktop.listHarnessModels("sage-simulator")).toEqual(
      expect.arrayContaining([expect.objectContaining({ id: "sim-deep" })]),
    );
    const first = await browserDesktop.sendPrompt({
      chatId: "tab-chat",
      harnessId: "sage-simulator",
      modelId: "sim-fast",
      text: "fast",
    });
    const second = await browserDesktop.sendPrompt({
      chatId: "tab-chat",
      harnessId: "sage-simulator",
      modelId: "sim-deep",
      text: "deep",
    });

    expect(second.sessionId).not.toBe(first.sessionId);
    expect(second.messages).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ sessionId: first.sessionId, text: "fast" }),
        expect.objectContaining({ sessionId: second.sessionId, text: "deep" }),
      ]),
    );
    expect((await browserDesktop.getBootstrapSnapshot()).selectedModelId).toBe(
      "sim-deep",
    );
  });

  it("rejects unadvertised browser harness and model identity before mutation", async () => {
    const before = await browserDesktop.getBootstrapSnapshot();
    await expect(
      browserDesktop.sendPrompt({
        chatId: "tab-chat",
        harnessId: "sage-simulator",
        modelId: "foreign-model",
        text: "must fail",
      }),
    ).rejects.toThrow("not available");
    expect(await browserDesktop.getBootstrapSnapshot()).toEqual(before);
  });

  it("exposes explicit transport and only admitted ACP binding identity", async () => {
    const snapshot = await browserDesktop.getBootstrapSnapshot();
    const [codex, pi, claude, omp, simulator] = snapshot.harnesses;
    expect(codex).toMatchObject({
      transport: "acp",
      bindingId: "codex-app-server",
    });
    expect([pi, claude, omp]).toEqual([
      expect.objectContaining({ transport: "acp", bindingId: null }),
      expect.objectContaining({ transport: "acp", bindingId: null }),
      expect.objectContaining({ transport: "acp", bindingId: null }),
    ]);
    expect(simulator).toMatchObject({
      transport: "legacySimulator",
      bindingId: null,
    });
  });

  it("accepts an exact 500-message projection then rejects a whole turn", () => {
    const exact = appendBrowserSimulatorTurn(
      messages(498),
      request,
      ["user", "assistant"],
      2,
    );
    expect(exact).toHaveLength(500);
    expect(() =>
      appendBrowserSimulatorTurn(exact, request, [
        "next-user",
        "next-assistant",
      ]),
    ).toThrow("existing messages were preserved");
    expect(exact).toHaveLength(500);
  });

  it("counts UTF-8 bytes and rejects before mutating the prior projection", () => {
    const existing = messages(1, "é".repeat(2 * 1024 * 1024));
    expect(() =>
      appendBrowserSimulatorTurn(existing, { ...request, text: "overflow" }, [
        "user",
        "assistant",
      ]),
    ).toThrow("capacity reached");
    expect(existing).toHaveLength(1);
  });
});
