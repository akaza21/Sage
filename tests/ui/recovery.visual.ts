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
      await settleTranscriptAtEnd(page);
      await expectNoViewportOverflow(page);
      await takeCanonicalScreenshot(workbench, name);
    });
  }
});

test.describe("Wave 2 accessibility and responsive states", () => {
  test("reveals Chat actions in the trailing state cell without moving the title", async ({
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
    const actions = row.getByRole("button", { name: /Chat actions for/ });
    const before = await title.evaluate((node) => {
      const bounds = node.getBoundingClientRect();
      return [bounds.x, bounds.y, bounds.width, bounds.height];
    });

    await expect(state).toHaveCSS("opacity", "1");
    await expect(actions).toHaveCSS("opacity", "0");
    await row.hover();
    await expect(state).toHaveCSS("opacity", "0");
    await expect(actions).toHaveCSS("opacity", "1");
    expect(
      await title.evaluate((node) => {
        const bounds = node.getBoundingClientRect();
        return [bounds.x, bounds.y, bounds.width, bounds.height];
      }),
    ).toEqual(before);
    await expectNoViewportOverflow(page);
  });

  test("honors reduced motion", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await openScenario(page, "waiting", "agent");

    const working = page.locator(".project-chat__working");
    await expect(working).toBeVisible();
    await expect(working).toHaveCSS("animation-duration", "0s");
  });
});
