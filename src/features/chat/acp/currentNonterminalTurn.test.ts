import { describe, expect, it } from "vitest";

import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import { currentNonterminalTurn } from "./currentNonterminalTurn";
import { page } from "./acpProjection.fixture";

function withStrandedPredecessor(current: AcpChatPage): AcpChatPage {
  const [turn] = current.turns;
  const stranded = {
    ...turn,
    id: "turn-0",
    sessionId: "session-0",
    runtimeEpoch: "1",
    ordinal: 9,
    state: "running",
    terminalAt: null,
  } as unknown as TurnProjection;
  return { ...current, turns: [stranded, ...current.turns] };
}

describe("currentNonterminalTurn", () => {
  it("returns the highest-ordinal nonterminal turn of the current session", () => {
    expect(currentNonterminalTurn(page("running"))?.id).toBe("turn-1");
  });

  it("returns null when the current session has only terminal turns", () => {
    expect(currentNonterminalTurn(page("completed"))).toBeNull();
  });

  it("ignores a stranded nonterminal turn belonging to an earlier session", () => {
    expect(
      currentNonterminalTurn(withStrandedPredecessor(page("completed"))),
    ).toBeNull();
  });

  it("does not let a higher-ordinal stranded turn shadow the current session", () => {
    expect(
      currentNonterminalTurn(withStrandedPredecessor(page("running")))?.id,
    ).toBe("turn-1");
  });

  it("returns null when no session is current", () => {
    const orphaned = { ...page("running"), currentSessionId: null };
    expect(currentNonterminalTurn(orphaned)).toBeNull();
  });

  it("returns null for a missing page", () => {
    expect(currentNonterminalTurn(null)).toBeNull();
  });
});
