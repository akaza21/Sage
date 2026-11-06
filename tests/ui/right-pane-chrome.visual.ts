import { expect, test, type Locator } from "@playwright/test";
import {
  openAgentScenario,
  openAgentSnapshotDiff,
} from "./resourceVisualHelpers";

test.use({
  deviceScaleFactor: 1,
  // Keep rounded-card edge coverage repeatable for exact DPR 1 captures:
  // raster on the CPU, and re-raster whole tiles, so a repaint elsewhere in
  // a tile cannot re-blend the pane's anti-aliased corner differently.
  launchOptions: { args: ["--disable-gpu", "--disable-partial-raster"] },
});
const pixelExact = { maxDiffPixels: 0, maxDiffPixelRatio: 0, threshold: 0 };

async function expectActiveTabStyle(tab: Locator) {
  await expect(tab).toHaveAttribute("aria-selected", "true");
  const appearance = await tab.evaluate((node) => {
    const frame = node.closest(".tab")!;
    const style = getComputedStyle(frame);
    return {
      height: frame.getBoundingClientRect().height,
      background: style.backgroundColor,
      border: style.border,
      radius: style.borderRadius,
      shadow: style.boxShadow,
    };
  });
  expect(appearance).toEqual({
    height: 28,
    background: "rgb(42, 42, 42)",
    border: "1px solid rgb(76, 76, 76)",
    radius: "6px",
    shadow: "none",
  });
}

