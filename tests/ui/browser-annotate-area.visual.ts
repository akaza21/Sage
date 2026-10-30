import { expect, test } from "@playwright/test";
import { forbidBrowserRetries } from "./shared/browserRetryGuard";

forbidBrowserRetries();

test.beforeEach(async ({ page }) => {
  await page.goto("/tests/ui/fixtures/browser-annotate-drag.html");
  await page.waitForFunction(() => "stopAnnotate" in window);
});

test("a trusted reverse drag stays local until one end report", async ({
  page,
}) => {
  await page.mouse.move(300, 250);
  await page.mouse.down();
  await page.mouse.move(100, 80, { steps: 12 });
  expect(await page.evaluate(() => Reflect.get(window, "reports"))).toEqual([]);
  await page.mouse.up();
  expect(await page.evaluate(() => Reflect.get(window, "reports"))).toEqual([
    { target: "area", x: 100, y: 80, width: 200, height: 170 },
  ]);
});

test("page-dispatched mouse events cannot report a capture", async ({
  page,
}) => {
  await page.evaluate(() => {
    for (const type of ["mousedown", "mousemove", "mouseup"])
      document.body.dispatchEvent(
        new MouseEvent(type, {
          bubbles: true,
          button: 0,
          clientX: 300,
          clientY: 250,
        }),
      );
  });
  expect(await page.evaluate(() => Reflect.get(window, "reports"))).toEqual([]);
});

test("leaving annotate restores trusted page clicks and cancels the drag", async ({
  page,
}) => {
  await page.mouse.move(300, 250);
  await page.mouse.down();
  await page.evaluate(() => Reflect.get(window, "stopAnnotate")());
  await page.mouse.up();
  await page.locator("#normal-click").click();
  expect(await page.evaluate(() => Reflect.get(window, "pageClicks"))).toBe(1);
  expect(await page.evaluate(() => Reflect.get(window, "reports"))).toEqual([]);
});
