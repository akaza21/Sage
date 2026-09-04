// Boundary: parameterized, mode-neutral browser primitives for the visual
// lane (fixed clock, workbench layout, navigation, font/reflow settlement,
// transcript settlement, viewport-overflow probe, canonical screenshot).
// This module owns no scenario registry, no mode branches, no landmark or
// surface assertions, and registers no tests. Scenario ownership,
// landmarks, and surface branches live in the owning specs and
// `agentVisualHelpers.ts` / `editorVisualHelpers.ts` /
// `resourceVisualHelpers.ts`, composed from these primitives.
import { expect, type Locator, type Page } from "@playwright/test";

import type {
  VisualScenarioMode,
  VisualScenarioName,
} from "../fixtures/visualScenarioTypes";

const FIXED_TIME = "2026-08-31T09:30:00.000Z";

export async function installFixedClock(page: Page, time: string = FIXED_TIME) {
  await page.clock.install({ time: new Date(time) });
}

export async function installLayoutMode(page: Page, mode: VisualScenarioMode) {
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
}

async function gotoWorkbench(page: Page): Promise<Locator> {
  await page.goto("/");
  const workbench = page.locator("main.workbench");
  await expect(workbench).toBeVisible();
  return workbench;
}

export async function installScenario(page: Page, name: VisualScenarioName) {
  await page.evaluate(async (scenarioName) => {
    const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
    await fixtures.installVisualScenario(scenarioName);
  }, name);
  await expect(page.locator("html")).toHaveAttribute(
    "data-visual-scenario",
    name,
  );
}

async function settleFontsAndReflow(page: Page) {
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
}

async function assertWorkbenchMode(
  workbench: Locator,
  mode: VisualScenarioMode,
) {
  await expect(workbench).toHaveClass(new RegExp(`workbench--${mode}`));
}

export async function openScenario(
  page: Page,
  name: VisualScenarioName,
  mode: VisualScenarioMode,
) {
  await installFixedClock(page);
  await installLayoutMode(page, mode);
  const workbench = await gotoWorkbench(page);
  await installScenario(page, name);
  await settleFontsAndReflow(page);
  await assertWorkbenchMode(workbench, mode);
  return workbench;
}

export async function settleTranscriptAtEnd(page: Page) {
  await page.locator(".message-list").evaluateAll(async (lists) => {
    let priorGeometry = "";
    let stableFrames = 0;
    for (let frame = 0; frame < 30 && stableFrames < 4; frame += 1) {
      for (const list of lists) list.scrollTop = list.scrollHeight;
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => resolve()),
      );
      const geometry = lists
        .map((list) => `${list.scrollHeight}:${list.clientHeight}`)
        .join("|");
      stableFrames = geometry === priorGeometry ? stableFrames + 1 : 0;
      priorGeometry = geometry;
    }
    for (const list of lists) list.scrollTop = list.scrollHeight;
  });
  await expect
    .poll(() =>
      page
        .locator(".message-list")
        .evaluateAll((lists) =>
          lists.every(
            (list) =>
              list.scrollHeight - list.scrollTop - list.clientHeight <= 1,
          ),
        ),
    )
    .toBe(true);
}

export async function expectNoViewportOverflow(page: Page) {
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

export async function takeCanonicalScreenshot(
  workbench: Locator,
  name: VisualScenarioName,
) {
  await expect(workbench).toHaveScreenshot(`${name}.png`);
}
