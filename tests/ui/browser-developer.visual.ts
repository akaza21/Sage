import { expect, test } from "./shared/test";
import { forbidBrowserRetries } from "./shared/browserRetryGuard";

forbidBrowserRetries();

test("developer mode names the backend's allowlist and remains opt-in", async ({
  page,
}) => {
  await page.goto("/tests/ui/fixtures/browser-developer.html");
  const toggle = page.getByRole("checkbox", { name: "Enable CDP access" });
  await expect(toggle).not.toBeChecked();
  await expect(toggle).toHaveAccessibleDescription(
    /Allowed commands: Runtime.evaluate, Page.reload\. Scripts can access data available to the page/,
  );
  await expect(page.getByText("Full CDP access")).toHaveCount(0);
  await toggle.click();
  await expect(toggle).toBeChecked();
  await toggle.click();
  await expect(toggle).not.toBeChecked();
});

test("capability copy follows a changed backend allowlist", async ({
  page,
}) => {
  await page.goto("/tests/ui/fixtures/browser-developer.html?extended");
  await expect(
    page.getByRole("checkbox", { name: "Enable CDP access" }),
  ).toHaveAccessibleDescription(
    /Allowed commands: Runtime.evaluate, Page.reload, Page.getLayoutMetrics\./,
  );
});
