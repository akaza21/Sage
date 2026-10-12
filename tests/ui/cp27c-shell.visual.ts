import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  expectNoViewportOverflow,
  openScenario,
} from "./shared/visualBrowserPrimitives";

type Bounds = { x: number; y: number; width: number; height: number };

function expectInside(bounds: Bounds, container: Bounds) {
  expect(bounds.x).toBeGreaterThanOrEqual(container.x);
  expect(bounds.y).toBeGreaterThanOrEqual(container.y);
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(
    container.x + container.width,
  );
  expect(bounds.y + bounds.height).toBeLessThanOrEqual(
    container.y + container.height,
  );
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

async function expectHitTarget(control: Locator, page: Page) {
  const bounds = await expectContained(control, page);
  expect(bounds.width).toBeGreaterThanOrEqual(24);
  expect(bounds.height).toBeGreaterThanOrEqual(24);
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

async function expectTitleBar(page: Page) {
  const titleBar = page.locator(".title-bar");
  const bounds = await expectContained(titleBar, page);
  const agent = titleBar.getByRole("radio", { name: "Agent", exact: true });
  expect(bounds.height).toBe((await agent.isChecked()) ? 48 : 40);
  let previousRight = bounds.x;
  for (const area of ["start", "center", "end"]) {
    const rect = await expectContained(
      titleBar.locator(`.title-bar__${area}`),
      page,
    );
    expect(rect.x).toBeGreaterThanOrEqual(previousRight);
    previousRight = rect.x + rect.width;
  }
  for (const mode of ["Agent", "Editor"]) {
    await expectHitTarget(
      titleBar.getByRole("radio", { name: mode, exact: true }).locator(".."),
      page,
    );
  }
}

for (const width of [480, 720]) {
  for (const forcedColors of ["none", "active"] as const) {
    test.describe(`CP-27c ${width}x560 ${forcedColors}`, () => {
      test.use({ viewport: { width, height: 560 }, forcedColors });

      test("keeps Editor file, find and tool drawer controls reachable", async ({
        page,
      }) => {
        await openScenario(page, "editor", "editor");
        await expectTitleBar(page);
        await page.getByRole("button", { name: "Hide left sidebar" }).click();
        const editor = page.getByRole("textbox", {
          name: "Edit src/workbench/AgentChatPanes.tsx",
          exact: true,
        });
        await expect(editor).toBeVisible();
        await expectContained(page.locator(".file-editor-stage"), page);
        const search = page.getByRole("button", { name: "Search files" });
        await expectHitTarget(search, page);
        await editor.press("Control+f");
        const find = page.getByRole("search", { name: "Find in file" });
        await find
          .getByRole("textbox", { name: "Find text" })
          .fill("ChatSurface");
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
        const drawer = page.locator(
          '.workbench--editor .sidebar[role="dialog"]',
        );
        await expectContained(drawer, page);
        await expectHitTarget(
          page.getByRole("button", { name: "Hide left sidebar" }),
          page,
        );
        await page.keyboard.press("Escape");
        await expect(drawer).toHaveCount(0);
        await expectNoViewportOverflow(page);
      });

      test("contains Settings selectors, keyboard rows and persistence errors", async ({
        page,
      }) => {
        await openScenario(page, "editor", "editor");
        await page.getByRole("button", { name: "Hide left sidebar" }).click();
        await page.getByRole("button", { name: "Open settings" }).click();
        await expectTitleBar(page);
        const settings = page.getByRole("region", {
          name: "Settings",
          exact: true,
        });
        await expectContained(settings, page);
        await expectHitTarget(settings.getByRole("searchbox"), page);
        await expectHitTarget(
          settings.getByRole("combobox", { name: "Font size" }),
          page,
        );
        await settings
          .getByRole("tab", { name: "Keyboard", exact: true })
          .click();
        const rows = settings.locator(".settings-surface__keymap tr");
        await expect(rows.first()).toBeVisible();
        for (const row of await rows.all()) {
          await row.scrollIntoViewIfNeeded();
          const rowBounds = await expectContained(row, page);
          const label = await expectContained(row.locator("th"), page);
          const shortcut = await expectContained(row.locator("td"), page);
          expectInside(label, rowBounds);
          expectInside(shortcut, rowBounds);
          expect(
            label.x + label.width <= shortcut.x ||
              shortcut.x + shortcut.width <= label.x ||
              label.y + label.height <= shortcut.y ||
              shortcut.y + shortcut.height <= label.y,
          ).toBe(true);
        }
        await settings
          .getByRole("tab", { name: "Editor", exact: true })
          .click();
        await page.evaluate(async () => {
          const { useEditorAppearanceStore } =
            await import("/src/app/editorAppearanceStore.ts");
          useEditorAppearanceStore.setState({
            persistencePhase: "error",
            persistenceMessage:
              "Editor settings could not be saved. Your changes remain in this window; retry when storage is available.",
          });
        });
        const error = settings.getByRole("alert");
        await error.scrollIntoViewIfNeeded();
        await expectContained(error, page);
        await expectHitTarget(
          error.getByRole("button", { name: "Retry" }),
          page,
        );
        await expectNoViewportOverflow(page);
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
        await expectContained(surface, page);
        const scroll = surface.locator(".diff-editor");
        await expectContained(scroll, page);
        await expect(scroll).toHaveCSS("overflow-x", "auto");
        expect(
          await scroll.evaluate((node) => node.scrollWidth > node.clientWidth),
        ).toBe(true);
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
        const terminalBounds = await expectContained(terminal, page);
        const diffBounds = await expectContained(surface, page);
        expect(diffBounds.y + diffBounds.height).toBeLessThanOrEqual(
          terminalBounds.y,
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
        expect((await expectContained(terminal, page)).height).toBe(34);
        const expand = terminal.getByRole("button", {
          name: "Expand terminal",
          exact: true,
        });
        await expectHitTarget(expand, page);
        await expand.click();
        await expectContained(terminal, page);
        await expectNoViewportOverflow(page);
      });

      test("contains a new-file dialog with an unbroken path and returns focus", async ({
        page,
      }) => {
        await openScenario(page, "editor", "editor");
        const opener = page
          .getByRole("button", { name: "New file", exact: true })
          .first();
        await expectHitTarget(opener, page);
        await opener.click();
        const dialog = page.getByRole("dialog", {
          name: "New file",
          exact: true,
        });
        const input = dialog.getByRole("textbox", {
          name: "file name",
          exact: false,
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
        const cancelBounds = await cancel.boundingBox();
        const createBounds = await create.boundingBox();
        expect(cancelBounds!.x + cancelBounds!.width).toBeLessThanOrEqual(
          createBounds!.x,
        );
        await cancel.click();
        await expect(dialog).toHaveCount(0);
        await expect(opener).toBeFocused();
      });

      test("preserves the labelled mode toggle with a long Agent title", async ({
        page,
      }) => {
        await openScenario(page, "editor", "editor");
        await page.evaluate(async () => {
          const { useWorkbenchStore } = await import("/src/app/store.ts");
          useWorkbenchStore.setState((state) => ({
            tabs: state.tabs.map((tab) =>
              tab.kind === "chat"
                ? { ...tab, title: `Chat: ${"Long title ".repeat(20)}` }
                : tab,
            ),
          }));
        });
        const titleBar = page.locator(".title-bar");
        await titleBar
          .getByRole("radio", { name: "Agent", exact: true })
          .locator("..")
          .click();
        await expectTitleBar(page);
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
          titleBar.getByRole("button", { name: "Open settings" }),
          page,
        );
      });
    });
  }
}
