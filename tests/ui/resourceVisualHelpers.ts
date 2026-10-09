import { expect, type Locator, type Page } from "@playwright/test";

import type {
  VisualAgentResourceState,
  VisualScenarioName,
} from "./fixtures/visualScenarioTypes";
import {
  expectNoViewportOverflow,
  installFixedClock,
  installLayoutMode,
  installScenario,
} from "./shared/visualBrowserPrimitives";

export async function openAgentScenario(page: Page, name: VisualScenarioName) {
  await installFixedClock(page);
  await installLayoutMode(page, "agent");
  await page.goto("/");
  await installScenario(page, name);
  await page.evaluate(() => document.fonts.ready);
  return page.locator("main.workbench");
}

export async function expandAgentSnapshotEdits(page: Page) {
  const edits = page.getByRole("region", { name: "Files edited in turn 1" });
  const disclosure = edits.getByRole("button", { name: /^1 file changed/ });
  await expect(disclosure).toBeVisible();
  if ((await disclosure.getAttribute("aria-expanded")) === "false") {
    await disclosure.click();
  }
  await expect(disclosure).toHaveAttribute("aria-expanded", "true");
  return edits;
}

export async function openAgentSnapshotDiff(page: Page) {
  const edits = await expandAgentSnapshotEdits(page);
  await edits
    .getByRole("button", {
      name: "Open diff Sage / src/workbench/AgentChatPanes.tsx",
    })
    .click();
  await expect(
    page.getByRole("table", { name: "Unified file diff" }),
  ).toBeVisible();
  await expect(
    page.getByText("Saved agent change", { exact: true }),
  ).toBeVisible();
}

export async function setResourceState(
  page: Page,
  state: VisualAgentResourceState,
) {
  await page.evaluate(async (nextState) => {
    const fixtures =
      await import("/tests/ui/fixtures/surfaces/resourceVisualFixtures.ts");
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

export async function exerciseResourceStates(
  page: Page,
  resource: "diff" | "file",
) {
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
