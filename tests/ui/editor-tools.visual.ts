import { expect, test } from "@playwright/test";

import {
  expectNoViewportOverflow,
  openScenario,
} from "./shared/visualBrowserPrimitives";
import { settleEditorGutter } from "./editorVisualHelpers";

test.describe("Wave 2 accessibility and responsive states", () => {
  test("shows selected, pressed, disabled, dirty, and confirmation states", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");
    await page.evaluate(async () => {
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/editorVisualFixtures.ts");
      fixtures.installVisualDirtyEditor();
    });

    await expect(
      page.getByRole("status", {
        // The browser names the host platform's own save chord.
        name: `Unsaved changes. Press ${process.platform === "darwin" ? "Command" : "Control"} plus S to save.`,
      }),
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "Save" })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Discard" })).toHaveCount(0);

    await expect(page.getByTitle("Indentation")).toHaveText("Spaces: 2");
    await expect(
      page.getByRole("group", { name: "Editor appearance" }),
    ).toHaveCount(0);

    const sourceControl = page.getByRole("button", {
      name: "Source Control",
      exact: true,
    });
    await sourceControl.click();
    await expect(sourceControl).toHaveAttribute("aria-expanded", "true");

    const sourceTools = page.getByLabel("Source Control project tools", {
      exact: true,
    });
    await expect(
      sourceTools.getByRole("button", { name: "Publish Branch", exact: true }),
    ).toBeVisible();
    await expect(
      sourceTools.getByRole("form", {
        name: "Commit staged changes in Sage",
      }),
    ).toHaveCount(0);

    const discard = sourceTools.getByRole("button", {
      name: /Discard unstaged changes to src\/workbench\/AgentChatPanes\.tsx/,
    });
    await discard.hover();
    await expect(discard).toHaveCSS("background-color", "rgb(42, 42, 42)");
    await expect(discard).toHaveCSS("border-radius", "4px");
    await discard.click();
    const confirmation = sourceTools.getByRole("group", {
      name: /Confirm discard of src\/workbench\/AgentChatPanes\.tsx/,
    });
    await expect(confirmation).toBeVisible();
    const cancel = confirmation.getByRole("button", {
      name: /Cancel discard/,
    });
    await expect(cancel).toBeVisible();
    await expect(cancel).toHaveCSS("border-radius", "4px");
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

    for (const state of ["ahead", "behind", "diverged"] as const) {
      await page.evaluate(async (primaryState) => {
        const fixtures =
          await import("/tests/ui/fixtures/surfaces/editorVisualFixtures.ts");
        fixtures.installVisualRepositoryPrimaryState(primaryState);
      }, state);
      const label =
        state === "ahead"
          ? "Push"
          : state === "behind"
            ? "Pull"
            : "Sync Changes";
      const action = sourceTools.getByRole("button", {
        name: label,
        exact: true,
      });
      await expect(action).toBeVisible();
      if (state === "diverged") await expect(action).toBeDisabled();
    }
    await expect(sourceTools).toContainText(
      "Sage cannot yet choose a merge or rebase safely.",
    );
  });

  test("keeps Editor Source Control errors distinct", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");
    // The capture below is taken while repository status loads, so first let
    // the File finish styling: until its stylesheet applies, line numbers sit
    // a row below their text and the screenshot depends on timing.
    await expect
      .poll(() =>
        page.evaluate(() => {
          const text = document.querySelector(".file-surface .cm-line");
          const number = [
            ...document.querySelectorAll(
              ".file-surface .cm-lineNumbers .cm-gutterElement",
            ),
          ].find((element) => element.textContent === "1");
          if (!text || !number) return Infinity;
          return Math.abs(
            number.getBoundingClientRect().top -
              text.getBoundingClientRect().top,
          );
        }),
      )
      .toBeLessThan(2);
    await expect(page.locator(".file-surface .cm-foldGutter")).toBeVisible();
    await page.evaluate(async () => {
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/editorVisualFixtures.ts");
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
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/editorVisualFixtures.ts");
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
    await searchTools.getByRole("searchbox").press("Enter");
    await expect(
      searchTools.getByRole("button", {
        name: "Open src/workbench/AgentChatPanes.tsx at line 3, column 17",
      }),
    ).toBeVisible();
    await expect(searchTools).toContainText(
      "export function AgentChatPanes() {",
    );
    await expectNoViewportOverflow(page);
    await settleEditorGutter(page);
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
      "Search all roots in the active project.",
    );
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-search.png");
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
    const symbol = outlineTools.getByRole("treeitem", {
      name: "Function: AgentChatPanes. src/workbench/AgentChatPanes.tsx, line 3, column 17",
    });
    await expect(symbol).toBeVisible();
    await expect(symbol).toHaveAttribute(
      "title",
      "AgentChatPanes — src/workbench/AgentChatPanes.tsx:3:17",
    );
    await expect(outlineTools.locator(".outline-card")).toHaveCount(1);
    await symbol.focus();
    await page.keyboard.press("Enter");
    await expect(page.getByTitle("Caret position")).toHaveText("Ln 3, Col 17");
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-outline-populated.png");
  });

  test("keeps Editor Outline empty state bounded", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");
    await page.evaluate(async () => {
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/editorVisualFixtures.ts");
      fixtures.installVisualEmptyEditorTools();
    });

    await page.getByRole("button", { name: "Outline", exact: true }).click();
    const outlineTools = page.getByLabel("Outline project tools", {
      exact: true,
    });
    await expect(outlineTools).toContainText("No symbols for the active file.");
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-outline.png");
  });

  test("keeps Editor focus, selection, and resize highlights subtle", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");

    const selectedFile = page
      .getByRole("tree", { name: "Project files" })
      .getByRole("treeitem", {
        name: "Open file src/workbench/AgentChatPanes.tsx",
      });
    const activeTab = page.getByRole("tab", {
      name: /AgentChatPanes\.tsx/,
    });
    const explorerActivity = page.getByRole("button", {
      name: "Explorer",
      exact: true,
    });
    const persistentStates = await page.evaluate(() => {
      const workbenchNode =
        document.querySelector<HTMLElement>(".workbench--editor");
      const selected = document.querySelector<HTMLElement>(
        ".workbench--editor .tree__row--selected",
      );
      const tab = document.querySelector<HTMLElement>(
        ".workbench--editor .tab--active",
      );
      const activity = document.querySelector<HTMLElement>(
        ".workbench--editor .activity-rail__button--active",
      );
      if (!workbenchNode || !selected || !tab || !activity) return null;
      return {
        focusEdge: getComputedStyle(workbenchNode)
          .getPropertyValue("--highlight-focus-edge")
          .trim(),
        focusHalo: getComputedStyle(workbenchNode)
          .getPropertyValue("--highlight-focus-halo")
          .trim(),
        focusWidth: getComputedStyle(workbenchNode)
          .getPropertyValue("--focus-ring-width")
          .trim(),
        selectedShadow: getComputedStyle(selected).boxShadow,
        tabShadow: getComputedStyle(tab).boxShadow,
        activityIndicatorWidth: getComputedStyle(activity, "::after").width,
      };
    });
    expect(persistentStates).not.toBeNull();
    expect(persistentStates!.focusEdge).toContain("color-mix");
    expect(persistentStates!.focusEdge).toContain("transparent");
    expect(persistentStates!.focusHalo).toContain("color-mix");
    expect(persistentStates!.focusHalo).toContain("transparent");
    expect(persistentStates!.focusWidth).toBe("1px");
    expect(persistentStates!.selectedShadow).toContain("1px");
    expect(persistentStates!.selectedShadow).not.toContain("2px");
    expect(persistentStates!.tabShadow).toContain("1px");
    expect(persistentStates!.activityIndicatorWidth).toBe("1px");
    await expect(selectedFile).toBeVisible();
    await expect(activeTab).toHaveAttribute("aria-selected", "true");
    await expect(explorerActivity).toHaveAttribute("aria-expanded", "true");

    const editorContent = page.getByRole("textbox", {
      name: "Edit src/workbench/AgentChatPanes.tsx",
    });
    await editorContent.focus();
    await expect(page.locator(".file-surface .cm-editor")).toHaveClass(
      /cm-focused/,
    );
    const editorFocus = await page.locator(".file-surface").evaluate((file) => {
      const frame = file.querySelector<HTMLElement>(".cm-editor");
      const content = file.querySelector<HTMLElement>(".cm-content");
      if (!frame || !content) return null;
      return {
        frameOutline: getComputedStyle(frame).outlineWidth,
        // The frame is an overlay above the sticky gutter, so it draws on
        // the editor's ::after (file-editor-surface.css).
        frameShadow: getComputedStyle(frame, "::after").boxShadow,
        frameContent: getComputedStyle(frame, "::after").content,
        contentOutline: getComputedStyle(content).outlineWidth,
      };
    });
    expect(editorFocus).not.toBeNull();
    expect(editorFocus!.frameOutline).toBe("0px");
    expect(editorFocus!.frameShadow).not.toBe("none");
    // Without content the ::after never draws, whatever its shadow says.
    expect(editorFocus!.frameContent).not.toBe("none");
    expect(editorFocus!.contentOutline).toBe("0px");

    await page.getByRole("button", { name: "Search", exact: true }).click();
    const searchTools = page.getByLabel("Search project tools", {
      exact: true,
    });
    const searchbox = searchTools.getByRole("searchbox");
    await searchbox.focus();
    const queryFocus = await searchTools
      .locator(".workspace-search__input")
      .evaluate((input) => {
        const style = getComputedStyle(input);
        return {
          borderWidth: style.borderTopWidth,
          borderColor: style.borderTopColor,
          shadow: style.boxShadow,
        };
      });
    expect(queryFocus.borderWidth).toBe("1px");
    expect(queryFocus.shadow).not.toBe("none");
    expect(queryFocus.borderColor).not.toContain("255, 255, 255");

    await searchTools
      .getByRole("button", { name: "Show Search filters" })
      .click();
    const include = searchTools.getByRole("textbox", {
      name: "Files to include",
    });
    await include.focus();
    const filterFocus = await include.evaluate((input) => {
      const style = getComputedStyle(input);
      return {
        borderWidth: style.borderTopWidth,
        borderColor: style.borderTopColor,
        outlineWidth: style.outlineWidth,
        shadow: style.boxShadow,
      };
    });
    expect(filterFocus).toMatchObject({
      borderWidth: "1px",
      outlineWidth: "0px",
    });
    expect(filterFocus.shadow).not.toBe("none");
    expect(filterFocus.borderColor).not.toContain("255, 255, 255");

    const matchCase = searchTools.getByRole("button", { name: "Match case" });
    await matchCase.click();
    await expect(matchCase).toHaveAttribute("aria-pressed", "true");
    const pressed = await matchCase.evaluate((button) => {
      const style = getComputedStyle(button);
      return { background: style.backgroundColor, shadow: style.boxShadow };
    });
    expect(pressed.background).not.toBe("rgb(41, 65, 38)");
    await expect
      .poll(() =>
        matchCase.evaluate((button) => getComputedStyle(button).boxShadow),
      )
      .toContain("1px");

    const splitter = page.getByRole("separator", {
      name: "Resize Editor tool rail",
    });
    const bounds = await splitter.boundingBox();
    expect(bounds).not.toBeNull();
    await page.mouse.move(bounds!.x + bounds!.width / 2, bounds!.y + 80);
    await searchTools
      .getByRole("textbox", { name: "Files to exclude" })
      .focus();
    await page.keyboard.press("Tab");
    await expect(splitter).toBeFocused();
    const focusedSplitter = await splitter.evaluate((node) => {
      const host = getComputedStyle(node);
      const line = getComputedStyle(node, "::after");
      return {
        focusVisible: node.matches(":focus-visible"),
        outlineStyle: host.outlineStyle,
        lineWidth: line.width,
        lineColor: line.backgroundColor,
        lineShadow: line.boxShadow,
      };
    });
    expect(focusedSplitter).toMatchObject({
      focusVisible: true,
      outlineStyle: "none",
      lineWidth: "1px",
    });
    expect(focusedSplitter.lineColor).not.toBe("rgba(0, 0, 0, 0)");
    expect(focusedSplitter.lineColor).not.toContain("255, 255, 255");
    expect(focusedSplitter.lineShadow).not.toBe("none");

    await page.mouse.down();
    await expect(splitter).toHaveClass(/workbench-sidebar-splitter--active/);
    await expect(workbench).toHaveScreenshot("editor-resizer-states.png");
    await page.mouse.up();

    await page.emulateMedia({ reducedMotion: "reduce" });
    await expect
      .poll(() =>
        matchCase.evaluate(
          (button) => getComputedStyle(button).transitionDuration,
        ),
      )
      .toBe("0s");
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
});

