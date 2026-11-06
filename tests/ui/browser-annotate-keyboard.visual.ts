import { expect, test } from "./shared/test";
import { forbidBrowserRetries } from "./shared/browserRetryGuard";

forbidBrowserRetries();

test("Escape from Comment after tabbing out of Downloads keeps the page covered", async ({
  page,
}) => {
  await page.goto("/tests/ui/fixtures/browser-annotate-keyboard.html");
  await expect(page.locator("body")).toHaveAttribute(
    "data-page-hidden",
    "false",
  );
  await page.getByRole("button", { name: "Press me" }).click();
  const comment = page.getByRole("textbox", { name: "Comment" });
  await expect(comment).toBeFocused();
  await page.getByRole("button", { name: "Downloads" }).click();
  const downloads = page.getByRole("dialog", { name: "Downloads" });
  await expect(downloads).toBeFocused();
  await expect(page.locator("body")).toHaveAttribute(
    "data-page-hidden",
    "true",
  );
  await page.keyboard.press("Tab");
  await expect(comment).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(comment).toHaveCount(0);
  await expect(downloads).toBeVisible();
  await expect(page.locator("body")).toHaveAttribute(
    "data-page-hidden",
    "true",
  );
  const controls = await page.evaluate(() =>
    (
      window as unknown as {
        coverCommands: { command: string; args: Record<string, unknown> }[];
      }
    ).coverCommands.filter((item) => item.command === "browser_control"),
  );
  expect(controls).toEqual([
    {
      command: "browser_control",
      args: { tabId: "browser-1", action: "annotateCancel" },
    },
  ]);
});
