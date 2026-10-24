import { expect, test } from "@playwright/test";
import {
  dragAnswerPhrase,
  openSelectionScenario,
} from "./shared/transcriptSelection";

test.use({ deviceScaleFactor: 1 });
test("CP-40: mouse-up selection actions match geometry and dismiss on Escape", async ({
  page,
}) => {
  const workbench = await openSelectionScenario(page);
  const selected = await dragAnswerPhrase(page);
  const toolbar = page.getByRole("toolbar", { name: "Selection actions" });
  await expect(
    toolbar.getByRole("button", { name: "Add to chat", exact: true }),
  ).toBeVisible();
  await expect(
    toolbar.getByRole("button", { name: "Ask in side chat", exact: true }),
  ).toBeVisible();
  const bounds = (await toolbar.boundingBox())!;
  expect(bounds.y + bounds.height).toBeLessThanOrEqual(selected.y - 4);
  expect(bounds.y + bounds.height).toBeGreaterThanOrEqual(selected.y - 8);
  expect(
    Math.abs(bounds.x + bounds.width / 2 - selected.x),
  ).toBeLessThanOrEqual(4);
  await expect(workbench).toHaveScreenshot("selection-popover.png");
  await page.keyboard.press("Escape");
  await expect(toolbar).toBeHidden();
});

test("CP-40: keyboard menu, focus return, scroll dismissal and reduced motion", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openSelectionScenario(page);
  await dragAnswerPhrase(page);
  await page.keyboard.press("Escape");
  const answer = page.locator("[data-selectable-answer]").first();
  await answer.focus();
  await page.keyboard.press("Shift+F10");
  await expect(
    page.getByRole("button", { name: "Add to chat", exact: true }),
  ).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(answer).toBeFocused();
  await page.keyboard.press("Shift+F10");
  await page.locator(".message-list").first().dispatchEvent("scroll");
  await expect(
    page.getByRole("toolbar", { name: "Selection actions" }),
  ).toBeHidden();
});

test("CP-40: toolbar flips below a selection at the scrollport edge", async ({
  page,
}) => {
  const workbench = await openSelectionScenario(page, true);
  await page
    .locator("[data-selectable-answer]")
    .first()
    .evaluate((answer) => {
      const scrollport = answer.closest<HTMLElement>(".message-list")!;
      scrollport.scrollTop +=
        answer.getBoundingClientRect().top -
        scrollport.getBoundingClientRect().top -
        10;
    });
  const selected = await dragAnswerPhrase(page, "selected phrase", false);
  const toolbar = (await page
    .getByRole("toolbar", { name: "Selection actions" })
    .boundingBox())!;
  expect(toolbar.y).toBeGreaterThanOrEqual(selected.y + selected.height + 4);
  expect(toolbar.y).toBeLessThanOrEqual(selected.y + selected.height + 8);
  await expect(workbench).toHaveScreenshot("selection-popover-flipped.png");
});
