import { expect, test } from "@playwright/test";

import { openScenario } from "./shared/visualBrowserPrimitives";

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
  await expect(pill).toContainText(/\d+ files? changed/);
  await expect(pill).toHaveAttribute("aria-expanded", "false");
  const box = await pill.boundingBox();
  expect(box && box.width < 560).toBeTruthy();
  await expect(pane).toHaveScreenshot("cp17-edit-pill-560.png");
  await pill.click();
  await expect(pill).toHaveAttribute("aria-expanded", "true");
  await expect(pane.locator(".acp-file-changes li").first()).toBeVisible();
  await expect(pane).toHaveScreenshot("cp17-edits-560.png");
});
