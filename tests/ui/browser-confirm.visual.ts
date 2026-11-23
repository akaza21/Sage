import { expect, test } from "@playwright/test";
import { forbidBrowserRetries } from "./shared/browserRetryGuard";

forbidBrowserRetries();

test("clear browsing data stays pending until explicitly confirmed", async ({
  page,
}) => {
  await page.addInitScript(() => {
    // Tauri's dialog plugin replaces the synchronous browser global.
    Object.defineProperty(window, "confirm", {
      value: async () => false,
    });
  });
  await page.goto("/tests/ui/fixtures/browser-address.html");
  const trigger = page.getByRole("button", { name: "More options" });
  const address = page.getByRole("textbox", { name: "Address or search" });
  const dialog = page.getByRole("dialog", { name: "Clear browsing data?" });
  const cancel = dialog.getByRole("button", { name: "Cancel" });
  const confirm = dialog.getByRole("button", { name: "Clear browsing data" });

  for (const dismissal of ["Cancel", "Escape"]) {
    await trigger.click();
    await page.getByRole("menuitem", { name: "Clear browsing data" }).click();
    await expect(dialog).toBeVisible();
    await expect(dialog).toHaveAccessibleDescription(
      /All browser tabs will close/,
    );
    await expect(page.getByRole("menu", { name: "Browser menu" })).toBeHidden();
    await expect(cancel).toBeFocused();
    await page.keyboard.press("Shift+Tab");
    await expect(confirm).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(cancel).toBeFocused();

    if (dismissal === "Escape") await page.keyboard.press("Escape");
    else await cancel.click();

    await expect(dialog).toHaveCount(0);
    await expect(trigger).toBeFocused();
    await expect(address).toHaveValue("en.wikipedia.org");
    await expect(page.getByText("Browsing data cleared.")).toHaveCount(0);
    await expect(page.locator(".notice--warning")).toHaveCount(0);
    await expect(page.locator("#workbench-error")).toBeEmpty();
  }

  await trigger.click();
  await page.getByRole("menuitem", { name: "Clear browsing data" }).click();
  await confirm.click();
  await expect(dialog).toHaveCount(0);
  // This fixture runs without the desktop backend: only the explicit action
  // should attempt the operation and report its unavailable-runtime error as
  // the workbench error.
  await expect(page.locator("#workbench-error")).not.toBeEmpty();
});
