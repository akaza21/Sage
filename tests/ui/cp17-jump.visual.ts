import { expect, test } from "@playwright/test";

import { settleTranscriptAtEnd } from "./shared/visualBrowserPrimitives";
import {
  disposeJourney,
  openJourney,
  pane,
  runEvent,
} from "./fixtures/agentTurnJourneyBrowser";

test.afterEach(async ({ page }) => disposeJourney(page));

test("CP-17(c) refs 15/17: Jump to latest floats over a 560 px pane column", async ({
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
  const composer = alpha.locator("form.composer");
  const [transcriptBox, buttonBox, composerBox] = await Promise.all([
    transcript.boundingBox(),
    button.boundingBox(),
    composer.boundingBox(),
  ]);
  expect(transcriptBox && buttonBox && composerBox).toBeTruthy();
  if (transcriptBox && buttonBox && composerBox) {
    // CP-47 (Codex s2/f0001): the control overlays the transcript's bottom
    // edge instead of reserving a row, centred on the reading column.
    expect(buttonBox.y).toBeGreaterThanOrEqual(transcriptBox.y);
    expect(buttonBox.y + buttonBox.height).toBeLessThanOrEqual(
      transcriptBox.y + transcriptBox.height - 16,
    );
    expect(buttonBox.y + buttonBox.height).toBeLessThan(composerBox.y);
    const column = await alpha.locator(".message-list__content").boundingBox();
    expect(
      Math.abs(
        buttonBox.x + buttonBox.width / 2 - (column!.x + column!.width / 2),
      ),
    ).toBeLessThanOrEqual(2);
  }
  await expect(alpha).toHaveScreenshot("cp17-jump-560.png");

  await button.click();
  await expect(button).toBeHidden();
  await expect
    .poll(() =>
      transcript.evaluate(
        (node) => node.scrollHeight - node.scrollTop - node.clientHeight,
      ),
    )
    .toBeLessThanOrEqual(1);
});
