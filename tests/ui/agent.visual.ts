import { expect, test } from "@playwright/test";

import { AGENT_VISUAL_SCENARIOS } from "./fixtures/visualScenarioTypes";
import { VISUAL_SCENARIOS } from "./fixtures/visualScenarioCatalog";
import {
  expectNoViewportOverflow,
  openScenario,
  settleTranscriptAtEnd,
} from "./shared/visualBrowserPrimitives";
import {
  expandChildActivityHierarchy,
  expectAgentImageContained,
  expectChildActivityContained,
  runAgentCanonicalTest,
} from "./agentVisualHelpers";

test.describe("Wave 2 canonical visual states", () => {
  test.use({ viewport: { width: 1380, height: 880 } });

  for (const { name, mode } of VISUAL_SCENARIOS.filter((scenario) =>
    AGENT_VISUAL_SCENARIOS.includes(scenario.name),
  )) {
    test(`${name} stays visually stable`, async ({ page }) => {
      await runAgentCanonicalTest(page, name, mode);
    });
  }
});

test.describe("V5b Agent image presentation", () => {
  test("stays contained at narrow width, forced colors, and reduced motion", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    await openScenario(page, "agent-image", "agent");

    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expectAgentImageContained(page);
    await expectNoViewportOverflow(page);

    await page.emulateMedia({
      forcedColors: "active",
      reducedMotion: "reduce",
    });
    expect(
      await page.locator(".acp-image").evaluate((figure) => {
        const figureStyle = getComputedStyle(figure);
        const imageStyle = getComputedStyle(
          figure.querySelector(".acp-image__src") ?? figure,
        );
        return {
          borderStyle: figureStyle.borderTopStyle,
          borderColor: figureStyle.borderTopColor,
          animationDuration: imageStyle.animationDuration,
          transitionDuration: imageStyle.transitionDuration,
        };
      }),
    ).toEqual({
      borderStyle: "solid",
      borderColor: "rgb(255, 255, 255)",
      animationDuration: "0s",
      transitionDuration: "0s",
    });
  });
});

test.describe("V5c Agent child activity presentation", () => {
  test("keeps four read-only levels usable at narrow width and forced colors", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    await openScenario(page, "agent-child-activity", "agent");
    const { activity, tree } = await expandChildActivityHierarchy(page);

    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expectChildActivityContained(page);
    const longLabel =
      "Coordinate the independent accessibility review across every visible workbench surface";
    const collaboration = page.getByRole("group", {
      name: `Harness collaboration: ${longLabel}`,
    });
    await expect(collaboration).toBeVisible();
    await expect(
      collaboration.locator(".acp-collaboration__label"),
    ).toHaveAttribute("title", longLabel);
    expect(
      await collaboration
        .locator(".acp-collaboration__label")
        .evaluate((label) => label.scrollWidth > label.clientWidth),
    ).toBe(true);
    await expectNoViewportOverflow(page);

    await page.emulateMedia({
      forcedColors: "active",
      reducedMotion: "reduce",
    });
    expect(
      await activity.evaluate((node) => {
        const item = node.querySelector<HTMLElement>(
          ".acp-child-activity__item",
        );
        const itemStyle = item ? getComputedStyle(item, "::before") : null;
        const label = node.querySelector<HTMLElement>(
          ".acp-collaboration__label",
        );
        return {
          activityBorderStyle: getComputedStyle(node).borderLeftStyle,
          connectorBorderStyle: itemStyle?.borderTopStyle,
          animationDuration: label
            ? getComputedStyle(label).animationDuration
            : null,
          transitionDuration: label
            ? getComputedStyle(label).transitionDuration
            : null,
        };
      }),
    ).toEqual({
      activityBorderStyle: "solid",
      connectorBorderStyle: "solid",
      animationDuration: "0s",
      transitionDuration: "0s",
    });
    await expect(tree.locator('[aria-level="4"]')).toBeFocused();
  });
});

