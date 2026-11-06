import { expect, test } from "./shared/test";
import { forbidBrowserRetries } from "./shared/browserRetryGuard";

forbidBrowserRetries();

for (const width of [1060, 780]) {
  test(`stale-tab warning stays below the wrapped site permissions description at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 880 });
    await page.goto("/tests/ui/fixtures/browser-site-permissions.html");
    const group = page.getByRole("group", { name: "Site permissions" });
    const description = group.locator("#browser-site-permissions-description");
    const warning = group.getByRole("alert").filter({
      hasText: "have not reloaded since an Allow ended",
    });
    await expect(description).toBeVisible();
    await expect(warning).toBeVisible();
    await expect(
      warning.getByRole("button", { name: "Reload tabs" }),
    ).toBeVisible();
    const descriptionBox = await description.boundingBox();
    const warningBox = await warning.boundingBox();
    expect(descriptionBox).not.toBeNull();
    expect(warningBox).not.toBeNull();
    expect(
      warningBox!.y - (descriptionBox!.y + descriptionBox!.height),
    ).toBeGreaterThanOrEqual(8);
  });
}
