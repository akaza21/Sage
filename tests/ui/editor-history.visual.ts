import { expect, test, type Page } from "@playwright/test";

import { openScenario } from "./shared/visualBrowserPrimitives";

// ED-2b: Go back and Go forward, through real keys and the command palette
// in Chromium against the browser runtime. Its Files are short projections,
// so the place is told by the caret's column.
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

// As a click on its row in Explorer opens it.
async function openFile(page: Page, relativePath: string) {
  await page.evaluate(async (path) => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const state = useWorkbenchStore.getState();
    await state.openFile({
      rootId: state.project!.roots[0].id,
      relativePath: path,
    });
  }, relativePath);
  await expect.poll(() => activeFile(page)).toBe(relativePath);
  await expect(page.locator(".cm-content")).toBeVisible();
}

const apple = process.platform === "darwin";

test.describe("Editor history", () => {
  test("goes back to where the caret was in the last File, and forward again", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "editor", "editor");
    await expect(page.locator(".cm-content")).toBeVisible();
    const scenarioFile = (await activeFile(page))!;

    await openFile(page, "src/apple/core.ts");
    await page.locator(".cm-content").click();
    await page.keyboard.press("Control+g");
    const goTo = page.getByRole("textbox", { name: "Go to line" });
    await goTo.fill("1:10");
    await goTo.press("Enter");
    await expect(page.getByText("Ln 1, Col 10")).toBeVisible();

    await openFile(page, "docs/api.md");
    await expect(page.getByText("Ln 1, Col 1", { exact: true })).toBeVisible();

    await page.keyboard.press(apple ? "Control+-" : "Alt+ArrowLeft");
    await expect.poll(() => activeFile(page)).toBe("src/apple/core.ts");
    await expect(page.getByText("Ln 1, Col 10")).toBeVisible();
    await expect(page.locator(".cm-content")).toBeFocused();

    // Inside the editor too, the key goes back rather than moving the caret.
    await page.keyboard.press(apple ? "Control+-" : "Alt+ArrowLeft");
    await expect.poll(() => activeFile(page)).toBe(scenarioFile);

    // The palette offers Go Forward now, with its shortcut.
    const dialog = page.getByRole("dialog", { name: "Quick Open" });
    const box = dialog.getByRole("combobox", { name: "Run a command" });
    await page.keyboard.press("ControlOrMeta+Shift+p");
    await box.fill(">go forward");
    const options = dialog.getByRole("option");
    await expect(options).toHaveCount(1);
    await expect(options.first().locator("kbd")).toHaveText(
      apple ? "Ctrl+Shift+-" : "Alt+Right",
    );
    await box.press("Enter");
    await expect(dialog).toBeHidden();
    await expect.poll(() => activeFile(page)).toBe("src/apple/core.ts");
    await expect(page.getByText("Ln 1, Col 10")).toBeVisible();

    await page.keyboard.press(apple ? "Control+Shift+-" : "Alt+ArrowRight");
    await expect.poll(() => activeFile(page)).toBe("docs/api.md");
  });
});
