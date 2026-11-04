import { expect, test, type Page } from "@playwright/test";

import { openScenario } from "./shared/visualBrowserPrimitives";

// EP-08 Quick Open, driven through real keys in Chromium. The browser
// runtime has no native index, so a stand-in for `quick_open_query` answers
// by substring, lists the recent Files the UI sends for an empty query, and
// takes a while on its first call, as a cold index does.
async function installNativeStandIn(page: Page, files: string[]) {
  await page.evaluate(async (paths) => {
    const { setQuickOpenInvoke } =
      await import("/src/features/quickOpen/quickOpenGateway.ts");
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const rootId = useWorkbenchStore.getState().project!.roots[0].id;
    // Calls in the 600 ms after the first one wait until then, as on a cold
    // index (React runs the first query's effect twice in development).
    // Counted from the first call, not from here, so a slow first open of
    // the box can't use the cold window up before it asks.
    let coldUntil = 0;
    setQuickOpenInvoke(async (_command, args) => {
      if (coldUntil === 0) coldUntil = Date.now() + 600;
      const request = args.request as {
        query: string;
        recent: Array<{ rootId: string; relativePath: string }>;
      };
      const cold = Date.now() < coldUntil;
      if (cold) {
        await new Promise((resolve) =>
          setTimeout(resolve, coldUntil - Date.now()),
        );
      }
      const query = request.query.toLowerCase();
      const results = query
        ? paths
            .filter((path) => path.toLowerCase().includes(query))
            .map((relativePath) => {
              const at = relativePath.toLowerCase().indexOf(query);
              return {
                rootId,
                relativePath,
                positions: [...query].map((_, index) => at + index),
              };
            })
        : request.recent.map((resource) => ({ ...resource, positions: [] }));
      return {
        state: "ready",
        results,
        indexedFiles: paths.length,
        truncated: false,
        built: cold,
        refreshing: false,
        indexBytes: 0,
        readyMs: 0,
      };
    });
  }, files);
}

async function activeFile(page: Page) {
  return page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const state = useWorkbenchStore.getState();
    return (
      state.tabs.find((tab) => tab.id === state.activeTabId)?.resource
        ?.relativePath ?? null
    );
  });
}

test.describe("Command palette (ED-2a)", () => {
  test("opens with Ctrl/Cmd+Shift+P and runs the Editor's commands", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "editor", "editor");
    await expect(page.locator(".cm-content")).toBeVisible();
    const dialog = page.getByRole("dialog", { name: "Quick Open" });
    const box = dialog.getByRole("combobox", { name: "Run a command" });
    const options = dialog.getByRole("option");

    await page.keyboard.press("ControlOrMeta+Shift+p");
    await expect(box).toBeFocused();
    await expect(box).toHaveValue(">");
    await box.fill(">go to line");
    await expect(options).toHaveCount(1);
    await expect(options.first()).toContainText("Go to Line…");
    // Its own shortcut is shown (Control+G on every platform).
    await expect(options.first().locator(".quick-open__shortcut")).toHaveText(
      "Ctrl+G",
    );
    await box.press("Enter");
    await expect(dialog).toBeHidden();
    await expect(
      page.getByRole("textbox", { name: "Go to line" }),
    ).toBeFocused();
    await page.keyboard.press("Escape");

    await page.keyboard.press("ControlOrMeta+Shift+p");
    await box.fill(">show search");
    await box.press("Enter");
    await expect(dialog).toBeHidden();
    await expect
      .poll(() =>
        page.evaluate(async () => {
          const { useWorkbenchStore } = await import("/src/app/store.ts");
          return useWorkbenchStore.getState().sidebarView;
        }),
      )
      .toBe("search");

    // Go to File turns the box back into Quick Open.
    await page.keyboard.press("ControlOrMeta+Shift+p");
    await box.fill(">file");
    await box.press("Enter");
    await expect(
      dialog.getByRole("combobox", { name: "Search Files by name" }),
    ).toHaveValue("");
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
  });
});

