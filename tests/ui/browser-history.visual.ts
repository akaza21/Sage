import { expect, test } from "@playwright/test";
import { forbidBrowserRetries } from "./shared/browserRetryGuard";

forbidBrowserRetries();

test("History keeps its layout, searches and removes visits, and navigates in this tab", async ({
  page,
}) => {
  await page.setViewportSize({ width: 900, height: 720 });
  await page.goto("/tests/ui/fixtures/browser-history.html");
  const address = page.getByRole("textbox", { name: "Address or search" });
  await expect(address).toHaveValue("https://current.example/start");
  await address.focus();
  await address.press("Control+h");
  const history = page.getByRole("region", { name: "History", exact: true });
  const search = history.getByRole("searchbox", { name: "Search history" });
  await expect(history).toBeVisible();
  await expect(search).toBeFocused();
  const surfaceBox = await page
    .getByRole("region", { name: "Browser page", exact: true })
    .boundingBox();
  const toolbarBox = await page
    .locator(".browser-surface__toolbar")
    .boundingBox();
  const historyBox = await history.boundingBox();
  expect(surfaceBox).not.toBeNull();
  expect(toolbarBox).not.toBeNull();
  expect(historyBox).not.toBeNull();
  expect(historyBox!.height).toBeGreaterThan(100);
  expect(historyBox!.y).toBeGreaterThanOrEqual(
    toolbarBox!.y + toolbarBox!.height,
  );
  expect(historyBox!.y + historyBox!.height).toBeLessThanOrEqual(
    surfaceBox!.y + surfaceBox!.height + 1,
  );

  await search.fill("Alpha");
  await expect(
    history.getByRole("button", { name: /^(?!Remove ).*Alpha guide/ }),
  ).toBeVisible();
  await expect(
    history.getByRole("button", { name: /^(?!Remove ).*Beta handbook/ }),
  ).toHaveCount(0);
  await history
    .getByRole("button", { name: /^(?!Remove ).*Alpha guide/ })
    .hover();
  const remove = history.getByRole("button", {
    name: "Remove Alpha guide from history",
  });
  await expect(remove).toBeVisible();
  await remove.click();
  await expect(history.getByText(/No history matches/)).toBeVisible();
  await search.press("Escape");
  await expect(history).toHaveCount(0);
  await expect(address).toBeFocused();

  await page.getByRole("button", { name: "More options" }).click();
  await page.getByRole("menuitem", { name: "History" }).click();
  await expect(
    history.getByRole("button", { name: /^(?!Remove ).*Alpha guide/ }),
  ).toHaveCount(0);
  await history
    .getByRole("button", { name: /^(?!Remove ).*Beta handbook/ })
    .click();
  await expect(history).toHaveCount(0);
  await expect(address).toHaveValue("https://beta.example/handbook");
  await expect(page.getByRole("alert")).toHaveCount(0);
});
