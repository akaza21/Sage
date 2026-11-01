import { expect, test, type Page } from "@playwright/test";
import { openScenario } from "./shared/visualBrowserPrimitives";

async function openTerminal(page: Page) {
  await openScenario(page, "agent-single", "agent");
  await page.evaluate(() => {
    const pty = { columns: 80, rows: 24 };
    Object.assign(window, { terminalPtySize: pty });
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {
        invoke: async (
          command: string,
          args: { request: { columns: number; rows: number } },
        ) => {
          switch (command) {
            case "terminal_create":
              return {
                sessionId: "f99cd56a-32d9-4600-a1f6-269e05734268",
                revision: "1",
                shellPath: "fixture-shell",
              };
            case "terminal_resize":
              Object.assign(pty, {
                columns: args.request.columns,
                rows: args.request.rows,
              });
              return;
            case "terminal_read":
              return new Promise(() => undefined);
            case "terminal_close":
              return;
            default:
              throw new Error(`Unexpected terminal command: ${command}`);
          }
        },
      },
    });
  });
  // Re-render the workbench after attaching the desktop fixture.
  await page
    .getByRole("radiogroup", { name: "Window mode" })
    .locator("label")
    .filter({ hasText: "Editor" })
    .click();
  await page
    .getByRole("button", { name: "Open terminal", exact: true })
    .click();
  await expect(page.locator(".xterm-screen")).toBeVisible();
}

async function gridGeometry(page: Page) {
  return page.locator(".terminal-dock__screen").evaluate((host) => {
    const grid = host.querySelector<HTMLElement>(".xterm-screen")!;
    const rows = host.querySelector<HTMLElement>(".xterm-rows")!;
    const scrollbar = host.querySelector(".scrollbar.vertical")!;
    const font = getComputedStyle(rows);
    const context = new OffscreenCanvas(1, 1).getContext("2d")!;
    context.font = `${font.fontSize} ${font.fontFamily}`;
    const character = context.measureText("W");
    const pty = (
      window as unknown as {
        terminalPtySize: { columns: number; rows: number };
      }
    ).terminalPtySize;
    return {
      ...pty,
      renderedRows: rows.children.length,
      width: grid.getBoundingClientRect().width,
      height: grid.getBoundingClientRect().height,
      availableWidth:
        host.clientWidth - scrollbar.getBoundingClientRect().width,
      availableHeight: host.clientHeight,
      cellWidth: character.width,
      cellHeight: Number.parseFloat(
        getComputedStyle(rows.firstElementChild!).height,
      ),
    };
  });
}

for (const deviceScaleFactor of [1, 1.25, 2]) {
  test.describe(`terminal grid at ${deviceScaleFactor} device scale`, () => {
    test.use({ deviceScaleFactor });

    test("fits both axes and matches the PTY after narrow and wide resizes", async ({
      page,
    }) => {
      await openTerminal(page);
      const host = page.locator(".terminal-dock__screen");
      let firstFit: Awaited<ReturnType<typeof gridGeometry>> | undefined;
      for (const [width, height] of [
        [32, 32],
        [800, 160],
        [1600, 320],
        [800, 160],
      ]) {
        await host.evaluate(
          (element, size) => {
            element.style.width = `${size.width}px`;
            element.style.height = `${size.height}px`;
          },
          { width, height },
        );
        await expect
          .poll(async () => {
            const grid = await gridGeometry(page);
            return {
              fitsWidth: grid.width <= grid.availableWidth,
              fitsHeight: grid.height <= grid.availableHeight,
              fillsWidth: grid.availableWidth - grid.width < grid.cellWidth,
              fillsHeight: grid.availableHeight - grid.height < grid.cellHeight,
              matchesColumns:
                grid.width === Math.round(grid.columns * grid.cellWidth),
              matchesRows: grid.renderedRows === grid.rows,
            };
          })
          .toEqual({
            fitsWidth: true,
            fitsHeight: true,
            fillsWidth: true,
            fillsHeight: true,
            matchesColumns: true,
            matchesRows: true,
          });
        if (width === 800) {
          const fit = await gridGeometry(page);
          if (firstFit) expect(fit).toEqual(firstFit);
          else firstFit = fit;
        }
      }
    });
  });
}
