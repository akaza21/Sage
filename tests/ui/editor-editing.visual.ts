import { expect, test, type Page } from "@playwright/test";

import { openScenario } from "./shared/visualBrowserPrimitives";

// EP-03 editing essentials, driven through real key and mouse input in
// Chromium. The macOS chords differ only for fold and unfold.
const apple = process.platform === "darwin";

const SOURCE = [
  "function total(items) {",
  "  return items.length;",
  "}",
  "const item = total([item, item]);",
  "",
].join("\n");

async function openEditableFile(page: Page) {
  await page.setViewportSize({ width: 1380, height: 880 });
  const workbench = await openScenario(page, "editor", "editor");
  const content = page.locator(".cm-content");
  await content.click();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.insertText(SOURCE);
  // The editing features load on demand; the fold gutter shows they are in.
  await expect(page.locator(".cm-foldGutter")).toBeVisible();
  return { workbench, content };
}

const line = (page: Page, index: number) => page.locator(".cm-line").nth(index);

test.describe("Editor editing essentials", () => {
  test("auto-closes, matches brackets and indents with Tab", async ({
    page,
  }) => {
    const { content } = await openEditableFile(page);
    await page.keyboard.press("ControlOrMeta+End");
    await page.keyboard.type("log(");
    await expect(line(page, 4)).toHaveText("log()");
    await page.keyboard.type('"');
    await expect(line(page, 4)).toHaveText('log("")');
    await page.keyboard.press("Backspace");
    await expect(line(page, 4)).toHaveText("log()");
    await expect(page.locator(".cm-matchingBracket")).toHaveCount(2);

    await page.keyboard.press("Home");
    await page.keyboard.press("Tab");
    await expect(line(page, 4)).toHaveText("  log()");
    await expect(content).toBeFocused();
    await page.keyboard.press("Shift+Tab");
    await expect(line(page, 4)).toHaveText("log()");

    // Esc then Tab leaves the editor without typing a tab.
    await page.keyboard.press("Escape");
    await page.keyboard.press("Tab");
    await expect(content).not.toBeFocused();
    await expect(line(page, 4)).toHaveText("log()");
  });

  test("selects next occurrences, adds cursors and toggles comments", async ({
    page,
  }) => {
    await openEditableFile(page);
    // Select the first "item" on line 4, then add the next two.
    await line(page, 3).getByText("const item", { exact: false }).click();
    await page.keyboard.press("Home");
    await page.keyboard.press("ControlOrMeta+ArrowRight");
    await page.keyboard.press("ControlOrMeta+ArrowRight");
    await page.keyboard.press("ControlOrMeta+Shift+ArrowLeft");
    await expect(page.locator(".cm-selectionMatch").first()).toBeVisible();
    await page.keyboard.press("ControlOrMeta+d");
    await page.keyboard.press("ControlOrMeta+d");
    await page.keyboard.type("x");
    await expect(line(page, 3)).toHaveText("const x = total([x, x]);");

    // Add a cursor below and type on both lines.
    await line(page, 0).click();
    await page.keyboard.press("Home");
    await page.keyboard.press("ControlOrMeta+Alt+ArrowDown");
    await expect(page.locator(".cm-cursor")).toHaveCount(2);
    await page.keyboard.type("#");
    await expect(line(page, 0)).toHaveText("#function total(items) {");
    await expect(line(page, 1)).toHaveText("#  return items.length;");
    await page.keyboard.press("Escape");
    await page.keyboard.press("ControlOrMeta+z");

    // Alt+click adds a cursor (the default multi-cursor modifier). With
    // several cursors only CodeMirror's drawn selection paints.
    await line(page, 0).getByText("total").dblclick();
    await line(page, 2).click({ modifiers: ["Alt"] });
    await expect(page.locator(".cm-cursor")).toHaveCount(2);
    await expect(page.locator(".cm-selectionBackground")).toHaveCount(1);
    expect(
      await line(page, 0).evaluate(
        (element) => getComputedStyle(element, "::selection").backgroundColor,
      ),
    ).toBe("rgba(0, 0, 0, 0)");
    await page.keyboard.press("Escape");

    await line(page, 3).click();
    await page.keyboard.press("ControlOrMeta+/");
    await expect(line(page, 3)).toHaveText("// const x = total([x, x]);");
    await page.keyboard.press("ControlOrMeta+/");
    await expect(line(page, 3)).toHaveText("const x = total([x, x]);");
    await page.keyboard.press("Home");
    await page.keyboard.press("Shift+End");
    await page.keyboard.press("Shift+Alt+a");
    await expect(line(page, 3)).toHaveText("/* const x = total([x, x]); */");
  });

  test("folds from the keyboard and the gutter, and Settings switch it off", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");
    await expect(page.locator(".cm-foldGutter")).toBeVisible();
    const lines = page.locator(".cm-line");
    await expect(lines).toHaveCount(9);
    await line(page, 2).click();
    await page.keyboard.press("End");
    await page.keyboard.press(
      apple ? "Meta+Alt+BracketLeft" : "Control+Shift+BracketLeft",
    );
    await expect(page.locator(".cm-foldPlaceholder")).toHaveCount(1);
    await expect(lines).toHaveCount(3);
    await page.keyboard.press(
      apple ? "Meta+Alt+BracketRight" : "Control+Shift+BracketRight",
    );
    await expect(page.locator(".cm-foldPlaceholder")).toHaveCount(0);
    await expect(lines).toHaveCount(9);

    // The gutter arrow folds too; it shows while the pointer is over it.
    // CodeMirror keeps a hidden spacer marker in the gutter to size it.
    const arrows = page
      .locator(".cm-foldGutter .cm-foldMarker")
      .filter({ visible: true });
    await page.locator(".cm-foldGutter").hover();
    await expect(arrows.first()).toHaveCSS("opacity", "1");
    await expect(arrows).toHaveCount(1);
    await arrows.first().click();
    await expect(page.locator(".cm-foldPlaceholder")).toHaveCount(1);
    await expect(lines).toHaveCount(3);
    await line(page, 2).click();
    await page.keyboard.press("End");
    await expect(page.locator(".cm-matchingBracket")).toHaveCount(2);
    await expect(page.locator(".cm-foldPlaceholder")).toHaveCount(1);
    await expect(workbench).toHaveScreenshot("editor-editing-marks.png");

    await page.getByRole("button", { name: "Open settings" }).click();
    const settings = page.getByRole("region", { name: "Settings" });
    await settings.getByRole("switch", { name: "Code folding" }).uncheck();
    await settings
      .getByRole("switch", { name: "Auto-close brackets" })
      .uncheck();
    await page.getByRole("button", { name: "Close Settings" }).click();
    await expect(page.locator(".cm-foldGutter")).toHaveCount(0);
    await expect(page.locator(".cm-foldPlaceholder")).toHaveCount(0);
    // Settings round-trips preserve the seeded File on the browser disk.
    await expect(lines).toHaveCount(9);
    await expect(line(page, 2)).toHaveText(
      "export function AgentChatPanes() {",
    );
    await page.locator(".cm-content").click();
    await page.keyboard.press("ControlOrMeta+End");
    await page.keyboard.type("(");
    await expect(line(page, 8)).toHaveText("}(");
  });

  // CodeMirror reads the platform when it loads; on macOS Shift+Option+A
  // types "Å", so the chord must go by the physical key.
  test("toggles a block comment from Shift+Option+A on macOS", async ({
    page,
  }) => {
    await page.addInitScript(() => {
      Object.defineProperty(Navigator.prototype, "platform", {
        get: () => "MacIntel",
      });
    });
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "editor", "editor");
    await expect(page.locator(".cm-foldGutter")).toBeVisible();
    await line(page, 3).click();
    await page.keyboard.press("Home");
    await page.keyboard.press("Shift+End");
    const typed = await page.locator(".cm-content").evaluate((content) =>
      content.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Å",
          code: "KeyA",
          keyCode: 65,
          shiftKey: true,
          altKey: true,
          bubbles: true,
          cancelable: true,
        }),
      ),
    );
    expect(typed).toBe(false);
    await expect(line(page, 3)).toHaveText(
      "  /* const visibleChats = useVisibleChats({ maximum: 2 }); */",
    );
  });
});
