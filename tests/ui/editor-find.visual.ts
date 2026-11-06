import { expect, test, type Page } from "@playwright/test";

import { expectFindContained } from "./editorVisualHelpers";
import {
  expectNoViewportOverflow,
  openScenario,
} from "./shared/visualBrowserPrimitives";

// EP-04 Find and Replace, driven through real keys in Chromium. Replace is
// Cmd+Option+F and Replace all Cmd+Enter on macOS, as in VS Code.
const apple = process.platform === "darwin";
const REPLACE = apple ? "Meta+Alt+f" : "Control+h";
const REPLACE_ALL = apple ? "Meta+Enter" : "Control+Alt+Enter";
const FIND_NEXT = apple ? "Meta+g" : "F3";

const SOURCE = [
  "const total = count(items);",
  "const label = count(names);",
  "const width = count(columns);",
  // (a+)+$ backtracks exponentially on this line.
  `${"a".repeat(36)}!`,
  "",
].join("\n");

async function openFile(page: Page) {
  await page.setViewportSize({ width: 1380, height: 880 });
  const workbench = await openScenario(page, "editor", "editor");
  const content = page.locator(".cm-content");
  await content.click();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.insertText(SOURCE);
  await page.keyboard.press("ControlOrMeta+Home");
  return { workbench, content };
}

const line = (page: Page, index: number) => page.locator(".cm-line").nth(index);

test.describe("Editor Find and Replace", () => {
  test("finds with options, replaces one and all, and undoes Replace all in one step", async ({
    page,
  }) => {
    const { workbench, content } = await openFile(page);
    await page.keyboard.press(REPLACE);
    const find = page.getByRole("search", { name: "Find in file" });
    const query = find.getByRole("textbox", { name: "Find text" });
    const replacement = find.getByRole("textbox", { name: "Replace text" });
    const status = find.getByRole("status");
    await expect(query).toBeFocused();
    await query.fill("count");
    await expect(status).toHaveText("1 of 3 matches");
    await expect(page.locator(".cm-findMatch")).toHaveCount(3);
    await expect(page.locator(".cm-findMatch--current")).toHaveCount(1);

    // Whole word and regex from the keyboard, as in VS Code.
    await query.fill("count\\((\\w+)\\)");
    await expect(status).toHaveText("No matches");
    await query.press(apple ? "Meta+Alt+r" : "Alt+r");
    await expect(
      find.getByRole("button", { name: "Use regular expression" }),
    ).toHaveAttribute("aria-pressed", "true");
    await expect(status).toHaveText("1 of 3 matches");
    await query.press(apple ? "Meta+Alt+w" : "Alt+w");
    await expect(status).toHaveText("1 of 3 matches");
    await query.press(apple ? "Meta+Alt+w" : "Alt+w");

    await replacement.fill("size($1)");
    await expectFindContained(page);
    await page.clock.runFor(1_000);
    await expect(workbench.getByRole("alert")).toHaveCount(0);
    await expect(workbench).toHaveScreenshot("editor-find-replace.png");
    await expect(workbench.getByRole("alert")).toHaveCount(0);

    await replacement.press("Enter");
    await expect(line(page, 0)).toHaveText("const total = size(items);");
    await expect(status).toHaveText("1 of 2 matches");
    await replacement.press(REPLACE_ALL);
    await expect(line(page, 1)).toHaveText("const label = size(names);");
    await expect(line(page, 2)).toHaveText("const width = size(columns);");
    await expect(status).toHaveText("No matches");

    await content.click();
    await page.keyboard.press("ControlOrMeta+z");
    await expect(line(page, 1)).toHaveText("const label = count(names);");
    await expect(line(page, 2)).toHaveText("const width = count(columns);");
    await expect(line(page, 0)).toHaveText("const total = size(items);");
    await page.clock.runFor(1_000);
    await expect(workbench.getByRole("alert")).toHaveCount(0);
  });

  test("stops a runaway pattern after 250 ms while typing stays live", async ({
    page,
  }) => {
    const { content } = await openFile(page);
    await page.keyboard.press("ControlOrMeta+f");
    const find = page.getByRole("search", { name: "Find in file" });
    const query = find.getByRole("textbox", { name: "Find text" });
    const status = find.getByRole("status");
    const regex = find.getByRole("button", { name: "Use regular expression" });
    if ((await regex.getAttribute("aria-pressed")) !== "true") {
      await regex.click();
    }
    // Long tasks on the page would mean the pattern ran on the main thread.
    await page.evaluate(() => {
      const state = window as unknown as { longTasks: number[] };
      state.longTasks = [];
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries())
          state.longTasks.push(entry.duration);
      }).observe({ type: "longtask" });
    });
    await query.fill("(a+)+$");
    await expect(status).toHaveText("Pattern stopped after 250 ms");
    await query.fill("a{3}!");
    await expect(status).toHaveText("1 of 1 match");
    const longTasks = await page.evaluate(
      () => (window as unknown as { longTasks: number[] }).longTasks,
    );
    expect(longTasks.filter((duration) => duration > 200)).toEqual([]);

    // Bad patterns name the problem, never the pattern.
    await query.fill("(?i:a)");
    await expect(status).toHaveText("Use Match case, not an inline flag");
    await query.fill("a*");
    await expect(status).toHaveText("Pattern matches empty text");
    await query.fill("(");
    await expect(status).toHaveText("Invalid pattern");

    // Typing in the File with the bar open leaves the caret where it is.
    await query.fill("const");
    await expect(status).toHaveText("1 of 3 matches");
    await line(page, 1).click();
    await page.keyboard.press("End");
    await page.keyboard.type(" // x");
    await expect(line(page, 1)).toHaveText("const label = count(names); // x");
    await expect(content).toBeFocused();
    await page.keyboard.press(FIND_NEXT);
    await expect(status).toHaveText("3 of 3 matches");
  });

  test("keeps the replace row inside a narrow editor", async ({ page }) => {
    await openFile(page);
    await page.setViewportSize({ width: 720, height: 560 });
    const hideSidebar = page.getByRole("button", { name: "Hide left sidebar" });
    if (await hideSidebar.isVisible()) await hideSidebar.click();
    await page.locator(".cm-content").click();
    await page.keyboard.press(REPLACE);
    const find = page.getByRole("search", { name: "Find in file" });
    await find.getByRole("textbox", { name: "Find text" }).fill("count");
    await expect(
      find.getByRole("textbox", { name: "Replace text" }),
    ).toBeVisible();
    await expectFindContained(page);
    expect(
      await find.evaluate((node) => {
        const box = node.getBoundingClientRect();
        return [...node.querySelectorAll("button, input")].every((control) => {
          const rect = control.getBoundingClientRect();
          return (
            rect.width > 0 &&
            rect.left >= box.left &&
            rect.right <= box.right &&
            rect.bottom <= box.bottom
          );
        });
      }),
    ).toBe(true);
    await expectNoViewportOverflow(page);
    await expect(find).toHaveScreenshot("editor-find-replace-narrow.png");
  });
});
