import { afterEach, describe, expect, it } from "vitest";

import {
  disposeAgentTurnJourneyFixture,
  installAgentTurnJourneyFixture,
  journeyAudit,
  runJourneyEvent,
} from "./agentTurnJourneyFixture";
import { MAX_SCRIPTED_EVENTS } from "./agentTurnJourneyLimits";
import {
  appendTurn,
  initialPage,
  updateLatestTurn,
} from "./agentTurnJourneyModel";
import { journeyRailStatus } from "./agentTurnJourneySnapshot";

describe("agent Turn journey fixture bounds", () => {
  it("projects rail receipts from the same authoritative Turn lifecycle", () => {
    const idle = initialPage("beta");
    expect(journeyRailStatus(idle)).toBe("idle");
    const running = appendTurn(idle, "Background completion", "running");
    expect(journeyRailStatus(running)).toBe("working");
    expect(
      journeyRailStatus(
        updateLatestTurn(
          running,
          "completed",
          "Background result is complete.",
        ),
      ),
    ).toBe("complete");
    expect(
      journeyRailStatus(updateLatestTurn(running, "deliveryUnknown")),
    ).toBe("failed");
  });
  afterEach(async () => {
    await disposeAgentTurnJourneyFixture();
  });

  it("keeps the real event ledger at 40 when event 41 is rejected", async () => {
    await installAgentTurnJourneyFixture();
    for (let index = 0; index < MAX_SCRIPTED_EVENTS; index += 1) {
      await runJourneyEvent({ type: "noop" });
    }
    await expect(runJourneyEvent({ type: "noop" })).rejects.toThrow(
      "Scripted event limit 40 exceeded",
    );

    const result = journeyAudit();
    expect(result.eventCount).toBe(MAX_SCRIPTED_EVENTS);
    expect(result.eventHistory).toEqual(
      Array.from({ length: MAX_SCRIPTED_EVENTS }, () => "noop:noop"),
    );
  });
});
