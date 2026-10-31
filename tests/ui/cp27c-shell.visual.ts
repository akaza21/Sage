import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  expectNoViewportOverflow,
  openScenario,
} from "./shared/visualBrowserPrimitives";

type Bounds = { x: number; y: number; width: number; height: number };

// Sub-pixel layout (fractional row heights, scrollIntoView alignment) may
// overshoot a container edge by less than one device pixel.
const SUBPIXEL = 0.5;

function expectInside(bounds: Bounds, container: Bounds) {
  expect(bounds.x).toBeGreaterThanOrEqual(container.x - SUBPIXEL);
  expect(bounds.y).toBeGreaterThanOrEqual(container.y - SUBPIXEL);
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(
    container.x + container.width + SUBPIXEL,
  );
  expect(bounds.y + bounds.height).toBeLessThanOrEqual(
    container.y + container.height + SUBPIXEL,
  );
}

function expectDisjoint(first: Bounds, second: Bounds) {
  expect(
    first.x + first.width <= second.x ||
      second.x + second.width <= first.x ||
      first.y + first.height <= second.y ||
      second.y + second.height <= first.y,
  ).toBe(true);
}

async function expectContained(locator: Locator, page: Page) {
  await expect(locator).toBeVisible();
  const bounds = await locator.boundingBox();
  expect(bounds).not.toBeNull();
  const viewport = page.viewportSize()!;
  expect(bounds!.width).toBeGreaterThan(0);
  expect(bounds!.height).toBeGreaterThan(0);
  expectInside(bounds!, { x: 0, y: 0, ...viewport });
  return bounds!;
}

async function expectHitTestable(control: Locator) {
  expect(
    await control.evaluate((node) => {
      const rect = node.getBoundingClientRect();
      return node.contains(
        document.elementFromPoint(
          rect.x + rect.width / 2,
          rect.y + rect.height / 2,
        ),
      );
    }),
  ).toBe(true);
}

async function expectHitTarget(control: Locator, page: Page) {
  const bounds = await expectContained(control, page);
  expect(bounds.width).toBeGreaterThanOrEqual(24);
  expect(bounds.height).toBeGreaterThanOrEqual(24);
  await expectHitTestable(control);
}

/**
 * The 24px status bar holds 23px controls under its 1px top border (an
 * established, pre-CP-27c size). They stay reachable when they fill the
 * bar's inner height, are at least 24px wide and receive the hit.
 */
async function expectStatusBarTarget(control: Locator, page: Page) {
  const bounds = await expectContained(control, page);
  const bar = await expectContained(page.locator(".status-bar"), page);
  expectInside(bounds, bar);
  expect(bounds.width).toBeGreaterThanOrEqual(24);
  expect(bounds.height).toBeGreaterThanOrEqual(bar.height - 1);
  await expectHitTestable(control);
}

async function expectTitleBar(page: Page) {
  const titleBar = page.locator(".title-bar");
  const bounds = await expectContained(titleBar, page);
  // One row: never the old wrapped two-row narrow title bar.
  expect(bounds.height).toBeLessThanOrEqual(48);
  let previousRight = bounds.x;
  for (const area of ["start", "center", "end"]) {
    const rect = await expectContained(
      titleBar.locator(`.title-bar__${area}`),
      page,
    );
    expectInside(rect, bounds);
    expect(rect.x).toBeGreaterThanOrEqual(previousRight);
    previousRight = rect.x + rect.width;
  }
  const modes: Bounds[] = [];
  for (const mode of ["Agent", "Editor"]) {
    const label = titleBar
      .getByRole("radio", { name: mode, exact: true })
      .locator("..");
    await expectHitTarget(label, page);
    await expect(label).toHaveText(mode);
    expect(
      await label.evaluate((node) => node.scrollWidth <= node.clientWidth),
    ).toBe(true);
    modes.push((await label.boundingBox())!);
  }
  expect(modes[0].y).toBe(modes[1].y);
  expectDisjoint(modes[0], modes[1]);
}

