import { expect, test, type Locator } from "@playwright/test";

import { EDITOR_VISUAL_SCENARIOS } from "./fixtures/visualScenarioTypes";
import { VISUAL_SCENARIOS } from "./fixtures/visualScenarioCatalog";
import {
  waitForAnimations,
  expectNoViewportOverflow,
  openScenario,
  takeCanonicalScreenshot,
} from "./shared/visualBrowserPrimitives";
import {
  expectFindContained,
  runEditorCanonicalTest,
  settleEditorGutter,
} from "./editorVisualHelpers";

type PickerRect = {
  bottom: number;
  height: number;
  left: number;
  right: number;
  top: number;
  width: number;
};

type PickerGeometryObservation = {
  active: PickerRect | null;
  activeId: string | null;
  activeInListbox: boolean;
  activeRole: string | null;
  current: PickerRect | null;
  focused: boolean;
  input: PickerRect | null;
  listbox: PickerRect | null;
  listboxId: string | null;
  listboxScrollTop: number | null;
  popup: PickerRect | null;
  popupScrollTop: number | null;
  semanticOwnerId: string | null;
  viewport: { height: number; width: number };
};

function requirePickerRect(name: string, rect: PickerRect | null): PickerRect {
  expect(rect, `${name} rect: ${JSON.stringify(rect)}`).not.toBeNull();
  if (!rect) throw new Error(`${name} rect is missing`);
  expect(rect.width, `${name} rect: ${JSON.stringify(rect)}`).toBeGreaterThan(
    0,
  );
  expect(rect.height, `${name} rect: ${JSON.stringify(rect)}`).toBeGreaterThan(
    0,
  );
  return rect;
}

function expectRectInside(
  name: string,
  inner: PickerRect,
  outer: PickerRect | { height: number; width: number },
) {
  const bounds: PickerRect =
    "left" in outer
      ? outer
      : {
          bottom: outer.height,
          height: outer.height,
          left: 0,
          right: outer.width,
          top: 0,
          width: outer.width,
        };
  const detail = `${name}: inner=${JSON.stringify(inner)} outer=${JSON.stringify(bounds)}`;
  expect(inner.left, detail).toBeGreaterThanOrEqual(bounds.left);
  expect(inner.top, detail).toBeGreaterThanOrEqual(bounds.top);
  expect(inner.right, detail).toBeLessThanOrEqual(bounds.right);
  expect(inner.bottom, detail).toBeLessThanOrEqual(bounds.bottom);
}

async function expectPickerGeometry(picker: Locator) {
  await waitForAnimations(
    picker.locator("xpath=ancestor::*[contains(@class, 'menu-popover')][1]"),
  );
  const observation = await picker.evaluate((input) => {
    const listbox = document.getElementById(
      input.getAttribute("aria-controls") ?? "",
    );
    const active = document.getElementById(
      input.getAttribute("aria-activedescendant") ?? "",
    );
    const popup = input.closest<HTMLElement>(".searchable-picker__popover");
    const current = popup?.querySelector(".searchable-picker__current");
    const rect = (node: Element | null): PickerRect | null => {
      if (!node) return null;
      const { bottom, height, left, right, top, width } =
        node.getBoundingClientRect();
      return { bottom, height, left, right, top, width };
    };
    return {
      active: rect(active),
      activeId: input.getAttribute("aria-activedescendant"),
      activeInListbox: Boolean(listbox?.contains(active)),
      activeRole: active?.getAttribute("role") ?? null,
      current: rect(current ?? null),
      focused: input === document.activeElement,
      input: rect(input),
      listbox: rect(listbox),
      listboxId: listbox?.id ?? null,
      listboxScrollTop:
        listbox instanceof HTMLElement ? listbox.scrollTop : null,
      popup: rect(popup),
      popupScrollTop: popup?.scrollTop ?? null,
      semanticOwnerId: input.getAttribute("aria-controls"),
      viewport: { height: window.innerHeight, width: window.innerWidth },
    } satisfies PickerGeometryObservation;
  });
  expect(
    observation.semanticOwnerId,
    `aria-controls: ${JSON.stringify(observation)}`,
  ).toBe(observation.listboxId);
  expect(
    observation.activeId,
    `aria-activedescendant: ${JSON.stringify(observation)}`,
  ).toBeTruthy();
  expect(
    observation.activeRole,
    `active option: ${JSON.stringify(observation)}`,
  ).toBe("option");
  expect(
    observation.activeInListbox,
    `active listbox ownership: ${JSON.stringify(observation)}`,
  ).toBe(true);
  expect(
    observation.focused,
    `focused input: ${JSON.stringify(observation)}`,
  ).toBe(true);

  const input = requirePickerRect("input", observation.input);
  const current = requirePickerRect("current", observation.current);
  const popup = requirePickerRect("popup", observation.popup);
  const listbox = requirePickerRect("listbox", observation.listbox);
  const active = requirePickerRect("active", observation.active);
  for (const [name, rect] of [
    ["input", input],
    ["current", current],
    ["popup", popup],
    ["listbox", listbox],
    ["active", active],
  ] as const) {
    expectRectInside(name, rect, observation.viewport);
  }
  expectRectInside("input in popup", input, popup);
  expectRectInside("current in popup", current, popup);
  expectRectInside("listbox in popup", listbox, popup);
  expectRectInside("active in listbox", active, listbox);
  expect(
    observation.listboxScrollTop,
    `listbox scrollTop: ${JSON.stringify(observation)}`,
  ).toBeGreaterThan(0);
  expect(
    observation.popupScrollTop,
    `popup scrollTop: ${JSON.stringify(observation)}`,
  ).toBe(0);
}

