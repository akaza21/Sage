import { describe, expect, it } from "vitest";
import type { AcpChatPage, HandoffSummary } from "../../../domain/acp";
import { sameTurnHandoffs } from "./turnHandoffs";

const handoff = (
  turnId: string,
  state: HandoffSummary["state"] = "inline",
): HandoffSummary => ({
  id: `handoff-${turnId}`,
  revision: "4",
  targetSession: "session",
  turnId,
  state,
  carriedItems: 2,
  carriedBytes: 300,
  omissions: { unsupportedKind: 0, oversize: 0, scanLimit: 0, budget: 0 },
});
const page = (handoffs: HandoffSummary[]) =>
  ({ handoffs }) as unknown as AcpChatPage;

describe("sameTurnHandoffs", () => {
  it("keeps a Turn memoized when only a fresh but equal handoff array arrives", () => {
    expect(
      sameTurnHandoffs(
        page([handoff("turn-1"), handoff("turn-2")]),
        page([handoff("turn-1"), handoff("turn-2", "unknown")]),
        "turn-1" as never,
      ),
    ).toBe(true);
  });

  it("invalidates only the Turn whose handoff changed", () => {
    expect(
      sameTurnHandoffs(
        page([handoff("turn-2", "pending")]),
        page([handoff("turn-2", "unknown")]),
        "turn-2" as never,
      ),
    ).toBe(false);
    expect(
      sameTurnHandoffs(page([]), page([handoff("turn-3")]), "turn-3" as never),
    ).toBe(false);
  });
});