test.describe("Wave 2 accessibility and responsive states", () => {
  test("keeps dual Chat header geometry stable when focus changes", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "agent-dual", "agent");

    const panes = page.locator(".agent-chat-pane");
    const paneHeaders = page.locator(".agent-chat-pane__header");
    const actionRows = page.locator(".agent-chat-pane .surface-header");
    await expect(panes).toHaveCount(2);
    await expect(paneHeaders).toHaveCount(2);
    await expect(actionRows).toHaveCount(2);
    expect(
      await paneHeaders.evaluateAll((headers) =>
        headers.map((header) => header.getBoundingClientRect().height),
      ),
    ).toEqual([32, 32]);
    expect(
      await actionRows.evaluateAll((rows) =>
        rows.every((row) => row.getBoundingClientRect().height <= 36),
      ),
    ).toBe(true);
    const shortcuts = page.locator(
      ".agent-chat-panes--split .composer__shortcut",
    );
    await expect(shortcuts).toHaveCount(2);
    await expect(shortcuts.first()).toBeHidden();
    for (let index = 0; index < 2; index += 1) {
      const pane = panes.nth(index);
      const controls = await Promise.all(
        [
          pane.getByRole("combobox", { name: "Harness" }),
          pane.getByRole("combobox", { name: "Model" }),
          pane.locator(".send-button"),
        ].map((control) =>
          control.evaluate((node) => node.getBoundingClientRect().y),
        ),
      );
      expect(Math.max(...controls) - Math.min(...controls)).toBeLessThan(4);
    }

    const before = await panes.evaluateAll((items) =>
      items.map((item) => {
        const bounds = item.getBoundingClientRect();
        return [bounds.x, bounds.y, bounds.width, bounds.height];
      }),
    );
    const secondFocus = panes.nth(1).locator(".agent-chat-pane__focus");
    await secondFocus.click();
    await expect(secondFocus).toHaveAttribute("aria-pressed", "true");
    const after = await panes.evaluateAll((items) =>
      items.map((item) => {
        const bounds = item.getBoundingClientRect();
        return [bounds.x, bounds.y, bounds.width, bounds.height];
      }),
    );
    expect(after).toEqual(before);
    await expectNoViewportOverflow(page);
  });

  test("shows only the focused Chat when the measured center becomes tight", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1_000, height: 720 });
    const workbench = await openScenario(page, "agent-dual", "agent");

    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expect(
      page.getByRole("separator", { name: "Resize chat panes" }),
    ).toHaveCount(0);
    await expect(
      page.getByRole("region", {
        name: "Chat pane Agent navigation polish",
      }),
    ).toBeVisible();

    await page
      .getByRole("complementary", { name: "Projects and chats" })
      .getByRole("button", { name: /Open chat Editor consistency pass/ })
      .click();
    await expect(
      page.getByRole("region", {
        name: "Chat pane Editor consistency pass",
      }),
    ).toBeVisible();
    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expectNoViewportOverflow(page);
    await settleTranscriptAtEnd(page);
    await expect(workbench).toHaveScreenshot("agent-dual-medium.png");
  });

  test("supports a 720px narrow Agent window", async ({ page }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    const workbench = await openScenario(page, "agent-dual", "agent");

    await expect(
      page.getByRole("region", { name: "Agent chat", exact: true }),
    ).toBeVisible();
    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expect(
      page.getByRole("separator", { name: "Resize chat panes" }),
    ).toHaveCount(0);
    await expectNoViewportOverflow(page);
    await settleTranscriptAtEnd(page);
    await expect(workbench).toHaveScreenshot("agent-narrow-720x560.png");
  });

  test("exposes visible keyboard focus", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "agent-single", "agent");

    await page.keyboard.press("Tab");
    const focusVisible = page.locator(":focus-visible");
    await expect(focusVisible).toHaveCount(1);
    await expect(focusVisible).toBeVisible();
    await settleTranscriptAtEnd(page);
    await expect(workbench).toHaveScreenshot("agent-keyboard-focus.png");
  });

  test("honors forced colors", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await page.emulateMedia({ forcedColors: "active" });
    const workbench = await openScenario(page, "agent-single", "agent");

    await settleTranscriptAtEnd(page);
    await expect(workbench).toHaveScreenshot("agent-forced-colors.png");
  });
});

test.describe("Wave 2 200% zoom state", () => {
  test.use({
    viewport: { width: 690, height: 440 },
    deviceScaleFactor: 2,
  });

  test("supports the Agent workbench at 200% zoom", async ({ page }) => {
    const workbench = await openScenario(page, "agent-dual", "agent");

    await expect(workbench).toHaveClass(/workbench--compact/);
    await expect
      .poll(() =>
        page.evaluate(() => ({
          devicePixelRatio: window.devicePixelRatio,
          innerWidth: window.innerWidth,
        })),
      )
      .toEqual({ devicePixelRatio: 2, innerWidth: 690 });
    await expect(
      page.getByRole("region", { name: "Agent chat", exact: true }),
    ).toBeVisible();
    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expect(
      page.getByRole("separator", { name: "Resize chat panes" }),
    ).toHaveCount(0);
    await expectNoViewportOverflow(page);
    await settleTranscriptAtEnd(page);
    await expect(workbench).toHaveScreenshot("agent-zoom-200.png");
  });

  test("keeps Agent activity cards bounded at 200% zoom", async ({ page }) => {
    await openScenario(page, "agent-activity-family", "agent");

    await expect
      .poll(() =>
        page
          .locator(".acp-activity-card")
          .evaluateAll((cards) =>
            cards.every(
              (card) =>
                card.clientWidth > 0 && card.scrollWidth <= card.clientWidth,
            ),
          ),
      )
      .toBe(true);
    await expectNoViewportOverflow(page);
  });

  test("keeps an Agent image contained at 200% zoom", async ({ page }) => {
    await openScenario(page, "agent-image", "agent");

    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expect
      .poll(() =>
        page.evaluate(() => ({
          devicePixelRatio: window.devicePixelRatio,
          innerWidth: window.innerWidth,
        })),
      )
      .toEqual({ devicePixelRatio: 2, innerWidth: 690 });
    await expectAgentImageContained(page);
    await expectNoViewportOverflow(page);
  });

  test("keeps four child-activity levels contained at 200% zoom", async ({
    page,
  }) => {
    await openScenario(page, "agent-child-activity", "agent");
    await expandChildActivityHierarchy(page);

    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expect
      .poll(() =>
        page.evaluate(() => ({
          devicePixelRatio: window.devicePixelRatio,
          innerWidth: window.innerWidth,
        })),
      )
      .toEqual({ devicePixelRatio: 2, innerWidth: 690 });
    await expectChildActivityContained(page);
    await expectNoViewportOverflow(page);
  });
});
