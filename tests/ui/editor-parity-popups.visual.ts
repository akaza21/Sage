import { expect, test } from "./shared/test";

import {
  waitForAnimations,
  openScenario,
} from "./shared/visualBrowserPrimitives";

// Editor parity ticket 5: Quick Open placement and density, and the Editor
// menus' density, measured in Chromium against the browser runtime.
test.describe("Editor popups", () => {
  test("Quick Open opens over the title bar's Search files at VS Code's width", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "editor", "editor");
    await expect(page.locator(".cm-content")).toBeVisible();
    const search = page.locator(".title-bar__search");
    const searchBox = (await search.boundingBox())!;
    const dialog = page.getByRole("dialog", { name: "Quick Open" });

    await search.click();
    await expect(dialog).toBeVisible();
    await waitForAnimations(dialog);
    const wide = (await dialog.boundingBox())!;
    expect(wide.width).toBeCloseTo(600, 0);
    expect(Math.abs(wide.y - searchBox.y)).toBeLessThanOrEqual(1);
    expect(wide.x + wide.width / 2).toBeCloseTo(1380 / 2, 0);

    // Narrower windows give it 62% of their width, as VS Code does.
    await page.setViewportSize({ width: 800, height: 700 });
    const narrow = (await dialog.boundingBox())!;
    expect(narrow.width).toBeCloseTo(800 * 0.62, 0);
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
  });

  test("Command rows are 22px with their shortcut drawn as keycaps", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "editor", "editor");
    await expect(page.locator(".cm-content")).toBeVisible();
    const dialog = page.getByRole("dialog", { name: "Quick Open" });
    const box = dialog.getByRole("combobox", { name: "Run a command" });

    await page.keyboard.press("ControlOrMeta+Shift+p");
    await box.fill(">go to line");
    const option = dialog.getByRole("option", { name: /Go to Line/ });
    await expect(option).toBeVisible();
    await waitForAnimations(dialog);
    expect((await option.boundingBox())!.height).toBeCloseTo(22, 0);
    const shortcut = option.locator(".quick-open__shortcut");
    await expect(shortcut).toHaveAttribute("aria-label", "Ctrl+G");
    await expect(shortcut.locator(".keycap")).toHaveText(["Ctrl", "G"]);
    // The list keeps to 40% of the window, so long lists scroll inside it.
    await box.fill(">");
    const list = dialog.getByRole("listbox");
    expect((await list.boundingBox())!.height).toBeLessThanOrEqual(880 * 0.4);
  });

  test("Explorer menu rows are 24px and stay inside the window at its edge", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "editor", "editor");
    await expect(page.locator(".cm-content")).toBeVisible();
    const row = page.getByRole("treeitem", { name: /^Open file / }).first();
    await expect(row).toBeVisible();
    const menu = page.getByRole("menu", { name: "Explorer actions" });

    await row.click({ button: "right" });
    await expect(menu).toBeVisible();
    await waitForAnimations(menu);
    const item = menu.getByRole("menuitem", { name: "Rename File" });
    expect((await item.boundingBox())!.height).toBeCloseTo(24, 0);
    await page.keyboard.press("Escape");
    await expect(menu).toBeHidden();

    // A right-click near the bottom of a short window flips the menu above
    // the pointer instead of letting the window cut it off.
    const rowBox = (await row.boundingBox())!;
    await page.setViewportSize({
      width: 1380,
      height: Math.ceil(rowBox.y + rowBox.height + 40),
    });
    await row.click({ button: "right" });
    await expect(menu).toBeVisible();
    await waitForAnimations(menu);
    const menuBox = (await menu.boundingBox())!;
    const viewport = page.viewportSize()!;
    expect(menuBox.y + menuBox.height).toBeLessThanOrEqual(viewport.height);
    expect(menuBox.y + menuBox.height).toBeLessThanOrEqual(
      rowBox.y + rowBox.height / 2 + 1,
    );
  });
});
