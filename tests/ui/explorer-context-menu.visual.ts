import { expect, test } from "@playwright/test";

import { openScenario } from "./shared/visualBrowserPrimitives";

// ED-18: a right-click on an Explorer row selects it and opens the Explorer
// actions at that row, in Chromium against the browser runtime.
test.describe("Explorer context menu", () => {
  test("opens the actions for the row right-clicked, next to it", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "editor", "editor");
    await expect(page.locator(".cm-content")).toBeVisible();
    const row = page.getByRole("treeitem", {
      name: /^Open file src\/workbench\/AgentChatPanes\.tsx(?:,|$)/,
    });
    await expect(row).toBeVisible();
    const tabsBefore = await page.getByRole("tab").count();

    await row.click({ button: "right" });
    const menu = page.getByRole("menu", { name: "Explorer actions" });
    await expect(menu).toBeVisible();
    await expect(
      menu.getByRole("menuitem", { name: "Rename File" }),
    ).toBeVisible();
    // Selected, not opened.
    await expect(row).toHaveAttribute("aria-current", "page");
    expect(await page.getByRole("tab").count()).toBe(tabsBefore);
    // Right under the row.
    const rowBox = (await row.boundingBox())!;
    const menuBox = (await menu.boundingBox())!;
    expect(menuBox.y).toBeGreaterThanOrEqual(rowBox.y + rowBox.height - 1);
    expect(menuBox.y).toBeLessThan(rowBox.y + rowBox.height + 24);

    // The root's heading above, while the menu is open: the menu moves
    // there and keeps the focus.
    const heading = page.locator(".root-tree__heading").first();
    await heading.click({ button: "right" });
    await expect(heading).toHaveAttribute("aria-current", "page");
    await page.clock.runFor(100);
    await expect(menu.getByRole("menuitem").first()).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(heading).toBeFocused();
    await row.click({ button: "right" });

    await page.keyboard.press("Escape");
    await expect(menu).toBeHidden();
    await expect(row).toBeFocused();

    // The keyboard's context-menu key does the same on the focused row.
    await page.keyboard.press("Shift+F10");
    await expect(menu).toBeVisible();
    await menu.getByRole("menuitem", { name: "Rename File" }).click();
    const dialog = page.getByRole("dialog", { name: "Rename File" });
    await expect(dialog.getByLabel("File name")).toHaveValue(
      "AgentChatPanes.tsx",
    );
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(row).toBeFocused();
  });
});
