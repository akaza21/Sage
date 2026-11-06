import { expect, test } from "@playwright/test";

import { RECOVERY_VISUAL_SCENARIOS } from "./fixtures/visualScenarioTypes";
import { VISUAL_SCENARIOS } from "./fixtures/visualScenarioCatalog";
import {
  expectNoViewportOverflow,
  openScenario,
  settleTranscriptAtEnd,
  takeCanonicalScreenshot,
} from "./shared/visualBrowserPrimitives";
import { assertAgentLandmarks } from "./agentVisualHelpers";

test.describe("Wave 2 canonical visual states", () => {
  test.use({ viewport: { width: 1380, height: 880 } });

  for (const { name, mode } of VISUAL_SCENARIOS.filter((scenario) =>
    RECOVERY_VISUAL_SCENARIOS.includes(scenario.name),
  )) {
    test(`${name} stays visually stable`, async ({ page }) => {
      const workbench = await openScenario(page, name, mode);
      await assertAgentLandmarks(page);
      if (name === "error") {
        const failure = page.getByRole("group", {
          name: "Turn failed",
          exact: true,
        });
        await expect(
          failure.getByText(
            "The preview process stopped unexpectedly. The Chat is preserved and can be retried.",
            { exact: true },
          ),
        ).toBeVisible();
        await expect(
          failure.getByRole("button", { name: "Retry", exact: true }),
        ).toBeVisible();
        await expect(
          page.locator("details.chat-transcript-work"),
        ).not.toHaveAttribute("open", "");
      }
      await settleTranscriptAtEnd(page);
      await expectNoViewportOverflow(page);
      await takeCanonicalScreenshot(workbench, name);
    });
  }
});

test.describe("Wave 2 accessibility and responsive states", () => {
  test("swaps a row's state for Pin and Archive on hover without moving or resizing the title", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "waiting", "agent");

    const chatButton = page
      .getByRole("complementary", { name: "Projects and chats" })
      .getByRole("button", { name: /Open chat .*pinned/ })
      .first();
    const row = chatButton.locator("..");
    const title = chatButton.locator(".project-chat__title-label");
    const state = row.locator(".project-chat__state");
    const quickActions = row.locator(".project-chat__quick-actions");
    const origin = () =>
      title.evaluate((node) => {
        const bounds = node.getBoundingClientRect();
        return [bounds.x, bounds.y, bounds.width, bounds.height];
      });
    const before = await origin();

    await expect(state).toHaveCSS("opacity", "1");
    await expect(quickActions).toHaveCSS("opacity", "0");
    await row.hover();
    await expect(state).toHaveCSS("opacity", "0");
    await expect(quickActions).toHaveCSS("opacity", "1");
    await expect(
      quickActions.getByRole("button", { name: /^(Pin|Unpin) / }),
    ).toBeVisible();
    await expect(
      quickActions.getByRole("button", { name: /^Archive / }),
    ).toBeVisible();
    // The actions sit over the row's end; the title neither moves nor resizes.
    expect(await origin()).toEqual(before);
    await expectNoViewportOverflow(page);
  });

  test("honors reduced motion", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await openScenario(page, "waiting", "agent");

    // AP-C4: the row spins in both its project group and Recents.
    const working = page.locator(".project-chat__working");
    await expect(working.first()).toBeVisible();
    for (const spinner of await working.all())
      await expect(spinner).toHaveCSS("animation-name", "none");
  });
});
