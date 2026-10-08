import { expect, test } from "@playwright/test";

import { settleTranscriptAtEnd } from "./shared/visualBrowserPrimitives";
import {
  disposeJourney,
  openJourney,
  pane,
  runEvent,
} from "./fixtures/agentTurnJourneyBrowser";

test.afterEach(async ({ page }) => disposeJourney(page));

test("CP-17(c) refs 15/17: Jump to latest uses a reserved 560 px pane zone", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1936, height: 1048 });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openJourney(page);
  await page.addStyleTag({
    content:
      ".agent-chat-panes--split { grid-template-columns: 560px 8px 560px !important; justify-content: center; }",
  });
  await runEvent(page, { type: "seedHistory", chat: "alpha", turns: 12 });
  await settleTranscriptAtEnd(page);
  const alpha = pane(page, "Alpha");
  await expect(alpha).toHaveCSS("width", "560px");
  const transcript = alpha.locator(".message-list");
  const button = alpha.getByRole("button", { name: "Jump to latest" });
  const settleScroll = () =>
    page.evaluate(
      () =>
        new Promise<void>((resolve) =>
          requestAnimationFrame(() =>
            requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
          ),
        ),
    );

  await transcript.evaluate((node) => {
    node.scrollTop = node.scrollHeight - node.clientHeight * 2 + 1;
    node.dispatchEvent(new Event("scroll", { bubbles: true }));
  });
  await settleScroll();
  await expect(button).toHaveCount(0);

  await transcript.evaluate((node) => {
    node.scrollTop = node.scrollHeight - node.clientHeight * 2 - 1;
    node.dispatchEvent(new Event("scroll", { bubbles: true }));
  });
  await settleScroll();
  await expect(button).toBeVisible();
  const zone = alpha.locator(".chat-follow-end-zone");
  const composer = alpha.locator("form.composer");
  const [transcriptBox, zoneBox, buttonBox, composerBox] = await Promise.all([
    transcript.boundingBox(),
    zone.boundingBox(),
    button.boundingBox(),
    composer.boundingBox(),
  ]);
  expect(transcriptBox && zoneBox && buttonBox && composerBox).toBeTruthy();
  if (transcriptBox && zoneBox && buttonBox && composerBox) {
    expect(transcriptBox.y + transcriptBox.height).toBeLessThanOrEqual(
      zoneBox.y,
    );
    expect(buttonBox.y).toBeGreaterThanOrEqual(zoneBox.y);
    expect(buttonBox.y + buttonBox.height).toBeLessThanOrEqual(composerBox.y);
    expect(buttonBox.x + buttonBox.width / 2).toBeCloseTo(
      zoneBox.x + zoneBox.width / 2,
      0,
    );
  }
  await expect(alpha).toHaveScreenshot("cp17-jump-560.png");

  await button.click();
  await expect(button).toHaveCount(0);
  await expect
    .poll(() =>
      transcript.evaluate(
        (node) => node.scrollHeight - node.scrollTop - node.clientHeight,
      ),
    )
    .toBeLessThanOrEqual(1);
});
