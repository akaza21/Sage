import { expect, test, type Page } from "@playwright/test";

import { serveUnderTauriCsp } from "./shared/tauriCsp";
import { openScenario } from "./shared/visualBrowserPrimitives";

const NBSP = " ";

async function serveUnderTauriCspRecordingStyles(page: Page) {
  await serveUnderTauriCsp(page);
  await page.addInitScript(() => {
    const refused: string[] = [];
    (window as unknown as { refusedStyles: string[] }).refusedStyles = refused;
    document.addEventListener("securitypolicyviolation", (event) => {
      if (event.effectiveDirective.startsWith("style-src"))
        refused.push(`${event.effectiveDirective} ${event.sourceFile}`);
    });
  });
}

test("the editor keeps its layout and plain spaces under the packaged CSP", async ({
  page,
}) => {
  await serveUnderTauriCspRecordingStyles(page);
  await page.setViewportSize({ width: 1380, height: 880 });
  await openScenario(page, "editor", "editor");
  const editor = page.locator(".cm-editor");
  const firstLine = editor.locator(".cm-line").first();
  const firstNumber = editor
    .locator(".cm-lineNumbers .cm-gutterElement")
    .filter({ hasText: /^1$/u });
  await expect(firstLine).toBeVisible();
  await expect(firstNumber).toBeVisible();

  const lineTop = (await firstLine.boundingBox())!.y;
  const numberTop = (await firstNumber.boundingBox())!.y;
  expect(Math.abs(lineTop - numberTop)).toBeLessThanOrEqual(1);

  await editor.locator(".cm-content").click();
  await page.keyboard.press("ControlOrMeta+End");
  await page.keyboard.press("Enter");
  await page.keyboard.type("let typed = 1;");
  const typed = editor.locator(".cm-line").filter({ hasText: "typed" });
  await expect(typed).toHaveCount(1);
  const text = await typed.evaluate((line) => line.textContent ?? "");
  expect(text.includes(NBSP)).toBe(false);
  expect(text.trim()).toBe("let typed = 1;");

  expect(
    await page.evaluate(
      () => (window as unknown as { refusedStyles: string[] }).refusedStyles,
    ),
  ).toEqual([]);
});

test("the real terminal keeps its monospace cell grid under the packaged CSP", async ({
  page,
}) => {
  await serveUnderTauriCspRecordingStyles(page);
  await page.setViewportSize({ width: 1380, height: 880 });
  await openScenario(page, "agent-single", "agent");
  // Only the native PTY is simulated. TerminalDock loads the real xterm renderer.
  await page.evaluate(async () => {
    let columns = 80;
    let visible = false;
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {
        invoke: async (
          command: string,
          args?: {
            visible: boolean;
            request: { columns: number; cursor: string };
          },
        ) => {
          switch (command) {
            case "get_terminal_dock_visible":
              return visible;
            case "set_terminal_dock_visible":
              visible = args!.visible;
              return visible;
            case "terminal_create":
              return {
                sessionId: "f99cd56a-32d9-4600-a1f6-269e05734268",
                revision: "1",
                shellPath: "fixture-shell",
              };
            case "terminal_resize":
              columns = args!.request.columns;
              return;
            case "terminal_read": {
              if (args!.request.cursor !== "0")
                return new Promise(() => undefined);
              const line = "iW"
                .repeat(Math.ceil(columns / 2))
                .slice(0, columns);
              return {
                bytesBase64: btoa(`${line}\r\n${line}\r\n`),
                nextCursor: "1",
                truncations: "0",
                truncated: false,
              };
            }
            case "terminal_close":
              return;
            default:
              throw new Error(
                `Unsupported terminal fixture command: ${command}`,
              );
          }
        },
      },
    });
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    useWorkbenchStore.setState((state) => ({
      sidebarOpen: !state.sidebarOpen,
    }));
  });
  await page
    .getByRole("button", { name: "Toggle terminal", exact: true })
    .click();
  const terminal = page.getByRole("region", { name: "Terminal", exact: true });
  const rows = terminal.locator(".xterm-rows > div");
  await expect(rows.nth(1)).toContainText("iWiW");
  await page.evaluate(() => document.fonts.ready);

  const grid = await rows.first().evaluate((row) => {
    // Ranges measure actual rendered advances, including xterm's spacing.
    const text = row.firstChild!.firstChild!;
    const range = document.createRange();
    const cells = Array.from({ length: 4 }, (_, index) => {
      range.setStart(text, index);
      range.setEnd(text, index + 1);
      const rect = range.getBoundingClientRect();
      return { x: rect.x, width: rect.width };
    });
    const style = getComputedStyle(row);
    const context = document.createElement("canvas").getContext("2d")!;
    context.font = `${style.fontSize} ${style.fontFamily}`;
    const rowBounds = row.getBoundingClientRect();
    const nextRow = row.nextElementSibling!.getBoundingClientRect();
    return {
      cells,
      measuredWidth: context.measureText("W").width,
      columns: row.textContent!.length,
      rowWidth: rowBounds.width,
      rowHeight: rowBounds.height,
      rowAdvance: nextRow.y - rowBounds.y,
      screenHeight: row.closest(".xterm-screen")!.getBoundingClientRect()
        .height,
      rowCount: row.parentElement!.childElementCount,
    };
  });
  expect(grid.cells[0].width).toBeGreaterThan(0);
  for (const [index, cell] of grid.cells.entries()) {
    expect(Math.abs(cell.width - grid.measuredWidth)).toBeLessThanOrEqual(0.1);
    expect(
      Math.abs(cell.x - grid.cells[0].x - index * grid.measuredWidth),
    ).toBeLessThanOrEqual(0.1);
  }
  expect(
    Math.abs(grid.rowWidth - grid.columns * grid.measuredWidth),
  ).toBeLessThanOrEqual(1);
  expect(grid.rowHeight).toBeGreaterThan(0);
  expect(grid.rowAdvance).toBeCloseTo(grid.rowHeight, 1);
  expect(grid.screenHeight).toBeCloseTo(grid.rowCount * grid.rowHeight, 0);
  expect(
    await page.evaluate(
      () => (window as unknown as { refusedStyles: string[] }).refusedStyles,
    ),
  ).toEqual([]);
});
