import { expect, test, type Page } from "@playwright/test";

import type {
  VisualScenarioMode,
  VisualScenarioName,
} from "./fixtures/visualScenarios";

const FIXED_TIME = "2026-08-31T09:30:00.000Z";

const canonicalVisualScenarios = [
  ["agent-single", "agent"],
  ["agent-dual", "agent"],
  ["agent-long", "agent"],
  ["editor", "editor"],
  ["diff", "agent"],
  ["preview", "agent"],
  ["error", "agent"],
  ["waiting", "agent"],
] as const satisfies ReadonlyArray<
  readonly [VisualScenarioName, VisualScenarioMode]
>;

async function openScenario(
  page: Page,
  name: VisualScenarioName,
  mode: VisualScenarioMode,
) {
  await page.clock.install({ time: new Date(FIXED_TIME) });
  await page.addInitScript((windowMode: VisualScenarioMode) => {
    localStorage.setItem(
      "sage.workbench.layout.v1",
      JSON.stringify({
        editorToolRailWidth: 264,
        agentChatSplitPercent: 50,
        windowMode,
      }),
    );
  }, mode);
  await page.goto("/");
  const workbench = page.locator("main.workbench");
  await expect(workbench).toBeVisible();
  await page.evaluate(async (scenarioName) => {
    const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
    await fixtures.installVisualScenario(scenarioName);
  }, name);
  await expect(page.locator("html")).toHaveAttribute(
    "data-visual-scenario",
    name,
  );
  await page.evaluate(() => document.fonts.ready);
  await page.locator(".message-list").evaluateAll((lists) => {
    for (const list of lists) list.scrollTop = list.scrollHeight;
  });
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      ),
  );
  await expect(workbench).toHaveClass(new RegExp(`workbench--${mode}`));
  return workbench;
}

async function expectNoViewportOverflow(page: Page) {
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          document.documentElement.scrollWidth <=
          document.documentElement.clientWidth,
      ),
    )
    .toBe(true);
}

test.describe("Wave 2 canonical visual states", () => {
  test.use({ viewport: { width: 1380, height: 880 } });

  for (const [name, mode] of canonicalVisualScenarios) {
    test(`${name} stays visually stable`, async ({ page }) => {
      const workbench = await openScenario(page, name, mode);

      if (mode === "agent") {
        const projectSidebar = page.getByRole("complementary", {
          name: "Projects and chats",
        });
        await expect(
          projectSidebar.getByRole("button", {
            name: "New chat",
            exact: true,
          }),
        ).toBeVisible();
        await expect(
          projectSidebar.getByRole("button", { name: "Search chats" }),
        ).toBeVisible();
      } else {
        await expect(page.getByRole("tabpanel")).toBeVisible();
      }
      await expectNoViewportOverflow(page);
      await expect(workbench).toHaveScreenshot(`${name}.png`);
    });
  }
});

test.describe("Wave 2 accessibility and responsive states", () => {
  test("supports a 720px narrow Agent window", async ({ page }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    const workbench = await openScenario(page, "agent-single", "agent");

    await expect(
      page.getByRole("region", { name: "Agent chat", exact: true }),
    ).toBeVisible();
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("agent-narrow-720x560.png");
  });

  test("supports the 200%-equivalent Agent viewport", async ({ page }) => {
    await page.setViewportSize({ width: 690, height: 440 });
    const workbench = await openScenario(page, "agent-single", "agent");

    await expect(
      page.getByRole("region", { name: "Agent chat", exact: true }),
    ).toBeVisible();
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("agent-zoom-200-equivalent.png");
  });

  test("exposes visible keyboard focus", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "agent-single", "agent");

    await page.keyboard.press("Tab");
    const focusVisible = page.locator(":focus-visible");
    await expect(focusVisible).toHaveCount(1);
    await expect(focusVisible).toBeVisible();
    await expect(workbench).toHaveScreenshot("agent-keyboard-focus.png");
  });

  test("honors forced colors", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await page.emulateMedia({ forcedColors: "active" });
    const workbench = await openScenario(page, "agent-single", "agent");

    await expect(workbench).toHaveScreenshot("agent-forced-colors.png");
  });

  test("honors reduced motion", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    const workbench = await openScenario(page, "waiting", "agent");

    await expect(
      page.getByRole("status", { name: "1 working turn" }),
    ).toBeVisible();
    await expect(workbench).toHaveScreenshot("agent-reduced-motion.png");
  });
});
