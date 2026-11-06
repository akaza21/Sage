import { createHash, randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";

import { expect, test, type Page } from "./shared/test";

import { openScenario } from "./shared/visualBrowserPrimitives";

// The packaged app runs under tauri.conf.json's CSP as Tauri serves it: every
// `<style>` in index.html gets a fresh nonce that is added to style-src, which
// makes the browser ignore 'unsafe-inline', and inline scripts are allowed by
// hash. Styles a library injects at runtime then need that nonce, or they are
// refused and CodeMirror/xterm run without their layout styles.
const tauriConfig = JSON.parse(
  readFileSync(
    new URL("../../src-tauri/tauri.conf.json", import.meta.url),
    "utf8",
  ),
) as { app: { security: { csp: Record<string, string> } } };

const NBSP = " ";

async function serveUnderTauriCsp(page: Page) {
  const nonce = BigInt(`0x${randomBytes(8).toString("hex")}`).toString();
  await page.route(
    (url) => url.pathname === "/",
    async (route) => {
      const response = await route.fetch();
      const scriptHashes: string[] = [];
      const html = (await response.text())
        .replace(/<style(?![^>]*\snonce=)/gu, `<style nonce="${nonce}"`)
        .replace(
          /<script(?![^>]*\ssrc=)[^>]*>([\s\S]*?)<\/script>/gu,
          (script, body: string) => {
            const digest = createHash("sha256").update(body).digest("base64");
            scriptHashes.push(`'sha256-${digest}'`);
            return script;
          },
        )
        // Vite's dev server injects each CSS module as a <style>; this is
        // its documented way to give them the nonce. Packaged builds link CSS.
        .replace(
          "<head>",
          `<head><meta property="csp-nonce" nonce="${nonce}" />`,
        );
      const directives = { ...tauriConfig.app.security.csp };
      directives["style-src"] += ` 'nonce-${nonce}'`;
      directives["script-src"] += ` ${scriptHashes.join(" ")}`;
      // Vite's HMR socket only; unrelated to styles.
      directives["connect-src"] +=
        ` ws://${new URL(route.request().url()).host}`;
      await route.fulfill({
        response,
        body: html,
        headers: {
          ...response.headers(),
          "content-security-policy": Object.entries(directives)
            .map(([name, sources]) => `${name} ${sources}`)
            .join("; "),
        },
      });
    },
  );
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
  await serveUnderTauriCsp(page);
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
  await serveUnderTauriCsp(page);
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
    const { useWorkbenchStore } = await sageImport("/src/app/store.ts");
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
