import { waitForAnimations } from "./shared/visualBrowserPrimitives";
import { expect, test } from "./shared/test";
import { forbidBrowserRetries } from "./shared/browserRetryGuard";
import { installComposerRowSettle } from "./shared/composerGeometry";

forbidBrowserRetries();

for (const name of ["Attach tab to chat", "Downloads"]) {
  test(`keyboard focus follows browser ${name} through overflow and back`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1400, height: 900 });
    await page.goto(
      "/tests/ui/fixtures/browser-address.html?chat=Chat&width=1200",
    );
    await page.evaluate(installComposerRowSettle);
    const surface = page.locator(".browser-surface");
    const toolbar = page.locator(".browser-surface__toolbar");
    const trigger = toolbar.getByRole("button", { name, exact: true });
    await toolbar.getByRole("textbox", { name: "Address or search" }).focus();
    for (
      let index = 0;
      index < 10 &&
      !(await trigger.evaluate((node) => node === document.activeElement));
      index++
    )
      await page.keyboard.press("Tab");
    await expect(trigger).toBeFocused();
    await surface.evaluate((node) => (node.style.width = "280px"));
    const more = toolbar.getByRole("button", { name: "More options" });
    await expect(more).toBeFocused();
    await expect(more).toHaveAttribute("aria-expanded", "false");
    await surface.evaluate((node) => (node.style.width = "1200px"));
    await expect(trigger).toBeFocused();
    await surface.evaluate((node) => (node.style.width = "280px"));
    await expect(more).toBeFocused();
    await page.keyboard.press("Enter");
    const menu = page.getByRole("menu", { name: "Browser menu" });
    await expect(menu).toBeVisible();
    const relocated =
      name === "Downloads"
        ? menu.getByRole("menuitem", { name, exact: true })
        : menu.getByRole("button", { name, exact: true });
    await relocated.focus();
    await surface.evaluate((node) => (node.style.width = "1200px"));
    await expect(trigger).toBeFocused();
    await page.keyboard.press("Escape");
    await trigger.focus();
    await surface.evaluate((node) => (node.style.width = "280px"));
    await expect(more).toBeFocused();
    await page.mouse.click(1300, 800);
    await expect(page.locator("body")).toBeFocused();
    await surface.evaluate((node) => (node.style.width = "1200px"));
    await expect(trigger).toBeVisible();
    await expect(page.locator("body")).toBeFocused();
    // Explicitly choosing More while the row fits must cancel the old return target.
    await trigger.focus();
    await more.focus();
    await surface.evaluate((node) => (node.style.width = "1190px"));
    await toolbar.evaluate(async (row) => {
      await (
        window as Window & {
          settleComposerRow: (row: Element) => Promise<void>;
        }
      ).settleComposerRow(row);
    });
    await expect(more).toBeFocused();
  });
}

test("clips browser navigation hover fills at the pill corner and retains focus", async ({
  page,
}) => {
  await page.goto("/tests/ui/fixtures/browser-address.html");
  const group = page.locator(".browser-surface__nav-group");
  const back = page.getByRole("button", { name: "Back", exact: true });
  await back.hover();
  expect(
    await group.evaluate((node) => {
      const box = node.getBoundingClientRect();
      return node.contains(
        document.elementFromPoint(box.left + 3, box.top + 3),
      );
    }),
  ).toBe(false);
  await page.keyboard.press("Tab");
  await back.focus();
  const focus = await back.evaluate((node) => {
    const style = getComputedStyle(node);
    return {
      focused: node.matches(":focus-visible"),
      width: parseFloat(style.outlineWidth),
      offset: parseFloat(style.outlineOffset),
    };
  });
  expect(focus.focused).toBe(true);
  expect(focus.width).toBeGreaterThan(0);
  expect(focus.offset).toBeLessThanOrEqual(-focus.width);
});

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
    "Show device toolbar",
  ])
    await expect(menu.getByRole("menuitem", { name })).toBeDisabled();
  for (const name of [
    "Downloads",
    "History",
    "Clear browsing data",
    "Browser settings",
  ])
    await expect(menu.getByRole("menuitem", { name })).toBeEnabled();
  await waitForAnimations(menu);
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