async function editorControlsReachable(page: Page) {
  await openScenario(page, "editor", "editor");
  await expectTitleBar(page);
  await page.getByRole("button", { name: "Hide left sidebar" }).click();
  const editor = page.getByRole("textbox", {
    name: "Edit src/workbench/AgentChatPanes.tsx",
    exact: true,
  });
  await expect(editor).toBeVisible();
  await expectContained(page.locator(".file-editor-stage"), page);
  const search = page
    .locator(".title-bar")
    .getByRole("button", { name: "Search files", exact: true });
  await expectHitTarget(search, page);
  // Icon-only or labelled, the pill never clips its label mid-word.
  expect(
    await search.evaluate((node) => node.scrollWidth <= node.clientWidth),
  ).toBe(true);
  await editor.press("ControlOrMeta+f");
  const find = page.getByRole("search", { name: "Find in file" });
  await find.getByRole("textbox", { name: "Find text" }).fill("ChatSurface");
  await expectContained(find, page);
  for (const name of [
    "Previous match",
    "Next match",
    "Match case",
    "Close find",
  ]) {
    await expectHitTarget(find.getByRole("button", { name }), page);
  }
  await find.getByRole("button", { name: "Close find" }).click();
  await search.click();
  const drawer = page.locator('.workbench--editor .sidebar[role="dialog"]');
  // At 760px and below the tool sidebar is a modal drawer over the editor.
  await expectContained(drawer, page);
  await expectStatusBarTarget(
    page.getByRole("button", { name: "Hide left sidebar" }),
    page,
  );
  await page.keyboard.press("Escape");
  await expect(drawer).toHaveCount(0);
  await expectNoViewportOverflow(page);
}

async function settingsContained(page: Page) {
  await openScenario(page, "editor", "editor");
  await page.getByRole("button", { name: "Hide left sidebar" }).click();
  await page.getByRole("button", { name: "Open settings" }).click();
  await expectTitleBar(page);
  const settings = page.getByRole("region", { name: "Settings", exact: true });
  const surface = await expectContained(settings, page);
  await expectHitTarget(settings.getByRole("searchbox"), page);
  const fontSize = settings.getByRole("combobox", { name: "Font size" });
  await fontSize.scrollIntoViewIfNeeded();
  await expectHitTarget(fontSize, page);
  await settings.getByRole("tab", { name: "Keyboard", exact: true }).click();
  const rows = settings.locator(".settings-surface__keymap tr");
  await expect(rows.first()).toBeVisible();
  for (const row of await rows.all()) {
    await row.scrollIntoViewIfNeeded();
    const rowBounds = await expectContained(row, page);
    expectInside(rowBounds, surface);
    const label = await expectContained(row.locator("th"), page);
    const shortcut = await expectContained(row.locator("td"), page);
    expectInside(label, rowBounds);
    expectInside(shortcut, rowBounds);
    expectDisjoint(label, shortcut);
  }
  await settings.getByRole("tab", { name: "Editor", exact: true }).click();
  await page.evaluate(async () => {
    const { useEditorSettingsStore } =
      await import("/src/app/editorSettingsStore.ts");
    useEditorSettingsStore.setState({
      persistencePhase: "error",
      persistenceMessage:
        "Editor settings could not be saved. Your changes remain in this window; retry when storage is available.",
    });
  });
  const error = settings.getByRole("alert");
  await error.scrollIntoViewIfNeeded();
  expectInside(await expectContained(error, page), surface);
  await expectHitTarget(error.getByRole("button", { name: "Retry" }), page);
  // The last setting stays reachable: Settings scrolls inside its own
  // surface instead of being clipped by the editor canvas.
  const reset = settings.getByRole("button", {
    name: "Reset Editor Appearance",
  });
  await reset.scrollIntoViewIfNeeded();
  expectInside(await expectContained(reset, page), surface);
  await expectHitTestable(reset);
  await expectNoViewportOverflow(page);
}

