import { describe, expect, it } from "vitest";

import type { ChatMessage } from "../domain/workbench";
import { appendBrowserSimulatorTurn } from "./browserDesktop";

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
