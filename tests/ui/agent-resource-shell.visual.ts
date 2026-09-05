import { expect, test, type Locator, type Page } from "@playwright/test";

import type {
  VisualAgentResourceState,
  VisualScenarioName,
} from "./fixtures/visualScenarios";

const FIXED_TIME = "2026-08-31T09:30:00.000Z";

async function openAgentScenario(page: Page, name: VisualScenarioName) {
  await page.clock.install({ time: new Date(FIXED_TIME) });
  await page.addInitScript(() => {
    localStorage.setItem(
      "sage.workbench.layout.v1",
      JSON.stringify({
        editorToolRailWidth: 264,
        agentChatSplitPercent: 50,
        windowMode: "agent",
      }),
    );
  });
  await page.goto("/");
  await page.evaluate(async (scenarioName) => {
    const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
    await fixtures.installVisualScenario(scenarioName);
  }, name);
  await expect(page.locator("html")).toHaveAttribute(
    "data-visual-scenario",
    name,
  );
  await page.evaluate(() => document.fonts.ready);
  return page.locator("main.workbench");
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

async function setResourceState(page: Page, state: VisualAgentResourceState) {
  await page.evaluate(async (nextState) => {
    const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
    await fixtures.installVisualAgentResourceState(nextState);
  }, state);
  await expect(page.locator("html")).toHaveAttribute(
    "data-visual-resource-state",
    state,
  );
}

async function expectInsideResourceCanvas(locator: Locator) {
  expect(
    await locator.evaluate((node) => {
      const canvas = node.closest<HTMLElement>(".agent-artifact-pane__canvas");
      if (!canvas) return false;
      const bounds = node.getBoundingClientRect();
      const canvasBounds = canvas.getBoundingClientRect();
      return (
        bounds.left >= canvasBounds.left &&
        bounds.right <= canvasBounds.right &&
        bounds.top >= canvasBounds.top &&
        bounds.bottom <= canvasBounds.bottom
      );
    }),
  ).toBe(true);
}

async function exerciseResourceStates(page: Page, resource: "diff" | "file") {
  if (resource === "file") {
    await expect(
      page.getByRole("textbox", {
        name: "Edit src/workbench/AgentChatPanes.tsx",
      }),
    ).toBeVisible();
  } else {
    await expect(
      page.getByRole("table", { name: "Unified file diff" }),
    ).toBeVisible();
  }

  await setResourceState(page, `${resource}-loading`);
  const loading = page.getByRole("status").filter({
    hasText:
      resource === "file"
        ? "Loading src/workbench/AgentChatPanes.tsx"
        : "Loading diff…",
  });
  await expect(loading).toBeVisible();
  await expectInsideResourceCanvas(loading);

  await setResourceState(page, `${resource}-error`);
  const alert = page.getByRole("alert").filter({
    hasText:
      resource === "file"
        ? "Could not open src/workbench/AgentChatPanes.tsx"
        : "Could not load this diff",
  });
  await expect(alert).toBeVisible();
  await expectInsideResourceCanvas(alert);
  await expectNoViewportOverflow(page);
}

test.describe("V6a Agent right-resource state shell", () => {
  test.use({ viewport: { width: 1_380, height: 880 } });

  test("uses one honest collapsed toggle and returns focus after closing", async ({
    page,
  }) => {
    const workbench = await openAgentScenario(page, "agent-single");
    const toggle = page.getByRole("button", {
      name: "Open files, changes, and preview",
    });
    await expect(toggle).toBeVisible();
    await expect(toggle.locator("svg")).toHaveCount(1);
    await expect(toggle.locator("small")).toHaveCount(0);

    for (let step = 0; step < 24; step += 1) {
      if (await toggle.evaluate((node) => node === document.activeElement)) {
        break;
      }
      await page.keyboard.press("Tab");
    }
    await expect(toggle).toBeFocused();
    const focusRing = await toggle.evaluate((node) => {
      const style = getComputedStyle(node);
      return {
        style: style.outlineStyle,
        width: Number.parseFloat(style.outlineWidth),
      };
    });
    expect(focusRing.style).toBe("solid");
    expect(focusRing.width).toBeGreaterThanOrEqual(1);
    await page.keyboard.press("Enter");
    await expect(
      page.getByRole("heading", { name: "No resources yet" }),
    ).toBeVisible();
    await expect(
      page.getByText(
        "Files, changes, and previews from this Chat will appear here.",
      ),
    ).toBeVisible();
    await expect(workbench).toHaveScreenshot("agent-resource-empty.png");
    const collapse = page.getByRole("button", {
      name: "Collapse files, changes, and preview",
    });
    await collapse.focus();
    await page.keyboard.press("Space");
    await expect(
      page.getByRole("button", {
        name: "Open files, changes, and preview",
      }),
    ).toBeFocused();
    await expectNoViewportOverflow(page);
  });

  test("announces File loading and failure without changing its surface", async ({
    page,
  }) => {
    const workbench = await openAgentScenario(page, "file");
    await expect(
      page.getByRole("textbox", {
        name: "Edit src/workbench/AgentChatPanes.tsx",
      }),
    ).toBeVisible();
    await setResourceState(page, "file-loading");
    const loading = page
      .getByRole("status")
      .filter({ hasText: "Loading src/workbench/AgentChatPanes.tsx" });
    await expect(loading).toContainText(
      "Loading src/workbench/AgentChatPanes.tsx",
    );
    await expect(loading.locator("svg")).toHaveCount(1);

    await page.emulateMedia({
      forcedColors: "active",
      reducedMotion: "reduce",
    });
    await expect
      .poll(() =>
        loading
          .locator("svg")
          .evaluate((icon) => getComputedStyle(icon).animationDuration),
      )
      .toBe("0s");

    await page.emulateMedia({
      forcedColors: "none",
      reducedMotion: "no-preference",
    });
    await setResourceState(page, "file-error");
    await expect(loading).toHaveCount(0);
    await expect(
      page.getByRole("alert").filter({ hasText: "Could not open" }),
    ).toContainText("The file is temporarily unavailable.");
    await expect(workbench).toHaveScreenshot("agent-file-error.png");
    await expectNoViewportOverflow(page);
  });

  test("announces Diff loading and failure with mode-neutral wording", async ({
    page,
  }) => {
    const workbench = await openAgentScenario(page, "diff");
    await expect(
      page.getByRole("table", { name: "Unified file diff" }),
    ).toBeVisible();
    await expect(page.getByText("Read-only", { exact: true })).toBeVisible();
    await expect(page.getByText(/Explorer focused/i)).toHaveCount(0);

    await setResourceState(page, "diff-loading");
    await expect(
      page.getByRole("status").filter({ hasText: "Loading diff…" }),
    ).toBeVisible();
    await expect(workbench).toHaveScreenshot("agent-diff-loading.png");
    await setResourceState(page, "diff-error");
    await expect(
      page.getByRole("alert").filter({ hasText: "Could not load this diff" }),
    ).toContainText("The comparison is temporarily unavailable.");
    await expectNoViewportOverflow(page);
  });

  for (const resource of ["file", "diff"] as const) {
    test(`keeps ${resource} states contained at 720px`, async ({ page }) => {
      await page.setViewportSize({ width: 720, height: 560 });
      const workbench = await openAgentScenario(page, resource);
      await expect(workbench).toHaveClass(/workbench--compact/);
      await exerciseResourceStates(page, resource);
    });
  }

  test("keeps the collapsed and expanded shell contained at 720px", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    const workbench = await openAgentScenario(page, "agent-single");
    await expect(workbench).toHaveClass(/workbench--compact/);
    const toggle = page.getByRole("button", {
      name: "Open files, changes, and preview",
    });
    expect(
      await toggle.evaluate((node) => {
        const bounds = node.getBoundingClientRect();
        return { width: bounds.width, height: bounds.height };
      }),
    ).toEqual({ width: 44, height: 44 });

    await toggle.click();
    await expect(
      page.getByRole("heading", { name: "No resources yet" }),
    ).toBeVisible();
    await expect(page.locator(".center-region")).toHaveAttribute("inert");
    await expectNoViewportOverflow(page);
  });
});

test.describe("V6a Agent resource shell at 200%", () => {
  test.use({
    viewport: { width: 690, height: 440 },
    deviceScaleFactor: 2,
  });

  test("keeps the single edge toggle and empty state usable", async ({
    page,
  }) => {
    const workbench = await openAgentScenario(page, "agent-single");
    await expect(workbench).toHaveClass(/workbench--compact/);
    const toggle = page.getByRole("button", {
      name: "Open files, changes, and preview",
    });
    await expect(toggle.locator("small")).toHaveCount(0);
    await toggle.click();
    await expect(
      page.getByRole("heading", { name: "No resources yet" }),
    ).toBeVisible();
    await expectNoViewportOverflow(page);
  });

  for (const resource of ["file", "diff"] as const) {
    test(`keeps ${resource} states contained`, async ({ page }) => {
      const workbench = await openAgentScenario(page, resource);
      await expect(workbench).toHaveClass(/workbench--compact/);
      await exerciseResourceStates(page, resource);
    });
  }
});
