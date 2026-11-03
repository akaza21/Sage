import { expect, test } from "@playwright/test";
import {
  dragAnswerPhrase,
  openSelectionScenario,
} from "./shared/transcriptSelection";

test.use({ deviceScaleFactor: 1 });

for (const width of [1872, 720, 480]) {
  for (const split of [false, true]) {
    test(`CP-45 find leaves pane header controls unobscured at ${width} (split ${split})`, async ({
      page,
    }) => {
      await openSelectionScenario(page);
      if (split) {
        await dragAnswerPhrase(page);
        await page
          .getByRole("button", { name: "Ask in side chat", exact: true })
          .click();
        await expect(
          page.getByRole("button", { name: "Close side chat", exact: true }),
        ).toBeVisible();
      }
      await page.setViewportSize({ width, height: 1000 });
      const answer = page.locator("[data-selectable-answer]").first();
      // Too narrow to split, the panes show only the focused side chat; go
      // back to its parent Chat, whose header still carries the side tab.
      if (split && width < 1872) {
        await expect(answer).toBeHidden();
        if (width <= 600) await page.keyboard.press("Control+b");
        await page
          .getByRole("button", { name: /^Open chat Rich answers/ })
          .click();
        if (width <= 600) await page.keyboard.press("Control+b");
      }
      await answer.click();
      await page.keyboard.press("Control+f");
      await page
        .getByRole("searchbox", { name: "Search chat" })
        .fill("selected");
      const find = page.locator(".chat-find");
      await expect(find.locator("[role=status]")).toContainText("result");
      const box = (await find.boundingBox())!;
      const headerLocator = page.locator(".agent-chat-center__header");
      const header = (await headerLocator.boundingBox())!;
      expect(box.y).toBeGreaterThanOrEqual(header.y + header.height);
      const controls = headerLocator.locator("button, [role=status]");
      expect(await controls.count()).toBeGreaterThan(0);
      for (const control of await controls.all()) {
        if (!(await control.isVisible())) continue;
        const bounds = (await control.boundingBox())!;
        const overlaps =
          bounds.x < box.x + box.width &&
          bounds.x + bounds.width > box.x &&
          bounds.y < box.y + box.height &&
          bounds.y + bounds.height > box.y;
        expect(overlaps).toBe(false);
        expect(
          await control.evaluate((element) => {
            const bounds = element.getBoundingClientRect();
            const top = document.elementFromPoint(
              bounds.x + bounds.width / 2,
              bounds.y + bounds.height / 2,
            );
            return top === element || element.contains(top);
          }),
        ).toBe(true);
      }
      expect(box.height).toBeGreaterThanOrEqual(74);
      expect(box.height).toBeLessThanOrEqual(78);
      const results = (await find
        .locator(".chat-find__results")
        .boundingBox())!;
      expect(results.height).toBe(32);
      await expect(find.getByRole("button", { name: "Next match" })).toHaveCSS(
        "color",
        "rgb(137, 137, 137)",
      );
      expect(box.x + box.width).toBeLessThanOrEqual(width);
    });
  }
}