for (const width of [480, 720]) {
  for (const forcedColors of ["none", "active"] as const) {
    test.describe(`CP-27c ${width}x560 ${forcedColors}`, () => {
      test.use({ viewport: { width, height: 560 }, forcedColors });

      test("keeps Editor file, find and tool drawer controls reachable", async ({
        page,
      }) => {
        await editorControlsReachable(page);
      });

      test("contains Settings selectors, keyboard rows and persistence errors", async ({
        page,
      }) => {
        await settingsContained(page);
      });

      test("keeps Editor Diff scrolling local and terminal error controls reachable", async ({
        page,
      }) => {
        await openScenario(page, "editor", "editor");
        await page.evaluate(async () => {
          const { useWorkbenchStore } = await import("/src/app/store.ts");
          const { fileResource, visualDiff } =
            await import("/tests/ui/fixtures/visualDocumentFixtures.ts");
          const { diffCacheKey } = await import("/src/domain/diffIdentity.ts");
          await useWorkbenchStore.getState().openDiff(fileResource);
          useWorkbenchStore.setState((state) => ({
            diffs: {
              ...state.diffs,
              [diffCacheKey(undefined, fileResource)]: {
                phase: "ready",
                value: visualDiff,
              },
            },
          }));
          Object.defineProperty(window, "__TAURI_INTERNALS__", {
            configurable: true,
            value: {
              invoke: async (command: string) => {
                if (command !== "terminal_create")
                  throw new Error(
                    `Unsupported geometry fixture command: ${command}`,
                  );
                throw { code: "unavailable" };
              },
            },
          });
        });
        await page.getByRole("button", { name: "Hide left sidebar" }).click();
        const surface = page.locator(".diff-surface");
        const surfaceBefore = await expectContained(surface, page);
        const scroll = surface.locator(".diff-editor");
        expectInside(await expectContained(scroll, page), surfaceBefore);
        await expect(scroll).toHaveCSS("overflow-x", "auto");
        // Horizontal diff scrolling stays inside the diff editor: scrolling it
        // to its far edge never scrolls the surface or the page.
        const scrolled = await scroll.evaluate((node) => {
          node.scrollLeft = node.scrollWidth;
          const surfaceNode = node.closest(".diff-surface")!;
          return {
            local: node.scrollLeft,
            max: node.scrollWidth - node.clientWidth,
            surface: surfaceNode.scrollLeft,
            page: document.scrollingElement!.scrollLeft,
          };
        });
        expect(Math.abs(scrolled.local - scrolled.max)).toBeLessThanOrEqual(1);
        expect(scrolled.surface).toBe(0);
        expect(scrolled.page).toBe(0);
        expect(await surface.boundingBox()).toEqual(surfaceBefore);
        await expectHitTarget(
          surface.getByRole("button", { name: "Refresh diff" }),
          page,
        );
        await expectHitTarget(
          surface.getByRole("button", { name: "Open file", exact: true }),
          page,
        );
        await page
          .getByRole("button", { name: "Open terminal", exact: true })
          .click();
        const terminal = page.getByRole("region", {
          name: "Terminal",
          exact: true,
        });
        const expanded = await expectContained(terminal, page);
        const diffBounds = await expectContained(surface, page);
        expect(diffBounds.y + diffBounds.height).toBeLessThanOrEqual(
          expanded.y,
        );
        await expect(terminal.getByRole("status")).toHaveText(
          "Terminal is unavailable on this platform.",
        );
        await expectHitTarget(
          terminal.getByRole("button", { name: "New terminal", exact: true }),
          page,
        );
        const collapse = terminal.getByRole("button", {
          name: "Collapse terminal",
          exact: true,
        });
        await expectHitTarget(collapse, page);
        await collapse.click();
        const expand = terminal.getByRole("button", {
          name: "Expand terminal",
          exact: true,
        });
        await expectHitTarget(expand, page);
        const collapsed = await expectContained(terminal, page);
        expect(collapsed.height).toBeLessThan(expanded.height);
        expectInside((await expand.boundingBox())!, collapsed);
        const diffWithCollapsed = await expectContained(surface, page);
        expect(diffWithCollapsed.height).toBeGreaterThan(diffBounds.height);
        expect(
          diffWithCollapsed.y + diffWithCollapsed.height,
        ).toBeLessThanOrEqual(collapsed.y);
        await expand.click();
        expect((await expectContained(terminal, page)).height).toBe(
          expanded.height,
        );
        await expectNoViewportOverflow(page);
      });

      test("contains a new-file dialog with an unbroken path and returns focus", async ({
        page,
      }) => {
        await openScenario(page, "editor", "editor");
        await page.evaluate(async () => {
          const fixtures =
            await import("/tests/ui/fixtures/surfaces/editorVisualFixtures.ts");
          fixtures.installVisualExplorerActionFixture();
        });
        const opener = page
          .getByLabel("Explorer project tools", { exact: true })
          .getByRole("button", { name: "New File", exact: true });
        await expectHitTarget(opener, page);
        await opener.click();
        const dialog = page.getByRole("dialog", {
          name: "New File",
          exact: true,
        });
        const input = dialog.getByRole("textbox", {
          name: "File name",
          exact: true,
        });
        const basename = `${"unbroken".repeat(24)}.txt`;
        await input.fill(basename);
        await expectContained(dialog, page);
        await expectHitTarget(input, page);
        const preview = dialog.locator(".new-file-dialog__preview");
        await expect(preview).toContainText(basename);
        await expectContained(preview, page);
        const contentBounds = await dialog.evaluate((node) => {
          const bounds = node.getBoundingClientRect();
          const style = getComputedStyle(node);
          const left =
            parseFloat(style.borderLeftWidth) + parseFloat(style.paddingLeft);
          const right =
            parseFloat(style.borderRightWidth) + parseFloat(style.paddingRight);
          const top =
            parseFloat(style.borderTopWidth) + parseFloat(style.paddingTop);
          const bottom =
            parseFloat(style.borderBottomWidth) +
            parseFloat(style.paddingBottom);
          return {
            x: bounds.x + left,
            y: bounds.y + top,
            width: bounds.width - left - right,
            height: bounds.height - top - bottom,
          };
        });
        expect(
          await preview.evaluate(
            (node) =>
              node.scrollWidth <= node.clientWidth &&
              node.scrollHeight <= node.clientHeight,
          ),
        ).toBe(true);
        const cancel = dialog.getByRole("button", {
          name: "Cancel",
          exact: true,
        });
        const create = dialog.getByRole("button", {
          name: "Create",
          exact: true,
        });
        await expectHitTarget(cancel, page);
        await expectHitTarget(create, page);
        for (const content of [input, preview, cancel, create]) {
          expectInside((await content.boundingBox())!, contentBounds);
        }
        expectDisjoint(
          (await cancel.boundingBox())!,
          (await create.boundingBox())!,
        );
        await cancel.click();
        await expect(dialog).toHaveCount(0);
        await expect(opener).toBeFocused();
      });

      test("preserves the labelled mode toggle with a long Agent title", async ({
        page,
      }) => {
        await openScenario(page, "agent-dual", "agent");
        const longTitle = `Chat: ${"Long title ".repeat(20).trim()}`;
        await page.evaluate(async (title) => {
          const { useWorkbenchStore } = await import("/src/app/store.ts");
          const { acpProjectionStoreFor } =
            await import("/src/app/acpProjectionRegistry.ts");
          const { acpDesktop } = await import("/src/services/acpDesktop.ts");
          const chats = useWorkbenchStore
            .getState()
            .tabs.filter((tab) => tab.kind === "chat");
          // The projection title is canonical and mirrors into the tab, so
          // the long title must come from every native source, including
          // the page a remounted Agent view reloads.
          const load = acpDesktop.getChatPage;
          acpDesktop.getChatPage = async (...request) => ({
            ...(await load(...request)),
            title,
          });
          for (const chat of chats) {
            const current = acpProjectionStoreFor(chat.id as never).getState();
            if (current.page && current.scope)
              current.applyAuthoritativePage(current.scope, {
                ...current.page,
                title,
              });
          }
          useWorkbenchStore.setState((state) => ({
            tabs: state.tabs.map((tab) =>
              tab.kind === "chat" ? { ...tab, title } : tab,
            ),
          }));
        }, longTitle);
        const titleBar = page.locator(".title-bar");
        const session = titleBar.getByRole("heading", { level: 1 });
        await expect(session).toContainText("Long title Long title");
        await expectTitleBar(page);
        const center = await expectContained(
          titleBar.locator(".title-bar__center"),
          page,
        );
        expectInside((await session.boundingBox())!, center);
        // The title is capped in JS and, when still too wide, ellipsized on
        // one line inside the centre track; it never pushes the toggle.
        expect(
          await session.evaluate((node) => {
            const style = getComputedStyle(node);
            return (
              style.textOverflow === "ellipsis" &&
              style.whiteSpace === "nowrap" &&
              node.textContent!.endsWith("…")
            );
          }),
        ).toBe(true);
        await expectHitTarget(
          titleBar.getByRole("button", { name: "Search chats" }),
          page,
        );
        await titleBar
          .getByRole("radio", { name: "Editor", exact: true })
          .locator("..")
          .click();
        await expectTitleBar(page);
        await expectHitTarget(
          titleBar.getByRole("button", { name: "Search files", exact: true }),
          page,
        );
        await expectHitTarget(
          titleBar.getByRole("button", { name: "Open settings" }),
          page,
        );
        await titleBar
          .getByRole("radio", { name: "Agent", exact: true })
          .locator("..")
          .click();
        await expectTitleBar(page);
        await expect(session).toContainText("Long title Long title");
      });
    });
  }
}

// The window is 480 wide at minimum but may be tall; Editor and Settings
// must use the extra height without leaving controls outside the viewport.
test.describe("CP-27c 480x1000 none", () => {
  test.use({ viewport: { width: 480, height: 1000 } });

  test("keeps Editor file, find and tool drawer controls reachable", async ({
    page,
  }) => {
    await editorControlsReachable(page);
  });

  test("contains Settings selectors, keyboard rows and persistence errors", async ({
    page,
  }) => {
    await settingsContained(page);
  });
});
