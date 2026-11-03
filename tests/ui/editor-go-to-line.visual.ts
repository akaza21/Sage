import { expect, test } from "@playwright/test";

import { openScenario } from "./shared/visualBrowserPrimitives";

test("Go to Line buffers immediate typing during a delayed import and preserves selection", async ({
  page,
}) => {
  let releaseImport!: () => void;
  const importGate = new Promise<void>((resolve) => {
    releaseImport = resolve;
  });
  await page.route("**/FileGoToLineBar.tsx*", async (route) => {
    await importGate;
    await route.continue();
  });

  try {
    await openScenario(page, "editor", "editor");
    const editor = page.getByRole("textbox", {
      name: "Edit src/workbench/AgentChatPanes.tsx",
      exact: true,
    });
    await editor.click();
    await page.keyboard.press("ControlOrMeta+a");
    await page.keyboard.insertText("one\ntwo\nthree");
    const lines = editor.locator(".cm-line");
    await expect(lines).toHaveText(["one", "two", "three"]);

    const requested = page.waitForRequest("**/FileGoToLineBar.tsx*");
    await page.keyboard.press("Control+g");
    // No wait for the import or field before the user's next keystrokes.
    await page.keyboard.type("120");
    await requested;
    await expect(lines).toHaveText(["one", "two", "three"]);
    const field = page.getByRole("textbox", {
      name: "Go to line",
      exact: true,
    });
    await expect(field).toBeFocused();
    await expect(field).toHaveValue("120");
    await expect(field).toHaveAttribute("maxlength", "32");
    await page.keyboard.press("ArrowLeft");
    await page.keyboard.press("Shift+ArrowLeft");

    releaseImport();
    await expect(
      page.getByRole("button", { name: "Close go to line", exact: true }),
    ).toBeVisible();
    await expect(field).toBeFocused();
    await expect(field).toHaveValue("120");
    expect(
      await field.evaluate((element: HTMLInputElement) => ({
        start: element.selectionStart,
        end: element.selectionEnd,
        direction: element.selectionDirection,
      })),
    ).toEqual({ start: 1, end: 2, direction: "backward" });
    await page.keyboard.type("3");
    await expect(field).toHaveValue("130");
    await page.keyboard.press("Enter");
    await expect(field).toHaveCount(0);
    await expect(editor).toBeFocused();
    await expect(page.getByTitle("Caret position")).toHaveText("Ln 3, Col 1");
    await expect(lines).toHaveText(["one", "two", "three"]);

    // A warm open still selects the previous query for replacement.
    await page.keyboard.press("Control+g");
    await page.keyboard.type("2");
    await expect(field).toHaveValue("2");
    await page.keyboard.press("Enter");
    await expect(page.getByTitle("Caret position")).toHaveText("Ln 2, Col 1");
    await expect(lines).toHaveText(["one", "two", "three"]);
  } finally {
    releaseImport();
    await page.unrouteAll({ behavior: "wait" });
  }
});
