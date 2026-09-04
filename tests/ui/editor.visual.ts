import { expect, test } from "@playwright/test";

import { EDITOR_VISUAL_SCENARIOS } from "./fixtures/visualScenarioTypes";
import { VISUAL_SCENARIOS } from "./fixtures/visualScenarioCatalog";
import {
  expectNoViewportOverflow,
  openScenario,
} from "./shared/visualBrowserPrimitives";
import {
  expectFindContained,
  runEditorCanonicalTest,
} from "./editorVisualHelpers";

test.describe("Wave 2 canonical visual states", () => {
  test.use({ viewport: { width: 1380, height: 880 } });

  for (const { name, mode } of VISUAL_SCENARIOS.filter((scenario) =>
    EDITOR_VISUAL_SCENARIOS.includes(scenario.name),
  )) {
    test(`${name} stays visually stable`, async ({ page }) => {
      await runEditorCanonicalTest(page, name, mode);
    });
  }
});

test.describe("Wave 2 accessibility and responsive states", () => {
  test("keeps Editor Settings visible, resettable, and responsive", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");
    await page.getByRole("button", { name: "Open settings" }).click();

    await expect(page.getByRole("tab", { name: "Settings" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    const settings = page.getByRole("region", { name: "Settings" });
    await expect(settings).toBeVisible();
    const reset = settings.getByRole("button", {
      name: "Reset Editor Appearance",
    });
    await expect(reset).toBeDisabled();
    await expect(
      settings.getByRole("checkbox", { name: "Line numbers" }),
    ).toBeChecked();
    await expect(
      settings.getByRole("checkbox", { name: "Show whitespace" }),
    ).not.toBeChecked();
    await settings
      .getByRole("combobox", { name: "Font size" })
      .selectOption("16");
    await settings.getByRole("checkbox", { name: "Line numbers" }).uncheck();
    await settings.getByRole("checkbox", { name: "Show whitespace" }).check();
    await expect(reset).toBeEnabled();
    await page.getByRole("button", { name: "Close Settings" }).click();
    await expect(page.locator(".cm-lineNumbers")).toHaveCount(0);
    await expect(page.locator(".cm-highlightSpace").first()).toBeVisible();
    await page.getByRole("button", { name: "Open settings" }).click();
    await reset.click();
    await expect(
      settings.getByRole("combobox", { name: "Font size" }),
    ).toHaveValue("13");
    await expect(
      settings.getByRole("checkbox", { name: "Line numbers" }),
    ).toBeChecked();
    await expect(
      settings.getByRole("checkbox", { name: "Show whitespace" }),
    ).not.toBeChecked();
    await expect(workbench).toHaveScreenshot("editor-settings.png");

    const settingsSearch = settings.getByRole("searchbox");
    await settingsSearch.fill("whitespace");
    await expect(
      settings.getByRole("checkbox", { name: "Show whitespace" }),
    ).toBeVisible();
    await expect(
      settings.getByRole("combobox", { name: "Font size" }),
    ).toHaveCount(0);
    await settingsSearch.fill("");

    await page.evaluate(async () => {
      const { useEditorAppearanceStore } =
        await import("/src/app/editorAppearanceStore.ts");
      useEditorAppearanceStore.setState({
        persistencePhase: "error",
        persistenceErrorKind: "save",
        persistenceMessage: "Editor settings could not be saved.",
      });
    });
    await expect(settings.getByRole("alert")).toContainText(
      "Editor settings could not be saved.",
    );
    await expect(settings.getByRole("button", { name: "Retry" })).toBeVisible();
    await expect(workbench).toHaveScreenshot("editor-settings-save-error.png");
    await settings.getByRole("button", { name: "Retry" }).click();
    await expect(settings.getByRole("alert")).toHaveCount(0);

    await page.setViewportSize({ width: 720, height: 560 });
    await page.getByRole("button", { name: "Hide left sidebar" }).click();
    await expect(settings).toBeVisible();
    await expectNoViewportOverflow(page);
    const fontCopy = settings.getByText(
      "Controls the size of text in the code editor.",
    );
    const fontSelect = settings.getByRole("combobox", { name: "Font size" });
    const [copyBounds, selectBounds] = await Promise.all([
      fontCopy.boundingBox(),
      fontSelect.boundingBox(),
    ]);
    expect(copyBounds).not.toBeNull();
    expect(selectBounds).not.toBeNull();
    expect(selectBounds!.y).toBeGreaterThan(copyBounds!.y);
    await page.emulateMedia({
      forcedColors: "active",
      reducedMotion: "reduce",
    });
    await expect(settings.getByRole("searchbox")).toBeVisible();
    const wordWrap = settings.getByRole("checkbox", { name: "Word wrap" });
    await wordWrap.focus();
    await page.keyboard.press("Space");
    await expect(
      settings.getByRole("checkbox", { name: "Word wrap" }),
    ).toBeChecked();
    expect(
      await settings
        .getByRole("searchbox")
        .evaluate((node) => getComputedStyle(node).transitionDuration),
    ).toBe("0s");
    await expect(reset).toBeEnabled();
    await expectNoViewportOverflow(page);
  });

  test("scrolls a restored Explorer file into the actual sidebar viewport", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1_000, height: 560 });
    await openScenario(page, "editor", "editor");
    await page.evaluate(async () => {
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/editorVisualFixtures.ts");
      await fixtures.installVisualExplorerScrollFixture();
    });

    const activeFile = page.getByRole("treeitem", {
      name: "Open file src/workbench/AgentChatPanes.tsx",
    });
    await expect(activeFile).toBeVisible();
    const visibility = await activeFile.evaluate((node) => {
      const row = node.getBoundingClientRect();
      const viewport = node.closest(".sidebar")?.getBoundingClientRect();
      const sidebar = node.closest<HTMLElement>(".sidebar");
      return {
        inside:
          viewport !== undefined &&
          row.top >= Math.max(viewport.top, 0) &&
          row.bottom <= Math.min(viewport.bottom, window.innerHeight),
        scrolled: Boolean(sidebar && sidebar.scrollTop > 0),
      };
    });
    expect(visibility).toEqual({ inside: true, scrolled: true });
    await expectNoViewportOverflow(page);
  });

  test("keeps compact Explorer actions usable and contained", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    const workbench = await openScenario(page, "editor", "editor");
    await page.evaluate(async () => {
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/editorVisualFixtures.ts");
      fixtures.installVisualExplorerActionFixture();
    });

    const explorer = page.getByLabel("Explorer project tools", { exact: true });
    const more = explorer.getByRole("button", {
      name: "More Explorer actions",
    });
    await expect(
      explorer.getByRole("button", { name: "New File" }),
    ).toBeVisible();
    await expect(
      explorer.getByRole("button", { name: "New Folder" }),
    ).toBeVisible();
    await expect(
      explorer.getByRole("button", { name: "Add folder root" }),
    ).toHaveCount(0);

    await more.focus();
    await more.press("Enter");
    const menu = page.getByRole("menu", { name: "Explorer actions" });
    await expect(menu).toBeVisible();
    await expect(menu.getByRole("menuitem")).toHaveText([
      "Add folder root",
      "Rename File",
      "Move to Recycle Bin",
    ]);
    await expect(
      menu.getByRole("menuitem", { name: "Add folder root" }),
    ).toBeFocused();
    expect(
      await menu
        .getByRole("menuitem")
        .evaluateAll((items) =>
          items.map((item) => (item as HTMLElement).tabIndex),
        ),
    ).toEqual([0, -1, -1]);
    await page.keyboard.press("End");
    await expect(
      menu.getByRole("menuitem", { name: "Move to Recycle Bin" }),
    ).toBeFocused();
    expect(
      await menu
        .getByRole("menuitem")
        .evaluateAll((items) =>
          items.map((item) => (item as HTMLElement).tabIndex),
        ),
    ).toEqual([-1, -1, 0]);
    await page.keyboard.press("Tab");
    await expect(menu).toBeHidden();
    await expect(more).toHaveAttribute("aria-expanded", "false");

    await more.focus();
    await more.press("ArrowUp");
    await expect(menu).toBeVisible();
    await expect(
      menu.getByRole("menuitem", { name: "Move to Recycle Bin" }),
    ).toBeFocused();
    await page.keyboard.press("Shift+Tab");
    await expect(menu).toBeHidden();
    await expect(more).toHaveAttribute("aria-expanded", "false");

    await more.focus();
    await more.press("ArrowDown");
    await expect(
      menu.getByRole("menuitem", { name: "Add folder root" }),
    ).toBeFocused();

    expect(
      await menu.evaluate((node) => {
        const menuBounds = node.getBoundingClientRect();
        return Boolean(
          menuBounds.left >= 8 &&
          menuBounds.right <= innerWidth - 8 &&
          menuBounds.top >= 8 &&
          menuBounds.bottom <= innerHeight - 8 &&
          node.scrollWidth <= node.clientWidth,
        );
      }),
    ).toBe(true);
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-explorer-actions.png");

    await menu.getByRole("menuitem", { name: "Rename File" }).click();
    await expect(menu).toBeHidden();
    const dialog = page.getByRole("dialog", { name: "Rename File" });
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(more).toBeFocused();

    await more.click();
    await page.emulateMedia({ forcedColors: "active" });
    await expect(menu).toBeVisible();
    expect(
      await menu.evaluate((node) => {
        const style = getComputedStyle(node);
        return {
          border: style.borderTopStyle,
          distinct: style.color !== style.backgroundColor,
        };
      }),
    ).toEqual({ border: "solid", distinct: true });
    await explorer.locator(".sidebar__heading > span").click();
    await expect(menu).toBeHidden();
    await expect(more).toBeFocused();

    await page.evaluate(async () => {
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/editorVisualFixtures.ts");
      fixtures.installVisualExplorerUnavailableActionFixture();
    });
    await more.press("ArrowDown");
    const reason = menu.getByRole("menuitem", {
      name: "Select a visible closed file or empty folder.",
    });
    await expect(reason).toHaveAttribute("aria-disabled", "true");
    await expect(reason).toHaveAttribute("tabindex", "-1");
    await page.keyboard.press("ArrowDown");
    await expect(reason).toBeFocused();
    expect(
      await menu
        .getByRole("menuitem")
        .evaluateAll((items) =>
          items.map((item) => (item as HTMLElement).tabIndex),
        ),
    ).toEqual([-1, 0]);
    await page.keyboard.press("ArrowDown");
    await expect(
      menu.getByRole("menuitem", { name: "Add folder root" }),
    ).toBeFocused();
    await page.keyboard.press("ArrowUp");
    await expect(reason).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(menu).toBeHidden();
    await more.press("ArrowUp");
    await expect(reason).toBeFocused();
    await expect(reason).toHaveAttribute("tabindex", "0");
  });

  test("keeps in-file Find compact without moving the document", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");
    const editor = page.getByRole("textbox", {
      name: "Edit src/workbench/AgentChatPanes.tsx",
    });
    const editorPlane = page.locator(".editor-input");
    const before = await editorPlane.boundingBox();

    await editor.press("Control+f");
    const find = page.getByRole("search", { name: "Find in file" });
    await expect(find).toBeVisible();
    await find.getByRole("textbox", { name: "Find text" }).fill("ChatSurface");
    await expect(find.getByRole("status")).toContainText("matches");
    expect(await editorPlane.boundingBox()).toEqual(before);
    await expectFindContained(page);
    await expect(workbench).toHaveScreenshot("editor-find.png");

    await page.setViewportSize({ width: 720, height: 560 });
    const hideSidebar = page.getByRole("button", { name: "Hide left sidebar" });
    if (await hideSidebar.isVisible()) await hideSidebar.click();
    await expect(find).toBeVisible();
    await expectFindContained(page);
    expect(
      await find.evaluate(
        (node) => node.scrollWidth <= node.clientWidth && node.clientWidth > 0,
      ),
    ).toBe(true);
    await page.emulateMedia({ forcedColors: "active" });
    await find.getByRole("button", { name: "Match case" }).click();
    await expect(
      find.getByRole("button", { name: "Match case" }),
    ).toHaveAttribute("aria-pressed", "true");
    await expectNoViewportOverflow(page);
  });

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
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/editorVisualFixtures.ts");
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

  test("honors forced colors in Editor mode", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await page.emulateMedia({ forcedColors: "active" });
    const workbench = await openScenario(page, "editor", "editor");

    await expect(page.getByRole("tabpanel")).toBeVisible();
    await expect(workbench).toHaveScreenshot("editor-forced-colors.png");
  });
});