for (const scenario of ["agent-single", "preview", "file"] as const) {
  test(`artifact header and canvas do not overlap in ${scenario}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1872, height: 1080 });
    await openAgentScenario(page, scenario);
    if (scenario === "agent-single") {
      await page
        .getByRole("button", {
          name: "Show files and preview · Waiting",
          exact: true,
        })
        .click();
    }
    const pane = page.getByLabel("Agent artifacts", { exact: true });
    await expect(pane).toBeVisible();
    if (scenario === "file") {
      await expect(pane.locator(".agent-artifact-header")).toBeVisible();
    }
    const geometry = await pane.evaluate((node) => {
      const header = node
        .querySelector(".agent-artifact-pane__header")!
        .getBoundingClientRect();
      const fileHeader = node
        .querySelector(".agent-artifact-header")
        ?.getBoundingClientRect();
      const canvas = node
        .querySelector(".agent-artifact-pane__canvas")!
        .getBoundingClientRect();
      const bounds = node.getBoundingClientRect();
      return {
        headerHeight: header.height,
        headerBottom: header.bottom,
        contentTop: fileHeader?.top ?? canvas.top,
        contentBottom: fileHeader?.bottom ?? header.bottom,
        canvasTop: canvas.top,
        canvasBottom: canvas.bottom,
        paneBottom: bounds.bottom,
      };
    });
    expect(geometry.headerHeight).toBe(46);
    expect(geometry.contentTop).toBeCloseTo(geometry.headerBottom, 0);
    expect(geometry.canvasTop).toBeCloseTo(geometry.contentBottom, 0);
    expect(geometry.canvasBottom).toBeLessThanOrEqual(geometry.paneBottom);
    expect(geometry.paneBottom - geometry.canvasBottom).toBeLessThanOrEqual(1);
  });
}

for (const viewport of [
  { width: 1280, height: 800 },
  { width: 1872, height: 1080 },
]) {
  test(`right pane and window chrome at ${viewport.width}`, async ({
    page,
  }) => {
    await page.setViewportSize(viewport);
    await openAgentScenario(page, "diff");
    await openAgentSnapshotDiff(page);
    const pane = page.getByLabel("Agent artifacts", { exact: true });
    await page.keyboard.press("Control+Shift+G");
    const changes = pane.getByRole("region", { name: "Chat changes" });
    const changesTab = pane.getByRole("tab", { name: "Changes", exact: true });
    await expect(changesTab).toHaveAttribute("aria-selected", "true");
    await expect(
      changes.getByRole("combobox", { name: "Changes scope" }),
    ).toHaveValue("lastTurn");
    // This chat's latest turn made no edits. Earlier edits and project Git
    // changes must not be represented as Last turn totals.
    await expect(changes.getByText("No changes in this scope.")).toBeVisible();
    await expect(changes.locator(".diff-header__counts")).toHaveCount(0);
    await expect(page.locator(".title-bar")).toHaveCSS(
      "background-color",
      "rgb(32, 32, 32)",
    );
    await expect(page.locator(".title-bar")).toHaveCSS("height", "40px");
    await expect(page.locator(".status-bar")).toHaveCSS("height", "24px");
    await expect(page.locator(".status-bar")).toHaveCSS(
      "background-color",
      "rgb(31, 31, 31)",
    );
    await expect(pane.locator(".agent-artifact-pane__header")).toHaveCSS(
      "height",
      "46px",
    );
    await expect(pane).toHaveScreenshot(
      `team-pane-${viewport.width}-changes.png`,
      pixelExact,
    );
    await expect(page.locator(".title-bar")).toHaveScreenshot(
      `team-pane-${viewport.width}-title.png`,
      pixelExact,
    );
    await expect(page.locator(".status-bar")).toHaveScreenshot(
      `team-pane-${viewport.width}-status.png`,
      pixelExact,
    );
    await expectActiveTabStyle(changesTab);

    await pane
      .getByRole("tab", {
        name: "Diff src/workbench/AgentChatPanes.tsx",
        exact: true,
      })
      .click();
    await expect(changes).toHaveCount(0);
    await expect(
      pane.getByRole("table", { name: "Unified file diff" }),
    ).toBeVisible();
    await expect(pane).toHaveScreenshot(
      `team-pane-${viewport.width}-diff.png`,
      pixelExact,
    );
    await changesTab.click();
    await expect(
      pane.getByRole("table", { name: "Unified file diff" }),
    ).toHaveCount(0);
    await expect(changes).toBeVisible();
    await openAgentSnapshotDiff(page);
    await expect(changes).toHaveCount(0);
    await expectActiveTabStyle(
      pane.getByRole("tab", {
        name: "Diff src/workbench/AgentChatPanes.tsx",
        exact: true,
      }),
    );
    await expect(changesTab).toHaveAttribute("aria-selected", "false");

    // Make the saved edit the latest turn, using the same real projection
    // shape as a chat whose last turn edited a file. The saved diff remains
    // the one opened above, rather than invented totals or file contents.
    await page.evaluate(async () => {
      const { acpProjectionStoreFor } =
        await import("/src/app/acpProjectionRegistry.ts");
      const projection = acpProjectionStoreFor("visual-chat-diff");
      const current = projection.getState().page!;
      projection.setState({
        page: { ...current, turns: current.turns.slice(0, 1) },
      });
    });
    await changesTab.click();
    const totals = changes.locator(
      ".changes-surface__toolbar .diff-header__counts",
    );
    await expect(totals).toHaveText("+1−1");
    await expect(
      changes.getByRole("button", {
        name: /src\/workbench\/AgentChatPanes\.tsx/,
      }),
    ).toBeVisible();
    await expect(
      changes.getByRole("table", { name: "Unified file diff" }),
    ).toBeVisible();
    await expect(pane).toHaveScreenshot(
      `team-pane-${viewport.width}-last-turn.png`,
      pixelExact,
    );
    const scope = changes.getByRole("combobox", { name: "Changes scope" });
    await expect(scope).toHaveValue("lastTurn");
    await expect(scope).toHaveCSS("background-color", "rgb(45, 45, 45)");
    await expect(totals).toHaveCSS("background-color", "rgb(45, 45, 45)");
    const bar = await changes.evaluate((node) => {
      const toolbar = node.querySelector(".changes-surface__toolbar")!;
      const bounds = toolbar.getBoundingClientRect();
      const scopeBounds = toolbar
        .querySelector("select")!
        .getBoundingClientRect();
      const countBounds = toolbar
        .querySelector(".diff-header__counts")!
        .getBoundingClientRect();
      const refreshBounds = toolbar
        .querySelector("button")!
        .getBoundingClientRect();
      const header = node
        .closest(".agent-artifact-pane")!
        .querySelector(".agent-artifact-pane__header")!
        .getBoundingClientRect();
      return {
        height: bounds.height,
        headerGap: bounds.top - header.bottom,
        scopeInset: scopeBounds.top - bounds.top,
        scopeHeight: scopeBounds.height,
        countHeight: countBounds.height,
        countGap: countBounds.left - scopeBounds.right,
        countOffset: countBounds.top - scopeBounds.top,
        refreshHeight: refreshBounds.height,
        refreshOffset: refreshBounds.top - scopeBounds.top,
      };
    });
    expect(bar).toEqual({
      height: 44,
      headerGap: 0,
      scopeInset: 6,
      scopeHeight: 31,
      countHeight: 31,
      countGap: 0,
      countOffset: 0,
      refreshHeight: 31,
      refreshOffset: 0,
    });
    await page
      .getByRole("radiogroup", { name: "Window mode" })
      .getByText("Editor", { exact: true })
      .click();
    await expect(
      page.getByRole("radio", { name: "Editor", exact: true }),
    ).toBeChecked();
    await expect(page.locator(".title-bar")).toHaveCSS(
      "background-color",
      "rgb(32, 32, 32)",
    );
    await expect(page.locator(".status-bar")).toHaveCSS(
      "background-color",
      "rgb(31, 31, 31)",
    );
  });
}
