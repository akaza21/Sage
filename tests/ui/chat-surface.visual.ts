import { expect, test } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

test.use({ deviceScaleFactor: 1 });
const pixels = { threshold: 0, maxDiffPixels: 0 };

for (const { width, height } of [
  { width: 1280, height: 800 },
  { width: 1872, height: 1080 },
]) {
  test(`chat header, notice and rows at DPR 1 (${width})`, async ({
    page,
  }, testInfo) => {
    await page.setViewportSize({ width, height });
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    await page.evaluate(async () => {
      const fixture = await import("/tests/ui/fixtures/chatSurfaceFixture.ts");
      fixture.installChatSurfaceFixture(true);
    });
    await expect(
      page.getByRole("button", { name: "Dismiss build notice" }),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Rename chat", exact: true }),
    ).toHaveCount(0);
    expect(await page.evaluate(() => window.devicePixelRatio)).toBe(1);
    const header = page.locator(".agent-task-header");
    const headerBox = (await header.boundingBox())!;
    await testInfo.attach("chat-geometry", {
      contentType: "application/json",
      body: JSON.stringify(
        await header.evaluate((element) => {
          const headerStyle = getComputedStyle(element);
          const reading = document.querySelector(".message-list__content")!;
          const prose = document.querySelector(".acp-markdown")!;
          return {
            headerHeight: element.getBoundingClientRect().height,
            headerMinHeight: headerStyle.minHeight,
            chromeRow: headerStyle.getPropertyValue("--chrome-row-height"),
            space2: headerStyle.getPropertyValue("--space-2"),
            readingWidth: reading.getBoundingClientRect().width,
            proseSize: getComputedStyle(prose).fontSize,
            proseLeading: getComputedStyle(prose).lineHeight,
          };
        }),
      ),
    });
    const cardEdge = await header.evaluate((element) =>
      parseFloat(getComputedStyle(element).getPropertyValue("--border-width")),
    );
    // The parent card owns its rounded edge and its GPU antialiasing.
    // Keep this component baseline inside that edge, including its divider.
    await expect(page).toHaveScreenshot(`chat-surface-header-${width}.png`, {
      ...pixels,
      clip: {
        x: headerBox.x + cardEdge,
        y: headerBox.y,
        width: headerBox.width - 2 * cardEdge,
        height: headerBox.height,
      },
    });
    await expect(page.locator(".build-notice")).toHaveScreenshot(
      `chat-surface-notice-${width}.png`,
      pixels,
    );
    await page.getByRole("button", { name: "Dismiss build notice" }).click();
    await expect(page.locator(".build-notice")).toHaveCount(0);
    await page.locator(".message-list").evaluate((element) => {
      element.scrollTop = 0;
    });
    await expect(page.locator(".message-list")).toHaveScreenshot(
      `chat-surface-rows-${width}.png`,
      pixels,
    );
    await page.screenshot({
      path: testInfo.outputPath("chat-surface-full.png"),
    });
    const actions = page.getByRole("group", { name: "Message actions" });
    await actions.hover();
    await expect(actions).toHaveScreenshot(
      `chat-surface-actions-${width}.png`,
      pixels,
    );
    await page.locator(".chat-transcript-work > summary").click();
    const command = page.locator(".acp-tool-rollup > summary");
    await expect(command).toHaveText(/Ran/);
    await command.click();
    await expect(page.getByLabel("Command line")).toHaveText(/node -e/);
    await expect(
      page.locator(".chat-transcript-work__groups"),
    ).toHaveScreenshot(`chat-surface-tools-${width}.png`, pixels);
    await page
      .getByRole("button", { name: "More actions", exact: true })
      .click();
    await expect(page.getByRole("menu")).toBeVisible();
    await page.keyboard.press("Escape");
    await page
      .locator(".agent-task-header")
      .getByRole("button", { name: /^Chat actions for / })
      .click();
    await page
      .getByRole("menuitem", { name: /^More chat actions in / })
      .click();
    await page
      .getByRole("menuitem", { name: "Rename chat", exact: true })
      .click();
    await expect(
      page.getByRole("textbox", { name: "New chat name" }),
    ).toBeFocused();
  });
}
