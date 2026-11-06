import { test } from "./shared/test";

import { registerAgentTurnActionScenarios } from "./fixtures/agentTurnActionScenarios";
import { registerAgentTurnContinuityScenarios } from "./fixtures/agentTurnContinuityScenarios";
import { disposeJourney } from "./fixtures/agentTurnJourneyBrowser";

test.afterEach(async ({ page }) => {
  await disposeJourney(page);
});

test.describe("AC-01 browser-proof Turn journeys", () => {
  test.use({ viewport: { width: 1380, height: 880 } });
  registerAgentTurnActionScenarios();
  registerAgentTurnContinuityScenarios();
});
