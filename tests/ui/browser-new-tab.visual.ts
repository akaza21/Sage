import { expect, test } from "./shared/test";
import { forbidBrowserRetries } from "./shared/browserRetryGuard";

forbidBrowserRetries();

test("Suggested tiles truncate long titles in a 560 px browser pane", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1920, height: 900 });
  await page.goto("/tests/ui/fixtures/browser-new-tab.html");

  const suggestions = page.locator(".browser-new-tab__suggestions");
  const tiles = suggestions.getByRole("button");
  await expect(tiles).toHaveCount(4);
  const first = tiles.first();
  await expect(first).toHaveCSS("text-overflow", "ellipsis");
  await expect(first).toHaveCSS("display", "block");
  await expect(first).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
  await expect(first).toHaveCSS("border-top-width", "0px");
  expect(
    await first.evaluate(
      (element) => element.scrollWidth > element.clientWidth,
    ),
  ).toBe(true);
  const tileBounds = await tiles.evaluateAll((elements) =>
    elements.map((element) => {
      const box = element.getBoundingClientRect();
      return { left: box.left, right: box.right, width: box.width };
    }),
  );
  expect(tileBounds[0]!.width).toBeGreaterThanOrEqual(80);
  expect(tileBounds[0]!.right).toBeLessThanOrEqual(tileBounds[1]!.left);
  expect(tileBounds[3]!.right).toBeLessThanOrEqual(560);
});
