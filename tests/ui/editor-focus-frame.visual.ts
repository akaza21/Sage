import { expect, test, type Page } from "@playwright/test";

import { openScenario } from "./shared/visualBrowserPrimitives";

// ED-9: the editor's focus frame is an overlay drawn above CodeMirror's
// sticky gutter. Measured in Chromium on an editable File: the frame shows,
// spans the gutter, and lets the pointer reach the gutter and the text
// beneath it. A read-only File (the Agent artifact pane) takes keyboard
// focus and draws the same frame, but refuses every edit.

const EDITOR_NAME = "Edit src/workbench/AgentChatPanes.tsx";

/** The index of the line that holds the document selection's head. */
function caretLine(page: Page) {
  return page.evaluate(() => {
    const node = window.getSelection()?.focusNode ?? null;
    const element = node instanceof Element ? node : node?.parentElement;
    const line = element?.closest(".file-surface .cm-line");
    return line
      ? [...document.querySelectorAll(".file-surface .cm-line")].indexOf(line)
      : null;
  });
}

test.describe("Editor focus frame", () => {
  test.use({ viewport: { width: 1380, height: 880 } });

  test("draws the focus frame above the gutter without taking clicks", async ({
    page,
  }) => {
    await openScenario(page, "editor", "editor");
    const content = page.getByRole("textbox", { name: EDITOR_NAME });
    const frame = page.locator(".file-surface .cm-editor");
    await expect(frame).not.toHaveClass(/cm-focused/);

    await content.focus();
    await expect(frame).toHaveClass(/cm-focused/);

    const drawn = await frame.evaluate((node) => {
      const overlay = getComputedStyle(node, "::after");
      const gutters = node.querySelector<HTMLElement>(".cm-gutters")!;
      const editorBox = node.getBoundingClientRect();
      const gutterBox = gutters.getBoundingClientRect();
      return {
        content: overlay.content,
        shadow: overlay.boxShadow,
        position: overlay.position,
        left: overlay.left,
        top: overlay.top,
        width: parseFloat(overlay.width),
        height: parseFloat(overlay.height),
        editorWidth: node.clientWidth,
        editorHeight: node.clientHeight,
        overlayLayer: Number(overlay.zIndex),
        gutterLayer: Number(getComputedStyle(gutters).zIndex),
        gutterInside:
          gutterBox.left >= editorBox.left &&
          gutterBox.right <= editorBox.right,
      };
    });
    // The frame is drawn: it has content and a visible shadow.
    expect(drawn.content).not.toBe("none");
    expect(drawn.shadow).not.toBe("none");
    // It covers the whole editor, gutter included, one layer above it.
    expect(drawn).toMatchObject({
      position: "absolute",
      left: "0px",
      top: "0px",
      gutterInside: true,
    });
    expect(drawn.width).toBeCloseTo(drawn.editorWidth, 0);
    expect(drawn.height).toBeCloseTo(drawn.editorHeight, 0);
    expect(drawn.overlayLayer).toBeGreaterThan(drawn.gutterLayer);

    // Under the frame, the gutter and the text still take the pointer.
    const hits = await page.evaluate(() => {
      const at = (element: Element) => {
        const rect = element.getBoundingClientRect();
        return document.elementFromPoint(
          rect.left + rect.width / 2,
          rect.top + rect.height / 2,
        );
      };
      const number = document.querySelectorAll(
        ".file-surface .cm-lineNumbers .cm-gutterElement",
      )[2];
      const text = document.querySelector(
        ".file-surface .cm-line span[class]",
      )!;
      return {
        gutter: Boolean(at(number)?.closest(".cm-gutters")),
        text: Boolean(at(text)?.closest(".cm-content")),
      };
    });
    expect(hits).toEqual({ gutter: true, text: true });

    // A click just right of the gutter, inside the frame, puts the caret
    // on the line clicked and keeps the editor focused.
    for (const index of [2, 0]) {
      const line = (await page
        .locator(".file-surface .cm-line")
        .nth(index)
        .boundingBox())!;
      await page.mouse.click(line.x + 2, line.y + line.height / 2);
      await expect.poll(() => caretLine(page)).toBe(index);
      await expect(frame).toHaveClass(/cm-focused/);
    }
  });

  test("lets the keyboard into a read-only File without letting it edit", async ({
    page,
  }) => {
    await openScenario(page, "file", "agent");
    const pane = page.locator(".agent-artifact-pane");
    const content = pane.locator(".cm-content");
    const frame = pane.locator(".cm-editor");
    await expect(content).toHaveAttribute("aria-readonly", "true");
    const text = await content.evaluate((node) => node.textContent);

    // Tab from the artifact tab reaches the text.
    const tab = pane.getByRole("tab", { selected: true });
    await tab.focus();
    await expect(tab).toBeFocused();
    for (let step = 0; step < 12; step += 1) {
      if (await content.evaluate((node) => node === document.activeElement))
        break;
      await page.keyboard.press("Tab");
    }
    await expect(content).toBeFocused();
    await expect(frame).toHaveClass(/cm-focused/);
    expect(
      await frame.evaluate(
        (node) => getComputedStyle(node, "::after").boxShadow,
      ),
    ).not.toBe("none");

    // Arrow keys move the caret; Shift+ArrowDown selects and copy takes the text.
    const caretLine = () =>
      page.evaluate(() => {
        const node = window.getSelection()?.focusNode ?? null;
        const element = node instanceof Element ? node : node?.parentElement;
        const line = element?.closest(".agent-artifact-pane .cm-line");
        return line
          ? [
              ...document.querySelectorAll(".agent-artifact-pane .cm-line"),
            ].indexOf(line)
          : null;
      });
    await page.keyboard.press("Control+Home");
    await expect.poll(caretLine).toBe(0);
    await page.keyboard.press("ArrowDown");
    await expect.poll(caretLine).toBe(1);
    await page.keyboard.press("Shift+ArrowDown");
    const selected = await page.evaluate(() =>
      window.getSelection()!.toString(),
    );
    expect(selected.length).toBeGreaterThan(0);
    await page.evaluate(() => {
      document.addEventListener("copy", (event) => {
        (window as unknown as { copied: string }).copied =
          event.clipboardData!.getData("text/plain");
      });
    });
    await page.keyboard.press("Control+c");
    await expect
      .poll(() =>
        page.evaluate(() => (window as unknown as { copied?: string }).copied),
      )
      .toBe(selected);

    // Typing, deleting, cut, paste and undo leave the document alone.
    await page.keyboard.type("zz");
    for (const key of [
      "Backspace",
      "Enter",
      "Control+x",
      "Control+v",
      "Control+z",
    ])
      await page.keyboard.press(key);
    await expect(content).toBeFocused();
    expect(await content.evaluate((node) => node.textContent)).toBe(text);
  });
});
