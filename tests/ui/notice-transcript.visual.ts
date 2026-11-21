import { expect, test } from "@playwright/test";

test.use({ viewport: { width: 1280, height: 800 } });

for (const [scenario, offender] of [
  ["history", "04"],
  ["boundary", "08"],
  ["session", "09"],
  ["failure", "03"],
]) {
  test(`${scenario} stays local with one explanatory owner`, async ({
    page,
  }) => {
    await page.goto(
      `/tests/ui/fixtures/notice-transcript.html?scenario=${scenario}`,
    );
    const notices = page.locator(".notice-fixture");
    await expect(notices.locator(".notice").first()).toBeVisible();
    if (scenario === "failure") {
      await page.locator(".chat-transcript-work__toggle").click();
      await expect(
        page.getByText("The harness exited unexpectedly.", { exact: true }),
      ).toHaveCount(1);
      await expect(
        page.getByText("harnessExited", { exact: true }),
      ).toBeVisible();
      const link = page.getByRole("link", { name: "Turn failure" });
      await link.click();
      await expect(
        page.getByRole("group", { name: "Turn failed" }),
      ).toBeFocused();
    }
    if (scenario === "boundary") {
      await expect(
        notices.getByText(/Changed-file summaries show retained records only/),
      ).toHaveCount(1);
    }
    if (scenario === "session")
      await expect(
        notices.getByText("Calendar tools are unavailable.", { exact: true }),
      ).toHaveCount(1);
    await expect(notices).toHaveScreenshot(`transcript-notice-${scenario}.png`);
    await page.screenshot({ path: `notice-shots/${offender}-after.png` });
    if (scenario === "history") {
      await page.getByRole("button", { name: "Details" }).click();
      await expect(page.getByText("History unavailable")).toBeVisible();
      await page.getByRole("button", { name: "Retry" }).focus();
      await page.keyboard.press("Enter");
      await expect(page.getByRole("alert")).toHaveCount(0);
      await expect(page.locator('.notice[role="status"]')).toHaveText(
        "Loading chat history…",
      );
    }
  });
}

for (const scenario of ["loading", "transition"]) {
  test(`${scenario} is transparent neutral progress`, async ({ page }) => {
    await page.goto(
      `/tests/ui/fixtures/notice-transcript.html?scenario=${scenario}`,
    );
    const progress = page.locator('.notice[role="status"]');
    await expect(progress).toHaveClass("notice notice--info");
    expect(
      await progress.evaluate((node) => getComputedStyle(node).backgroundColor),
    ).toBe("rgba(0, 0, 0, 0)");
    expect(
      await progress.evaluate((node) => getComputedStyle(node).color),
    ).toBe("rgb(173, 173, 173)");
  });
}