test.describe("Wave 2 canonical visual states", () => {
  test.use({ viewport: { width: 1380, height: 880 } });

  for (const { name, mode } of VISUAL_SCENARIOS.filter(
    (scenario) =>
      EDITOR_VISUAL_SCENARIOS.includes(scenario.name) &&
      scenario.name === "editor",
  )) {
    test(`${name} stays visually stable`, async ({ page }) => {
      await runEditorCanonicalTest(page, name, mode);
    });
  }
});

test.describe("Wave 2 accessibility and responsive states", () => {
  test("keeps Editor settings across a restart", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "editor", "editor");
    await page.getByRole("button", { name: "Open settings" }).click();
    const settings = page.getByRole("region", { name: "Settings" });
    await settings
      .getByRole("combobox", { name: "Font size" })
      .selectOption("16");
    await settings
      .getByRole("combobox", { name: "Tab size" })
      .selectOption("8");
    await settings.getByRole("switch", { name: "Show whitespace" }).check();
    await expect
      .poll(() =>
        page.evaluate(async () => {
          const { useEditorSettingsStore } =
            await import("/src/app/editorSettingsStore.ts");
          return useEditorSettingsStore.getState().persistencePhase;
        }),
      )
      .toBe("idle");

    // The browser lane's port keeps settings for the tab's session, the way
    // the desktop app keeps them in SQLite; reopening the Workbench reloads
    // the page and restores them through the same path.
    await openScenario(page, "editor", "editor");
    await expect(page.locator(".cm-highlightSpace").first()).toBeVisible();
    await expect(page.getByTitle("Indentation")).toHaveText("Spaces: 8");
    await page.getByRole("button", { name: "Open settings" }).click();
    await expect(
      settings.getByRole("combobox", { name: "Font size" }),
    ).toHaveValue("16");
    await expect(
      settings.getByRole("combobox", { name: "Tab size" }),
    ).toHaveValue("8");
    await expect(
      settings.getByRole("switch", { name: "Show whitespace" }),
    ).toBeChecked();
    await expect(
      settings.getByRole("switch", { name: "Line numbers" }),
    ).toBeChecked();
    await expect(settings.getByRole("group", { name: "Text" })).toBeVisible();
    await expect(
      settings.getByRole("group", { name: "Editing" }),
    ).toBeVisible();
  });

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
      settings.getByRole("switch", { name: "Line numbers" }),
    ).toBeChecked();
    await expect(
      settings.getByRole("switch", { name: "Show whitespace" }),
    ).not.toBeChecked();
    await settings
      .getByRole("combobox", { name: "Font size" })
      .selectOption("16");
    await settings.getByRole("switch", { name: "Line numbers" }).uncheck();
    await settings.getByRole("switch", { name: "Show whitespace" }).check();
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
      settings.getByRole("switch", { name: "Line numbers" }),
    ).toBeChecked();
    await expect(
      settings.getByRole("switch", { name: "Show whitespace" }),
    ).not.toBeChecked();
    await expect(workbench).toHaveScreenshot("editor-settings.png");

    const settingsSearch = settings.getByRole("searchbox");
    await settingsSearch.fill("whitespace");
    await expect(
      settings.getByRole("switch", { name: "Show whitespace" }),
    ).toBeVisible();
    await expect(
      settings.getByRole("combobox", { name: "Font size" }),
    ).toHaveCount(0);
    await settingsSearch.fill("");

    await page.evaluate(async () => {
      const { useEditorSettingsStore } =
        await import("/src/app/editorSettingsStore.ts");
      useEditorSettingsStore.setState({
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
    const wordWrap = settings.getByRole("switch", { name: "Word wrap" });
    await wordWrap.focus();
    await page.keyboard.press("Space");
    await expect(
      settings.getByRole("switch", { name: "Word wrap" }),
    ).toBeChecked();
    expect(
      await settings
        .getByRole("searchbox")
        .evaluate((node) => getComputedStyle(node).transitionDuration),
    ).toBe("0s");
    await expect(reset).toBeEnabled();
    await expectNoViewportOverflow(page);
  });

  test("editor-picker-repository selects an alternate repository", async ({
    page,
  }) => {
    const workbench = await openScenario(
      page,
      "editor-picker-repository",
      "editor",
    );
    await page.evaluate(async () => {
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/editorVisualFixtures.ts");
      fixtures.installVisualRepositoryPickerFixture();
    });
    await page
      .getByRole("button", { name: "Source Control", exact: true })
      .click();
    await page.getByRole("button", { name: "Select repository" }).click();
    const picker = page.getByRole("combobox", { name: "Select repository" });
    await picker.press("ArrowDown");
    await picker.press("ArrowDown");
    await picker.press("ArrowDown");
    await picker.press("Enter");
    await expect(
      page.getByRole("button", { name: "Select repository" }),
    ).toContainText("packages/repository-2");
    await page.getByRole("button", { name: "Select repository" }).click();
    await expect(
      page.getByRole("option", {
        name: "Sage — packages/repository-2",
        exact: true,
      }),
    ).toHaveAttribute("aria-selected", "true");
    await expectNoViewportOverflow(page);
    await takeCanonicalScreenshot(workbench, "editor-picker-repository");
  });

  test("editor-picker-repository-filtered retains current and disabled reason hooks", async ({
    page,
  }) => {
    const workbench = await openScenario(
      page,
      "editor-picker-repository-filtered",
      "editor",
    );
    await page.evaluate(async () => {
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/editorVisualFixtures.ts");
      fixtures.installVisualRepositoryPickerFixture();
    });
    await page
      .getByRole("button", { name: "Source Control", exact: true })
      .click();
    await page.getByRole("button", { name: "Select repository" }).click();
    const picker = page.getByRole("combobox", { name: "Select repository" });
    const popup = page.locator(
      ".searchable-picker__popover.menu-popover--editor",
    );
    const listbox = page.getByRole("listbox", {
      name: "Select repository",
      exact: true,
    });
    await expect(popup).toBeVisible();
    await expect(popup.locator(".searchable-picker__current")).toHaveText(
      "Current: Sage — Repository root",
    );
    await picker.fill("read-only");
    const readonlyOption = listbox.getByRole("option", {
      name: "Read-only root — packages/repository-31 Repository root is unavailable.",
      exact: true,
    });
    await expect(readonlyOption).toBeVisible();
    await expect(readonlyOption).toHaveAttribute("aria-disabled", "true");
    await expect(listbox.getByRole("option")).toHaveCount(1);
    await expect(
      listbox.locator('[role="option"]:not([aria-disabled="true"])'),
    ).toHaveCount(0);
    await expect(popup.locator(".searchable-picker__current")).toHaveText(
      "Current: Sage — Repository root",
    );
    await expect(
      listbox.getByText("Repository root is unavailable.", { exact: true }),
    ).toBeVisible();
    await expect
      .poll(() =>
        popup.evaluate((node) => {
          const style = getComputedStyle(node);
          const bounds = node.getBoundingClientRect();
          return (
            style.opacity === "1" &&
            style.visibility === "visible" &&
            bounds.width > 0 &&
            bounds.height > 0
          );
        }),
      )
      .toBe(true);
    await expectNoViewportOverflow(page);
    await takeCanonicalScreenshot(
      workbench,
      "editor-picker-repository-filtered",
    );
  });

  test("editor-picker-narrow keeps the picker keyboard reachable", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    const workbench = await openScenario(
      page,
      "editor-picker-narrow",
      "editor",
    );
    await page.evaluate(async () => {
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/editorVisualFixtures.ts");
      fixtures.installVisualRepositoryPickerFixture();
    });
    await page
      .getByRole("button", { name: "Source Control", exact: true })
      .click();
    await page
      .getByRole("button", { name: "Select repository" })
      .press("Enter");
    const picker = page.getByRole("combobox", { name: "Select repository" });
    await picker.press("End");
    await expect(picker).toBeFocused();
    await expectPickerGeometry(picker);
    await expectNoViewportOverflow(page);
    await takeCanonicalScreenshot(workbench, "editor-picker-narrow");
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

    await waitForAnimations(menu);
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

    await editor.press("ControlOrMeta+f");
    const find = page.getByRole("search", { name: "Find in file" });
    await expect(find).toBeVisible();
    await find.getByRole("textbox", { name: "Find text" }).fill("ChatSurface");
    await expect(find.getByRole("status")).toContainText("matches");
    expect(await editorPlane.boundingBox()).toEqual(before);
    await expectFindContained(page);
    await settleEditorGutter(page);
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
    await expect(
      page.getByRole("button", { name: "Select repository", exact: true }),
    ).toBeVisible();
    await expect(
      page.getByRole("textbox", {
        name: "Edit src/workbench/AgentChatPanes.tsx",
      }),
    ).toContainText("export function AgentChatPanes");
    await expect(
      page.locator(".file-surface .cm-content .cm-line span[class]").first(),
    ).toBeVisible();
    await expect(
      page.getByRole("status").filter({ hasText: "Loading Source Control…" }),
    ).toHaveCount(0);
    await expect(
      page.getByRole("status").filter({ hasText: "Loading File…" }),
    ).toHaveCount(0);
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
    const drawer = page.locator('.workbench--editor .sidebar[role="dialog"]');
    const center = page.locator(".workbench--editor .center-region");
    await expect(drawer).toBeVisible();
    await expect(center).toHaveAttribute("inert", "");
    await expect(center).toHaveAttribute("aria-hidden", "true");
    await page.getByRole("button", { name: "Hide left sidebar" }).click();
    await expect(
      page.getByRole("textbox", {
        name: "Edit src/workbench/AgentChatPanes.tsx",
        includeHidden: true,
      }),
    ).toContainText("export function AgentChatPanes");
    await expect(
      page
        .getByRole("status", { includeHidden: true })
        .filter({ hasText: "Loading editor…" }),
    ).toHaveCount(0);
    await expect(
      center.locator(".file-surface .cm-content .cm-line span[class]").first(),
    ).toBeAttached();
    await page.getByRole("button", { name: "Show left sidebar" }).click();
    await expect(center).toHaveAttribute("inert", "");
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
          return (
            drawerBounds.left <= centerBounds.left &&
            drawerBounds.top <= centerBounds.top &&
            drawerBounds.right >= centerBounds.right &&
            drawerBounds.bottom >= centerBounds.bottom
          );
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
    // Git change markers arrive after the file renders; capture them settled.
    await settleEditorGutter(page);
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
    await expect(
      page.getByRole("button", { name: "Show left sidebar" }),
    ).toBeVisible();

    const settings = page.getByRole("region", { name: "Settings" });
    await expect(settings).toBeVisible();
    await expect(settings.getByRole("searchbox")).toBeVisible();
    await expect(
      settings.getByRole("combobox", { name: "Font size" }),
    ).toBeVisible();
    await expectNoViewportOverflow(page);
  });

  test("editor-picker-zoom-200 remains contained at the DPR proxy", async ({
    page,
  }) => {
    const workbench = await openScenario(
      page,
      "editor-picker-zoom-200",
      "editor",
    );
    await page.evaluate(async () => {
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/editorVisualFixtures.ts");
      fixtures.installVisualRepositoryPickerFixture();
    });
    await page
      .getByRole("button", { name: "Source Control", exact: true })
      .click();
    await page.getByRole("button", { name: "Select repository" }).click();
    await expect(
      page.getByRole("combobox", { name: "Select repository" }),
    ).toBeVisible();
    const picker = page.getByRole("combobox", { name: "Select repository" });
    await picker.press("PageDown");
    await expect(picker).toBeFocused();
    await expectPickerGeometry(picker);
    await expectNoViewportOverflow(page);
    await takeCanonicalScreenshot(workbench, "editor-picker-zoom-200");
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
    await page
      .getByRole("button", { name: "Source Control", exact: true })
      .click();
    const renameTrigger = page.getByRole("button", {
      name: "Branch to rename",
    });
    await expect(renameTrigger).toHaveText("feature/editor-polish");
    await renameTrigger.click();
    const renameInput = page.getByRole("combobox", {
      name: "Branch to rename",
    });
    const renameListbox = page.getByRole("listbox", {
      name: "Branch to rename",
    });
    await expect(renameInput).toBeFocused();
    await expect(renameListbox.getByRole("option")).toHaveCount(3);
    await expect(
      renameListbox.getByRole("option", {
        name: "feature/editor-polish",
        exact: true,
      }),
    ).toHaveAttribute("aria-selected", "true");
    await expect(
      page.getByText("Current: feature/editor-polish", { exact: true }),
    ).toBeVisible();
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowDown");
    await expect(renameInput).toHaveAttribute(
      "aria-activedescendant",
      /option-main$/,
    );
    await page.keyboard.press("Enter");
    await expect(renameTrigger).toHaveText("main");
    await expect(renameTrigger).toBeFocused();
    await renameTrigger.click();
    const renamePopup = renameInput.locator(
      "xpath=ancestor::div[contains(concat(' ', normalize-space(@class), ' '), ' menu-popover ')][1]",
    );
    await expect(renameInput).toBeFocused();
    await expect(renamePopup).toBeVisible();
    await expect(renameListbox).toBeVisible();
    await waitForAnimations(renamePopup);
    const [renameInputBox, renameListboxBox, renamePopupBox] =
      await Promise.all([
        renameInput.boundingBox(),
        renameListbox.boundingBox(),
        renamePopup.boundingBox(),
      ]);
    expect(renameInputBox).not.toBeNull();
    expect(renameListboxBox).not.toBeNull();
    expect(renamePopupBox).not.toBeNull();
    if (renameInputBox && renameListboxBox && renamePopupBox) {
      const viewport = await page.evaluate(() => ({
        height: window.innerHeight,
        width: window.innerWidth,
      }));
      for (const box of [renameInputBox, renameListboxBox, renamePopupBox]) {
        expect(box.width).toBeGreaterThan(0);
        expect(box.height).toBeGreaterThan(0);
        expect(box.x).toBeGreaterThanOrEqual(0);
        expect(box.y).toBeGreaterThanOrEqual(0);
        expect(box.x + box.width).toBeLessThanOrEqual(viewport.width);
        expect(box.y + box.height).toBeLessThanOrEqual(viewport.height);
      }
      expect(renameInputBox.x).toBeGreaterThanOrEqual(renamePopupBox.x);
      expect(renameInputBox.y).toBeGreaterThanOrEqual(renamePopupBox.y);
      expect(renameInputBox.x + renameInputBox.width).toBeLessThanOrEqual(
        renamePopupBox.x + renamePopupBox.width,
      );
      expect(renameInputBox.y + renameInputBox.height).toBeLessThanOrEqual(
        renamePopupBox.y + renamePopupBox.height,
      );
      expect(renameListboxBox.x).toBeGreaterThanOrEqual(renamePopupBox.x);
      expect(renameListboxBox.y).toBeGreaterThanOrEqual(renamePopupBox.y);
      expect(renameListboxBox.x + renameListboxBox.width).toBeLessThanOrEqual(
        renamePopupBox.x + renamePopupBox.width,
      );
      expect(renameListboxBox.y + renameListboxBox.height).toBeLessThanOrEqual(
        renamePopupBox.y + renamePopupBox.height,
      );
    }
    await expectNoViewportOverflow(page);
    await page.keyboard.press("Escape");
    await expect(renameTrigger).toBeFocused();
    await page.getByRole("button", { name: "Explorer", exact: true }).click();
    await page.evaluate(async () => {
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/editorVisualFixtures.ts");
      fixtures.installVisualExplorerActionFixture();
    });
    await page.getByRole("button", { name: "More Explorer actions" }).click();
    const explorerMenu = page.getByRole("menu", { name: "Explorer actions" });
    await expect(explorerMenu).toBeVisible();
    await waitForAnimations(explorerMenu);
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
    await editorContent.press("ControlOrMeta+f");
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
