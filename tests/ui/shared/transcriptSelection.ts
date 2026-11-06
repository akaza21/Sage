import { expect, type Page } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
  settleTranscriptAtEnd,
} from "./visualBrowserPrimitives";

export async function openSelectionScenario(page: Page, longAnswer = false) {
  await page.setViewportSize({ width: 1872, height: 1080 });
  const workbench = await openScenario(page, "rich", "agent");
  await settleChatProjections(page);
  await page.evaluate(async (long) => {
    const fixture = await sageImport(
      "/tests/ui/fixtures/transcriptSelectionFixtures.ts",
    );
    fixture.installTranscriptSelectionFixture(long);
  }, longAnswer);
  await settleTranscriptAtEnd(page);
  return workbench;
}

export async function dragAnswerPhrase(
  page: Page,
  phrase = "selected phrase",
  scroll = true,
) {
  const answer = page.locator("[data-selectable-answer]").first();
  await expect(answer).toContainText(phrase);
  if (scroll) await answer.scrollIntoViewIfNeeded();
  const rect = await answer.evaluate((element, text) => {
    const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
    while (walker.nextNode()) {
      const node = walker.currentNode as Text;
      const index = node.data.indexOf(text);
      if (index < 0) continue;
      const range = document.createRange();
      range.setStart(node, index);
      range.setEnd(node, index + text.length);
      const box = range.getBoundingClientRect();
      return { x: box.x, y: box.y, width: box.width, height: box.height };
    }
    throw new Error(`Missing selectable phrase: ${text}`);
  }, phrase);
  await page.mouse.move(rect.x + 1, rect.y + rect.height / 2);
  await page.mouse.down();
  await page.mouse.move(rect.x + rect.width + 1, rect.y + rect.height / 2, {
    steps: 14,
  });
  await page.mouse.up();
  await expect(
    page.getByRole("toolbar", { name: "Selection actions" }),
  ).toBeVisible();
  return rect;
}
