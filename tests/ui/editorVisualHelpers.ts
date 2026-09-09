import { expect, type Page } from "@playwright/test";

import type {
  VisualScenarioMode,
  VisualScenarioName,
} from "./fixtures/visualScenarioTypes";
import {
  expectNoViewportOverflow,
  openScenario,
  takeCanonicalScreenshot,
} from "./shared/visualBrowserPrimitives";

async function assertEditorLandmarks(page: Page) {
  await expect(page.getByRole("tabpanel")).toBeVisible();
}

export async function expectFindContained(page: Page) {
  expect(
    await page.locator(".editor-find--overlay").evaluate((find) => {
      const stage = find.closest<HTMLElement>(".file-editor-stage");
      if (!stage) return false;
      const findBounds = find.getBoundingClientRect();
      const stageBounds = stage.getBoundingClientRect();
      return (
        findBounds.left >= stageBounds.left &&
        findBounds.right <= stageBounds.right &&
        findBounds.top >= stageBounds.top &&
        findBounds.bottom <= stageBounds.bottom
      );
    }),
  ).toBe(true);
}

export async function runEditorCanonicalTest(
  page: Page,
  name: VisualScenarioName,
  mode: VisualScenarioMode,
) {
  const workbench = await openScenario(page, name, mode);
  await assertEditorLandmarks(page);
  if (name === "editor") {
    await expect(
      page.getByRole("textbox", {
        name: "Edit src/workbench/AgentChatPanes.tsx",
        exact: true,
      }),
    ).toContainText("export function AgentChatPanes");
    await expect(
      page.locator(".file-surface .cm-content .cm-line span[class]").first(),
    ).toBeVisible();
    await expect(
      page.getByRole("status").filter({ hasText: "Loading File…" }),
    ).toHaveCount(0);
    const tree = page.getByRole("tree", { name: "Project files" });
    const activeFile = tree.getByRole("treeitem", {
      name: "Open file src/workbench/AgentChatPanes.tsx",
    });
    await expect(activeFile).toBeVisible();
    await expect(activeFile).toHaveAttribute("aria-current", "page");
    expect(
      await activeFile.evaluate((node) => {
        const row = node.getBoundingClientRect();
        const viewport = node.closest(".sidebar")?.getBoundingClientRect();
        return Boolean(
          viewport &&
          row.top >= Math.max(viewport.top, 0) &&
          row.bottom <= Math.min(viewport.bottom, window.innerHeight),
        );
      }),
    ).toBe(true);
  }
  await expectNoViewportOverflow(page);
  await takeCanonicalScreenshot(workbench, name);
}
