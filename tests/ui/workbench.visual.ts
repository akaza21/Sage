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

async function settleTranscriptAtEnd(page: Page) {
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
      if (mode === "agent") await settleTranscriptAtEnd(page);
      await expectNoViewportOverflow(page);
      await expect(workbench).toHaveScreenshot(`${name}.png`);
    });
  }
});

test.describe("Wave 2 accessibility and responsive states", () => {
  test("keeps Editor Source Control compact and visually stable", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");

    await page
      .getByRole("button", { name: "Source Control", exact: true })
      .click();
    await expect(
      page.getByLabel("Source Control project tools", { exact: true }),
    ).toBeVisible();
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-source-control.png");
  });

  test("keeps the Editor welcome surface useful and bounded", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");
    await page.evaluate(async () => {
      const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
      fixtures.installVisualEmptyEditorCanvas();
    });

    const welcome = page.getByRole("region", { name: "Editor welcome" });
    await expect(welcome).toBeVisible();
    await expect(
      welcome.getByRole("button", { name: "Browse files" }),
    ).toBeVisible();
    await expect(
      welcome.getByRole("button", { name: "Search files" }),
    ).toBeVisible();
    await expect(
      welcome.getByRole("button", { name: "Open Agent" }),
    ).toBeVisible();
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-welcome.png");
  });

  test("shows selected, pressed, disabled, dirty, and confirmation states", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");
    await page.evaluate(async () => {
      const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
      fixtures.installVisualDirtyEditor();
    });

    await expect(
      page.getByRole("status", {
        name: "Unsaved changes. Press Control or Command plus S to save.",
      }),
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "Save" })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Discard" })).toHaveCount(0);

    const wrap = page.getByRole("button", {
      name: "Toggle editor word wrap",
    });
    await wrap.click();
    await expect(wrap).toHaveAttribute("aria-pressed", "true");
    const font = page.getByRole("button", {
      name: "Cycle editor font size",
    });
    const tabs = page.getByRole("button", { name: "Cycle editor tab size" });
    await font.click();
    await tabs.click();
    await expect(font).toHaveText("Font: 14px");
    await expect(tabs).toHaveText("Tabs: 4 spaces");

    const sourceControl = page.getByRole("button", {
      name: "Source Control",
      exact: true,
    });
    await sourceControl.click();
    await expect(sourceControl).toHaveAttribute("aria-expanded", "true");

    const sourceTools = page.getByLabel("Source Control project tools", {
      exact: true,
    });
    const commit = sourceTools.getByRole("button", {
      name: /Commit staged changes in Sage/,
    });
    await expect(commit).toBeDisabled();

    const discard = sourceTools.getByRole("button", {
      name: /Discard unstaged changes to src\/workbench\/AgentChatPanes\.tsx/,
    });
    await discard.hover();
    await expect(discard).toHaveCSS("background-color", "rgb(42, 42, 42)");
    await discard.click();
    const confirmation = sourceTools.getByRole("group", {
      name: /Confirm discard of src\/workbench\/AgentChatPanes\.tsx/,
    });
    await expect(confirmation).toBeVisible();
    const cancel = confirmation.getByRole("button", {
      name: /Cancel discard/,
    });
    await expect(cancel).toBeVisible();
    await cancel.focus();
    await expect(cancel).toBeFocused();
    await page.keyboard.press("Shift+Tab");
    const confirmDiscard = confirmation.getByRole("button", {
      name: /Confirm discard/,
    });
    await expect(confirmDiscard).toBeFocused();
    await expect(page.locator(":focus-visible")).toHaveCount(1);

    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-control-states.png");
  });

  test("keeps Editor Source Control errors distinct", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");
    await page.evaluate(async () => {
      const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
      fixtures.installVisualRepositoryLoading();
    });

    await page
      .getByRole("button", { name: "Source Control", exact: true })
      .click();
    const sourceTools = page.getByLabel("Source Control project tools", {
      exact: true,
    });
    await expect(sourceTools.getByRole("status")).toContainText(
      "Loading repository status",
    );
    await expect(workbench).toHaveScreenshot(
      "editor-source-control-loading.png",
    );
    await page.evaluate(async () => {
      const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
      fixtures.installVisualRepositoryError();
    });
    await expect(sourceTools.getByRole("alert")).toContainText(
      "Could not refresh repository inventory.",
    );
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-source-control-error.png");
  });

  test("keeps Editor Search compact and visually stable", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");

    await page.getByRole("button", { name: "Search", exact: true }).click();
    const searchTools = page.getByLabel("Search project tools", {
      exact: true,
    });
    await searchTools.getByRole("searchbox").fill("AgentChatPanes");
    await searchTools
      .getByRole("button", { name: "Search", exact: true })
      .click();
    await expect(
      searchTools.getByRole("button", {
        name: "Open src/workbench/AgentChatPanes.tsx at line 3, column 17",
      }),
    ).toBeVisible();
    await expect(searchTools).toContainText(
      "export function AgentChatPanes() {",
    );
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-search-populated.png");
  });

  test("keeps Editor Search empty state bounded", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");

    await page.getByRole("button", { name: "Search", exact: true }).click();
    const searchTools = page.getByLabel("Search project tools", {
      exact: true,
    });
    await expect(searchTools).toContainText(
      "Search all roots in the active Project.",
    );
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-search.png");
  });

  test("keeps Editor Problems cards bounded", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");

    await page.getByRole("button", { name: "Problems", exact: true }).click();
    const problemsTools = page.getByLabel("Problems project tools", {
      exact: true,
    });
    await expect(
      problemsTools.getByRole("heading", { name: "Problems" }),
    ).toBeVisible();
    await expect(problemsTools.getByText("1 Flag")).toBeVisible();
    await expect(
      problemsTools.getByRole("button", {
        name: /maximum should be named maxVisibleChats/i,
      }),
    ).toBeVisible();
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-problems-populated.png");
  });

  test("keeps Editor Problems empty state bounded", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");
    await page.evaluate(async () => {
      const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
      fixtures.installVisualEmptyEditorTools();
    });

    await page.getByRole("button", { name: "Problems", exact: true }).click();
    const problemsTools = page.getByLabel("Problems project tools", {
      exact: true,
    });
    await expect(problemsTools).toContainText(
      "No problems for the active File.",
    );
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-problems.png");
  });

  test("keeps Editor Outline cards bounded", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");

    await page.getByRole("button", { name: "Outline", exact: true }).click();
    const outlineTools = page.getByLabel("Outline project tools", {
      exact: true,
    });
    await expect(
      outlineTools.getByRole("heading", { name: "Outline" }),
    ).toBeVisible();
    await expect(outlineTools.getByText("1 Symbol")).toBeVisible();
    await expect(
      outlineTools.getByRole("button", { name: /AgentChatPanes/i }),
    ).toBeVisible();
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-outline-populated.png");
  });

  test("keeps Editor Outline empty state bounded", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");
    await page.evaluate(async () => {
      const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
      fixtures.installVisualEmptyEditorTools();
    });

    await page.getByRole("button", { name: "Outline", exact: true }).click();
    const outlineTools = page.getByLabel("Outline project tools", {
      exact: true,
    });
    await expect(outlineTools).toContainText("No symbols for the active File.");
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-outline.png");
  });

  test("keeps Editor planes and resize boundaries intentional", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "editor", "editor");

    await expect
      .poll(() =>
        page.evaluate(() => {
          const activity = document.querySelector<HTMLElement>(
            ".workbench--editor .activity-rail",
          );
          const sidebar = document.querySelector<HTMLElement>(
            '.workbench--editor .sidebar[aria-label="Explorer project tools"]',
          );
          const splitter = document.querySelector<HTMLElement>(
            ".workbench--editor .workbench-sidebar-splitter",
          );
          const center = document.querySelector<HTMLElement>(
            ".workbench--editor .center-region",
          );
          if (!activity || !sidebar || !splitter || !center) return null;
          const activityStyle = getComputedStyle(activity);
          const sidebarStyle = getComputedStyle(sidebar);
          const splitterStyle = getComputedStyle(splitter);
          const splitterLine = getComputedStyle(splitter, "::after");
          const centerStyle = getComputedStyle(center);
          return {
            activityBackground: activityStyle.backgroundColor,
            activityWidth: activity.getBoundingClientRect().width,
            centerBackground: centerStyle.backgroundColor,
            centerBorder: centerStyle.borderColor,
            centerRadius: centerStyle.borderRadius,
            sidebarBackground: sidebarStyle.backgroundColor,
            sidebarBorder: sidebarStyle.borderColor,
            sidebarRadius: sidebarStyle.borderRadius,
            splitterLine: splitterLine.backgroundColor,
            splitterWidth: splitterStyle.width,
          };
        }),
      )
      .toEqual({
        activityBackground: "rgb(20, 20, 20)",
        activityWidth: 48,
        centerBackground: "rgb(20, 20, 20)",
        centerBorder: "rgb(36, 36, 36)",
        centerRadius: "6px",
        sidebarBackground: "rgb(20, 20, 20)",
        sidebarBorder: "rgb(36, 36, 36)",
        sidebarRadius: "6px",
        splitterLine: "rgba(0, 0, 0, 0)",
        splitterWidth: "8px",
      });
  });

  test("supports a 720px narrow Agent window", async ({ page }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    const workbench = await openScenario(page, "agent-single", "agent");

    await expect(
      page.getByRole("region", { name: "Agent chat", exact: true }),
    ).toBeVisible();
    await expectNoViewportOverflow(page);
    await settleTranscriptAtEnd(page);
    await expect(workbench).toHaveScreenshot("agent-narrow-720x560.png");
  });

  test("supports a 720px narrow Editor window", async ({ page }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    const workbench = await openScenario(page, "editor", "editor");

    await expect(workbench).toHaveClass(/workbench--compact/);
    await expect(page.locator(".center-region")).toBeVisible();
    await expect
      .poll(() =>
        page.evaluate(() => {
          const drawer = document.querySelector<HTMLElement>(
            '.workbench--editor .sidebar[role="dialog"]',
          );
          const center = document.querySelector<HTMLElement>(
            ".workbench--editor .center-region",
          );
          if (!drawer || !center) return null;
          const drawerBounds = drawer.getBoundingClientRect();
          const centerBounds = center.getBoundingClientRect();
          return centerBounds.left >= drawerBounds.right;
        }),
      )
      .toBe(true);
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-narrow-720x560.png");
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

  test("honors forced colors in Editor mode", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await page.emulateMedia({ forcedColors: "active" });
    const workbench = await openScenario(page, "editor", "editor");

    await expect(page.getByRole("tabpanel")).toBeVisible();
    await expect(workbench).toHaveScreenshot("editor-forced-colors.png");
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

test.describe("Wave 2 200% zoom state", () => {
  test.use({
    viewport: { width: 690, height: 440 },
    deviceScaleFactor: 2,
  });

  test("supports the Agent workbench at 200% zoom", async ({ page }) => {
    const workbench = await openScenario(page, "agent-single", "agent");

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
    await expectNoViewportOverflow(page);
    await settleTranscriptAtEnd(page);
    await expect(workbench).toHaveScreenshot("agent-zoom-200.png");
  });

  test("supports the Editor workbench at 200% zoom", async ({ page }) => {
    const workbench = await openScenario(page, "editor", "editor");

    await expect(workbench).toHaveClass(/workbench--compact/);
    await expect
      .poll(() =>
        page.evaluate(() => ({
          devicePixelRatio: window.devicePixelRatio,
          innerWidth: window.innerWidth,
        })),
      )
      .toEqual({ devicePixelRatio: 2, innerWidth: 690 });
    await expect(page.locator(".center-region")).toBeVisible();
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-zoom-200.png");
  });
});
