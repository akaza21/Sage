import { expect, type Page } from "@playwright/test";
import { openJourney, pane, settleRender } from "./agentTurnJourneyBrowser";
import { settleTranscriptAtEnd } from "../shared/visualBrowserPrimitives";

export async function openNavigationChat(
  page: Page,
  turns = 6,
  incomplete = false,
  longAnswer = false,
) {
  await page.setViewportSize({ width: 1936, height: 1048 });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openJourney(page);
  await page.addStyleTag({
    content: ".agent-chat-panes { grid-template-columns: 760px !important; }",
  });
  await page.evaluate(
    async ({ count, incomplete, longAnswer }) => {
      if (count < 1 || count > 100)
        throw new Error("Navigation fixture is capped at 100 turns");
      const [
        { acpProjectionStoreFor },
        { appendTurn, updateLatestTurn, JOURNEY_IDS },
        { publishJourneyPage },
      ] = await Promise.all([
        import("/src/app/acpProjectionRegistry.ts"),
        import("/tests/ui/fixtures/agentTurnJourneyModel.ts"),
        import("/tests/ui/fixtures/agentTurnJourneyFixture.ts"),
      ]);
      const store = acpProjectionStoreFor(JOURNEY_IDS.alpha);
      const { page: initial } = store.getState();
      if (!initial) throw new Error("Journey projection not ready");
      let projection = initial;
      for (let index = 1; index <= count; index++) {
        projection = appendTurn(
          projection,
          `Navigator prompt ${index}`,
          "completed",
        );
        projection = updateLatestTurn(
          projection,
          "completed",
          `Needle turn ${index}.\n\n- **Finished** the requested work.\n- ${"Retained answer text. ".repeat(24)}`,
        );
      }
      if (incomplete)
        projection = { ...projection, nextCursor: "older-navigation-history" };
      if (longAnswer)
        projection = updateLatestTurn(
          projection,
          "completed",
          "prefix ".repeat(40_000) + "\n\n**Hidden tail needle**",
        );
      // Through the fixture backend, so a focus reload keeps this history.
      publishJourneyPage(projection);
    },
    { count: turns, incomplete, longAnswer },
  );
  await settleRender(page, true);
  await settleTranscriptAtEnd(page);
  const alpha = pane(page, "Alpha");
  await expect(alpha.locator(".acp-turn").last()).toBeVisible();
  return alpha;
}