test.describe("Quick Open", () => {
  test("opens with Ctrl/Cmd+P, finds, goes to a line, and opens a File", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "editor", "editor");
    // The scenario's File, unedited: editing it here would leave a buffer
    // the browser runtime can't snapshot, which holds the tab open.
    await expect(page.locator(".cm-content")).toBeVisible();
    const lines = await page.locator(".cm-line").allTextContents();
    const lineIndex = lines.findIndex(
      (text, index) =>
        index > 0 && text.trim() !== "" && lines.indexOf(text) === index,
    );
    expect(lineIndex).toBeGreaterThan(0);
    const current = (await activeFile(page))!;
    expect(current).not.toBeNull();
    await installNativeStandIn(page, [
      current,
      "src/apple/core.ts",
      "docs/api.md",
    ]);

    const dialog = page.getByRole("dialog", { name: "Quick Open" });
    const input = dialog.getByRole("combobox", {
      name: "Search Files by name",
    });
    const options = dialog.getByRole("option");

    // A cold index explains itself rather than showing an empty list.
    // The scenario's clock is fake; it is stepped past the timers here.
    await page.keyboard.press("ControlOrMeta+p");
    await expect(input).toBeFocused();
    await page.clock.runFor(200);
    await expect(dialog.getByRole("status")).toHaveText("Indexing…");
    await page.clock.runFor(500);
    await expect(dialog.getByRole("status")).toBeHidden();
    // The File already open is the one recent File.
    const name = current.slice(current.lastIndexOf("/") + 1);
    await expect(options).toHaveCount(1);
    await expect(options.first()).toContainText(name);

    // :line jumps within the File.
    await input.fill(`${name}:${lineIndex + 1}`);
    await expect(options).toHaveCount(1);
    await expect(options.first().locator(".quick-open__match")).toHaveText(
      name,
    );
    await input.press("Enter");
    await expect(dialog).toBeHidden();
    await expect(page.locator(".cm-activeLine")).toHaveText(lines[lineIndex]);

    // A bare :line goes to a line of the active File.
    await page.keyboard.press("ControlOrMeta+p");
    await input.fill(":1");
    await expect(dialog.getByRole("status")).toHaveText(
      "Press Enter to go to line 1 in the active File.",
    );
    await expect(options).toHaveCount(0);
    await input.press("Enter");
    await expect(dialog).toBeHidden();
    await expect(page.locator(".cm-activeLine")).toHaveText(lines[0]);

    // Arrows move the selection; Enter opens the chosen File.
    await page.keyboard.press("ControlOrMeta+p");
    await input.fill("/ap");
    await expect(options).toHaveCount(2);
    await expect(options.nth(0)).toHaveAttribute("aria-selected", "true");
    await input.press("ArrowDown");
    await expect(options.nth(1)).toHaveAttribute("aria-selected", "true");
    const chosen = await options.nth(1).textContent();
    await input.press("Enter");
    await expect(dialog).toBeHidden();
    await expect.poll(() => activeFile(page)).not.toBe(current);
    const opened = (await activeFile(page))!;
    expect(chosen).toContain(opened.slice(opened.lastIndexOf("/") + 1));

    // An empty query lists recent Files, the latest first.
    await page.getByRole("button", { name: "Search files" }).click();
    await expect(input).toBeFocused();
    await expect(options).toHaveCount(2);
    await expect(options.nth(0)).toContainText(
      opened.slice(opened.lastIndexOf("/") + 1),
    );
    await expect(options.nth(1)).toContainText(name);

    await input.press("Escape");
    await expect(dialog).toBeHidden();

    // A click outside closes it and gives the focus back to the editor.
    await page.locator(".cm-content").click();
    await page.keyboard.press("ControlOrMeta+p");
    await expect(input).toBeFocused();
    await page.mouse.click(1300, 860);
    await expect(dialog).toBeHidden();
    await expect(page.locator(".cm-content")).toBeFocused();
  });
});