test.describe("Wave 2 200% zoom state", () => {
  test.use({
    viewport: { width: 690, height: 440 },
    deviceScaleFactor: 2,
  });

  test("keeps Editor Settings usable at 200% zoom", async ({ page }) => {
    await openScenario(page, "editor", "editor");
    await page.getByRole("button", { name: "Open settings" }).click();
    await page.getByRole("button", { name: "Hide left sidebar" }).click();

    const settings = page.getByRole("region", { name: "Settings" });
    await expect(settings).toBeVisible();
    await expect(settings.getByRole("searchbox")).toBeVisible();
    await expect(
      settings.getByRole("combobox", { name: "Font size" }),
    ).toBeVisible();
    await expectNoViewportOverflow(page);
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
    await page.evaluate(async () => {
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/editorVisualFixtures.ts");
      fixtures.installVisualExplorerActionFixture();
    });
    await page.getByRole("button", { name: "More Explorer actions" }).click();
    const explorerMenu = page.getByRole("menu", { name: "Explorer actions" });
    await expect(explorerMenu).toBeVisible();
    expect(
      await explorerMenu.evaluate((node) => {
        const menuBounds = node.getBoundingClientRect();
        return Boolean(
          menuBounds.left >= 8 &&
          menuBounds.right <= innerWidth - 8 &&
          menuBounds.top >= 8 &&
          menuBounds.bottom <= innerHeight - 8 &&
          node.scrollWidth <= node.clientWidth,
        );
      }),
    ).toBe(true);
    await page.keyboard.press("Escape");
    await expect(page.locator(".center-region")).toBeVisible();
    await page.getByRole("button", { name: "Hide left sidebar" }).click();
    const editorContent = page.locator(".file-surface .cm-content");
    await editorContent.click();
    await editorContent.press("Control+f");
    const find = page.getByRole("search", { name: "Find in file" });
    await expect(find).toBeVisible();
    await expectFindContained(page);
    expect(
      await find.evaluate(
        (node) => node.scrollWidth <= node.clientWidth && node.clientWidth > 0,
      ),
    ).toBe(true);
    await find.getByRole("button", { name: "Close find" }).click();
    await page.getByRole("button", { name: "Show left sidebar" }).click();
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-zoom-200.png");
  });
});
