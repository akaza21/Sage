import { expect, test, type Page } from "@playwright/test";

import { openScenario } from "./shared/visualBrowserPrimitives";

// ED-9: the editor's focus frame is an overlay drawn above CodeMirror's
// sticky gutter. Measured in Chromium on an editable File: the frame shows,
// spans the gutter, and lets the pointer reach the gutter and the text
// beneath it.

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
});
