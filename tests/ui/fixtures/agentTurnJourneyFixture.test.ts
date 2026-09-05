import { afterEach, describe, expect, it } from "vitest";

import {
  disposeAgentTurnJourneyFixture,
  installAgentTurnJourneyFixture,
  journeyAudit,
  runJourneyEvent,
} from "./agentTurnJourneyFixture";
import { MAX_SCRIPTED_EVENTS } from "./agentTurnJourneyLimits";

describe("agent Turn journey fixture bounds", () => {
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
