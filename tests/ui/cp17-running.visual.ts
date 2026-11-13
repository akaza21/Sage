import { expect, test } from "@playwright/test";

import {
  waitForAnimations,
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

test("CP-17(b) refs 08/15/17/19: running header and chronological rows fit a 560 px pane", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1936, height: 1048 });
  await openScenario(page, "waiting", "agent");
  await expect(page.locator(".acp-chat-projection[data-chat-id]")).toHaveCount(
    1,
  );
  await settleChatProjections(page);
  await page.evaluate(async () => {
    const [{ buildVisualScenario }, { acpProjectionStoreFor }, ids] =
      await Promise.all([
        import("/tests/ui/fixtures/visualScenarioShell.ts"),
        import("/src/app/acpProjectionRegistry.ts"),
        import("/tests/ui/fixtures/visualFixtureIds.ts"),
      ]);
    const scenario = buildVisualScenario("waiting");
    const chatPage = scenario.pages[0];
    const turn = chatPage.turns[0];
    const prompt = turn.items[0];
    if (prompt.kind !== "text") throw new Error("Expected text prompt fixture");
    turn.state = "running";
    turn.items.push({
      ...prompt,
      id: "cp17-progress" as typeof prompt.id,
      ordinal: 4,
      payload: {
        role: "assistant",
        text: "Progress follows the live tool row.",
      },
    });
    acpProjectionStoreFor(chatPage.chatId).getState().applyAuthoritativePage(
      {
        projectId: ids.PROJECT_ID,
        projectGeneration: ids.GENERATION,
        chatId: chatPage.chatId,
      },
      chatPage,
    );
  });
  await page.addStyleTag({
    content:
      ".agent-chat-panes { grid-template-columns: 560px !important; justify-content: center; }",
  });

  const pane = page.locator(".agent-chat-pane");
  await expect(pane).toHaveCSS("width", "560px");
  const work = pane.locator(".chat-transcript-work");
  await expect(work).toHaveCount(1);
  const heading = work.locator(":scope > .chat-transcript-work__heading");
  await expect(heading).toHaveText("Working for 19m");
  // Timer callbacks still run, but capture timing must not age the fixture.
  await page.clock.runFor(1_100);
  await expect(heading).toHaveText("Working for 19m");
  await expect(work.locator(":scope > summary")).toHaveCount(0);
  const rows = work.locator(
    ".acp-tool-rollup__label, .acp-message--assistant .acp-markdown",
  );
  await expect(rows).toHaveText([
    "Search workspace",
    "Progress follows the live tool row.",
  ]);
  await waitForAnimations(rows.last());
  const headingBox = await work
    .locator(".chat-transcript-work__heading")
    .boundingBox();
  const firstBox = await rows.first().boundingBox();
  const proseBox = await rows.last().boundingBox();
  expect(headingBox && firstBox && proseBox).toBeTruthy();
  if (headingBox && firstBox && proseBox) {
    expect(headingBox.y + headingBox.height).toBeLessThanOrEqual(firstBox.y);
    expect(firstBox.y + firstBox.height).toBeLessThanOrEqual(proseBox.y);
    expect(headingBox.width).toBeLessThanOrEqual(560);
  }
  await expect(pane).toHaveScreenshot("cp17-running-560.png");
});
