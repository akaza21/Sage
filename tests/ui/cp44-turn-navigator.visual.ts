import { expect, test } from "@playwright/test";
import { disposeJourney } from "./fixtures/agentTurnJourneyBrowser";
import { openNavigationChat } from "./fixtures/chatNavigationFixture";

test.afterEach(async ({ page }) => disposeJourney(page));

test("CP-44 remains keyboard-operable in forced colours without motion", async ({
  page,
}) => {
  const alpha = await openNavigationChat(page);
  await page.emulateMedia({ forcedColors: "active", reducedMotion: "reduce" });
  const button = alpha.getByRole("button", {
    name: "Go to turn 3: Navigator prompt 3",
    exact: true,
  });
  await button.focus();
  await page.clock.runFor(150);
  await expect(alpha.getByRole("tooltip")).toBeVisible();
  expect(
    await button.evaluate(
      (node) => getComputedStyle(node, "::before").transitionDuration,
    ),
  ).toBe("0s");
  await button.press("Enter");
  await expect(alpha.locator('[data-turn-ordinal="3"]')).toBeVisible();
  await expect(alpha).toHaveScreenshot("turn-navigator-forced-colors.png");
});

test("CP-44 dashes widen, preview and jump with the measured gutter geometry", async ({
  page,
}) => {
  const alpha = await openNavigationChat(page);
  const rail = alpha.getByRole("list", { name: "Turn navigator" });
  const buttons = rail.getByRole("button", { name: /^Go to turn/ });
  await expect(buttons).toHaveCount(6);
  await expect(alpha).toHaveScreenshot("turn-navigator-rest.png");
  await buttons.nth(2).hover();
  await page.clock.runFor(350);
  const tooltip = alpha.getByRole("tooltip");
  await expect(tooltip).toContainText("Navigator prompt 3");
  await expect(tooltip).toContainText("Needle turn 3.");
  const widths = await buttons.evaluateAll((nodes) =>
    nodes.map((node) => parseFloat(getComputedStyle(node, "::before").width)),
  );
  expect(widths).toEqual([13, 18, 24, 18, 13, 9]);
  const [dash, card] = await Promise.all([
    buttons.nth(2).boundingBox(),
    tooltip.boundingBox(),
  ]);
  expect(dash && card).toBeTruthy();
  expect(
    Math.abs(card!.y + card!.height / 2 - dash!.y - dash!.height / 2),
  ).toBeLessThanOrEqual(6);
  await expect(alpha).toHaveScreenshot("turn-navigator-hover.png");
  await buttons.first().click();
  await expect
    .poll(() =>
      alpha
        .locator(".acp-message--user")
        .first()
        .evaluate(
          (node) =>
            node.getBoundingClientRect().top -
            node.closest(".message-list")!.getBoundingClientRect().top,
        ),
    )
    .toBeGreaterThanOrEqual(8);
  const inset = await alpha
    .locator(".acp-message--user")
    .first()
    .evaluate(
      (node) =>
        node.getBoundingClientRect().top -
        node.closest(".message-list")!.getBoundingClientRect().top,
    );
  expect(inset).toBeLessThanOrEqual(16);
  await expect(tooltip).toBeVisible();
  await expect(alpha).toHaveScreenshot("turn-navigator-jump.png");
  await buttons.nth(2).focus();
  await page.keyboard.press("ArrowDown");
  await expect(buttons.nth(3)).toBeFocused();
  await page.addStyleTag({
    content:
      ".agent-chat-panes--split { grid-template-columns: 719px 8px 719px !important; }",
  });
  await expect(rail).toBeHidden();
});

test("CP-44 virtual jumps mount retained turns and retain the anchor after prepend and resize", async ({
  page,
}) => {
  const alpha = await openNavigationChat(page, 80);
  await expect(alpha.locator('[data-turn-ordinal="1"]')).toHaveCount(0);
  const rail = alpha.getByRole("list", { name: "Turn navigator" });
  expect(await rail.getByRole("button").count()).toBeLessThanOrEqual(60);
  await rail
    .getByRole("button", {
      name: "Go to turn 1: Navigator prompt 1",
      exact: true,
    })
    .click();
  const prompt = alpha.locator('[data-turn-ordinal="1"] .acp-message--user');
  await expect(prompt).toBeVisible();
  await expect
    .poll(() =>
      prompt.evaluate((node) =>
        Math.abs(
          node.getBoundingClientRect().top -
            node.closest(".message-list")!.getBoundingClientRect().top -
            12,
        ),
      ),
    )
    .toBeLessThanOrEqual(1);
  await page.evaluate(async () => {
    const { acpProjectionStoreFor } =
      await import("/src/app/acpProjectionRegistry.ts");
    const store = acpProjectionStoreFor("journey-chat-alpha");
    const { scope, page } = store.getState();
    const first = page!.turns[0]!;
    const older = { ...first, id: "navigation-older-turn", ordinal: 0 };
    store.getState().applyAuthoritativePage(scope!, {
      ...page!,
      projectionRevision: String(Number(page!.projectionRevision) + 1),
      turns: [older, ...page!.turns],
      nextCursor: null,
    });
  });
  await page.addStyleTag({ content: ".acp-turn { padding-top: 18px; }" });
  await expect
    .poll(() =>
      prompt.evaluate((node) =>
        Math.abs(
          node.getBoundingClientRect().top -
            node.closest(".message-list")!.getBoundingClientRect().top -
            12,
        ),
      ),
    )
    .toBeLessThanOrEqual(1);
  expect(await alpha.locator(".acp-turn").count()).toBeLessThanOrEqual(40);
});
