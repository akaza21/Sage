import { expect, test } from "./shared/test";

import {
  waitForAnimations,
  openScenario,
} from "./shared/visualBrowserPrimitives";

test("CP-17(e) refs 07/13/17: completed edit pill expands within a 560 px chat pane", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1936, height: 1048 });
  await openScenario(page, "agent-single", "agent");
  await page.addStyleTag({
    content:
      ".agent-chat-panes { grid-template-columns: 560px !important; justify-content: center; }",
  });
  const pane = page.locator(".agent-chat-pane");
  await expect(pane).toHaveCSS("width", "560px");
  const pill = pane.locator(".acp-file-changes__header").first();
  const summary = pane.locator(".chat-edit-summary-zone");
  await expect(pill).toContainText(/\d+ files? changed/);
  await expect(pill).toHaveAttribute("aria-expanded", "false");
  await waitForAnimations(summary);
  const box = await pill.boundingBox();
  const collapsedHeight = (await summary.boundingBox())!.height;
  expect(box && box.width < 560).toBeTruthy();
  await expect(pane).toHaveScreenshot("cp17-edit-pill-560.png");
  await pill.click();
  await expect(pill).toHaveAttribute("aria-expanded", "true");
  await expect
    .poll(async () => (await summary.boundingBox())!.height)
    .toBeGreaterThan(collapsedHeight + 32);
  await expect(pane.locator(".acp-file-changes li").first()).toBeVisible();
  await expect(pane).toHaveScreenshot("cp17-edits-560.png");
});