test.describe("Wave 2 200% zoom state", () => {
  test.use({
    viewport: { width: 690, height: 440 },
    deviceScaleFactor: 2,
  });

  test("keeps long Outline rows contained at 200% zoom", async ({ page }) => {
    await openScenario(page, "editor", "editor");
    await page.evaluate(async () => {
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/editorVisualFixtures.ts");
      fixtures.installVisualLongOutlineSymbols();
    });

    await page.getByRole("button", { name: "Outline", exact: true }).click();
    const symbols = page
      .getByLabel("Outline project tools", { exact: true })
      .locator(".outline-card");
    await expect(symbols).toHaveCount(3);
    expect(
      await symbols.evaluateAll((rows) =>
        rows.every(
          (row) => row.clientWidth > 0 && row.scrollWidth <= row.clientWidth,
        ),
      ),
    ).toBe(true);
    await expectNoViewportOverflow(page);
  });
});

// Page zoom is a smaller CSS viewport at a higher device scale, as in the
// other 200% states; fractional scales catch sub-pixel rounding drift.
for (const scale of [1, 1.25, 1.5, 2]) {
  test.describe(`Editor gutter at ${scale * 100}% zoom`, () => {
    test.use({
      viewport: {
        width: Math.round(1380 / scale),
        height: Math.round(880 / scale),
      },
      deviceScaleFactor: scale,
    });

    test("keeps every line number level with its line", async ({ page }) => {
      await openScenario(page, "editor", "editor");
      const drawer = page.locator('.workbench--editor .sidebar[role="dialog"]');
      if (await drawer.isVisible())
        await page.getByRole("button", { name: "Hide left sidebar" }).click();
      const editor = page.locator(".file-surface .cm-editor");
      await expect(editor.locator(".cm-line").first()).toBeVisible();
      const rows = await editor.evaluate((root) => {
        const lines = [...root.querySelectorAll<HTMLElement>(".cm-line")];
        // CodeMirror keeps one invisible sizing element first in the gutter.
        const numbers = [
          ...root.querySelectorAll<HTMLElement>(
            ".cm-lineNumbers .cm-gutterElement",
          ),
        ].filter((element) => element.style.visibility !== "hidden");
        const middle = (element: HTMLElement) => {
          const box = element.getBoundingClientRect();
          return box.top + box.height / 2;
        };
        return lines.map((line, index) => ({
          label: numbers[index]?.textContent ?? null,
          offset: numbers[index]
            ? Math.abs(middle(numbers[index]) - middle(line))
            : null,
        }));
      });
      expect(rows.length).toBeGreaterThan(2);
      rows.forEach((row, index) => {
        expect(row.label).toBe(String(index + 1));
        expect(row.offset).not.toBeNull();
        expect(row.offset!).toBeLessThanOrEqual(1);
      });
    });
  });
}
