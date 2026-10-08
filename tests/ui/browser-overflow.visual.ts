import { expect, test } from "@playwright/test";

test("new-tab browser menu retains unavailable page actions at 560 px", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1920, height: 900 });
  await page.goto("/tests/ui/fixtures/browser-address.html?new-tab");
  const toolbar = page.locator(".browser-surface__toolbar");
  await expect(toolbar).toHaveCSS("background-color", "rgb(24, 24, 24)");
  await page.getByRole("button", { name: "More options" }).click();
  const menu = page.getByRole("menu", { name: "Browser menu" });
  await expect(menu).toBeVisible();
  await expect(menu).toHaveCSS("background-color", "rgb(42, 42, 42)");
  for (const name of [
    "Find in page",
    "Print",
    "Take a screenshot",
    "Zoom out",
    "Zoom in",
    "Reset zoom",
  ])
    await expect(menu.getByRole("menuitem", { name })).toBeDisabled();
  for (const name of [
    "Downloads",
    "History",
    "Clear browsing data",
    "Browser settings",
  ])
    await expect(menu.getByRole("menuitem", { name })).toBeEnabled();
  const geometry = await menu.evaluate((element) => {
    const menuBox = element.getBoundingClientRect();
    const toolbarBox = element
      .closest(".browser-surface__toolbar")!
      .getBoundingClientRect();
    return {
      width: menuBox.width,
      right: menuBox.right,
      top: menuBox.top - toolbarBox.top,
    };
  });
  expect(geometry.width).toBeGreaterThanOrEqual(216);
  expect(geometry.width).toBeLessThanOrEqual(224);
  expect(geometry.right).toBeLessThanOrEqual(560);
  expect(geometry.top).toBeGreaterThanOrEqual(40);
  expect(geometry.top).toBeLessThanOrEqual(44);
});
