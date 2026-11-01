import { expect, test, type Locator, type Page } from "@playwright/test";
import { openScenario } from "./shared/visualBrowserPrimitives";

// Sub-pixel layout may overshoot a container edge by under one device pixel.
const SUBPIXEL = 0.5;

async function bounds(locator: Locator) {
  await expect(locator).toBeVisible();
  const box = await locator.boundingBox();
  expect(box).not.toBeNull();
  return box!;
}

function expectInside(
  inner: { x: number; y: number; width: number; height: number },
  outer: { x: number; y: number; width: number; height: number },
) {
  expect(inner.x).toBeGreaterThanOrEqual(outer.x - SUBPIXEL);
  expect(inner.y).toBeGreaterThanOrEqual(outer.y - SUBPIXEL);
  expect(inner.x + inner.width).toBeLessThanOrEqual(
    outer.x + outer.width + SUBPIXEL,
  );
  expect(inner.y + inner.height).toBeLessThanOrEqual(
    outer.y + outer.height + SUBPIXEL,
  );
}

async function backgroundColor(locator: Locator) {
  return locator.evaluate((node) => getComputedStyle(node).backgroundColor);
}

async function expectUnclippedText(locator: Locator) {
  await expect(locator).toBeVisible();
  expect(
    await locator.evaluate(
      (node) => node.clientWidth > 0 && node.scrollWidth <= node.clientWidth,
    ),
  ).toBe(true);
}

interface TerminalBridgeState {
  created: string[];
  closed: string[];
  input: Record<string, string>;
}

function bridgeState(page: Page) {
  return page.evaluate(
    () =>
      (window as unknown as { __terminals: TerminalBridgeState }).__terminals,
  );
}

/**
 * The terminal is desktop-only. Install a native bridge whose sessions echo
 * their input back as output, like a shell at a prompt, and record every
 * create, write and close; other native calls reject. Then re-render the
 * workbench so it sees the desktop runtime.
 */
async function openTerminal(page: Page, mode: "agent" | "editor") {
  await openScenario(page, "agent-single", "agent");
  await page.evaluate(() => {
    const state: TerminalBridgeState = { created: [], closed: [], input: {} };
    const output = new Map<string, string>();
    Object.assign(window, { __terminals: state });
    type Request = {
      handle?: { sessionId: string };
      text?: string;
      cursor?: string;
    };
    const commands: Record<string, (request: Request) => Promise<unknown>> = {
      async terminal_create() {
        const sessionId = `00000000-0000-4000-8000-${String(state.created.length + 1).padStart(12, "0")}`;
        state.created.push(sessionId);
        state.input[sessionId] = "";
        output.set(sessionId, "");
        return { sessionId, revision: "1", shellPath: "/bin/bash" };
      },
      async terminal_write({ handle, text }) {
        state.input[handle!.sessionId] += text!;
        output.set(handle!.sessionId, output.get(handle!.sessionId)! + text!);
      },
      async terminal_read({ handle, cursor }) {
        const from = Number(cursor);
        // Long-poll briefly, as the native side waits for new output.
        for (let wait = 0; wait < 10; wait += 1) {
          if (output.get(handle!.sessionId)!.length > from) break;
          await new Promise((resolve) => setTimeout(resolve, 20));
        }
        const text = output.get(handle!.sessionId)!;
        return {
          bytesBase64: btoa(text.slice(from)),
          nextCursor: String(text.length),
          truncations: "0",
          truncated: false,
        };
      },
      async terminal_resize() {},
      async terminal_close({ handle }) {
        state.closed.push(handle!.sessionId);
      },
    };
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {
        invoke: (command: string, args?: { request?: Request }) =>
          command in commands
            ? commands[command](args?.request ?? {})
            : Promise.reject(new Error("visual fixture")),
        transformCallback: () => 0,
      },
    });
  });
  const modeSwitch = (name: string) =>
    page.getByRole("radio", { name, exact: true });
  await modeSwitch("Editor").check({ force: true });
  if (mode === "agent") await modeSwitch("Agent").check({ force: true });
  await expect(page.locator("main.workbench")).toHaveClass(
    new RegExp(`workbench--${mode}`),
  );
  await page.getByRole("button", { name: "Open terminal" }).click();
  const dock = page.getByRole("region", { name: "Terminal", exact: true });
  await expect(dock.getByRole("tab")).toHaveCount(1);
  return dock;
}

for (const mode of ["agent", "editor"] as const) {
  test(`${mode} terminal dock sits under the center region, beside a full-height rail`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    const dock = await openTerminal(page, mode);
    const dockBox = await bounds(dock);
    // The region's padding box: Editor draws the region as a bordered card.
    const center = await page.locator(".center-region").evaluate((node) => {
      const rect = node.getBoundingClientRect();
      return {
        x: rect.x + node.clientLeft,
        y: rect.y + node.clientTop,
        width: node.clientWidth,
        height: node.clientHeight,
      };
    });
    const statusBar = await bounds(page.locator(".status-bar"));
    // The dock spans the center region's width and ends at its bottom edge.
    expect(Math.abs(dockBox.x - center.x)).toBeLessThanOrEqual(SUBPIXEL);
    expect(
      Math.abs(dockBox.x + dockBox.width - (center.x + center.width)),
    ).toBeLessThanOrEqual(SUBPIXEL);
    expectInside(dockBox, center);
    // Nothing sits left of the dock but the rail, which runs down to the
    // status bar instead of stopping at the dock's top edge.
    const rail = await bounds(
      page.locator(mode === "agent" ? ".project-sidebar" : ".sidebar").first(),
    );
    expect(rail.x + rail.width).toBeLessThanOrEqual(dockBox.x + SUBPIXEL);
    expect(rail.y + rail.height).toBeGreaterThan(
      dockBox.y + dockBox.height / 2,
    );
    expect(rail.y + rail.height).toBeLessThanOrEqual(statusBar.y + SUBPIXEL);
  });
}

test("an open terminal keeps its session, input and output across Agent and Editor", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const dock = await openTerminal(page, "agent");
  const screen = dock.locator(".xterm-rows");
  const typeLine = async (text: string) => {
    await dock.locator(".xterm").click();
    await page.keyboard.type(text);
    await expect(screen).toContainText(text);
  };
  await typeLine("agent-1");
  const [session] = (await bridgeState(page)).created;
  for (const [mode, line] of [
    ["Editor", "editor-2"],
    ["Agent", "agent-3"],
  ] as const) {
    await page.getByRole("radio", { name: mode, exact: true }).check({
      force: true,
    });
    await expect(page.locator(".center-region .terminal-dock")).toBeVisible();
    await expect(dock.getByRole("tab")).toHaveCount(1);
    await typeLine(line);
  }
  // Earlier output is still on screen, and every keystroke reached the one
  // session, which was never closed and reopened.
  await expect(screen).toContainText("agent-1editor-2agent-3");
  const kept = await bridgeState(page);
  expect(kept.created).toEqual([session]);
  expect(kept.input[session]).toBe("agent-1editor-2agent-3");
  expect(kept.closed).toEqual([]);

  await dock.getByRole("button", { name: "Close terminal tab" }).click();
  await expect(dock).toHaveCount(0);
  await expect(page.locator(".center-region__dock")).toHaveCount(0);
  expect((await bridgeState(page)).closed).toEqual([session]);
});

test("terminal dock header controls are whole, hit-testable and unfilled at rest", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const dock = await openTerminal(page, "agent");
  const header = await bounds(dock.locator(".terminal-dock__header"));
  for (const control of [
    dock.getByRole("button", { name: "Close terminal tab" }),
    dock.getByRole("button", { name: "New terminal" }),
    dock.getByRole("button", { name: "Collapse terminal" }),
  ]) {
    const box = await bounds(control);
    expectInside(box, header);
    expect(box.width).toBeGreaterThanOrEqual(24);
    expect(box.height).toBeGreaterThanOrEqual(24);
    // The icon is drawn in full inside the button.
    expectInside(await bounds(control.locator("svg")), box);
    expect(await backgroundColor(control)).toBe("rgba(0, 0, 0, 0)");
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
  await dock.getByRole("button", { name: "Collapse terminal" }).click();
  const collapsed = await bounds(dock);
  expect(collapsed.height).toBeLessThanOrEqual(header.height + 1 + SUBPIXEL);
  await expect(
    dock.getByRole("button", { name: "Expand terminal" }),
  ).toBeVisible();
});

test("a narrow composer keeps the harness name beside its brand mark, on the composer fill", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openScenario(page, "preview", "agent");
  const composer = page.locator("form.composer").first();
  // The Preview pane leaves the composer below the 400px compaction step.
  expect((await bounds(composer)).width).toBeLessThanOrEqual(400);
  const trigger = composer.getByRole("button", { name: "Harness" });
  const logo = trigger.locator(".harness-logo");
  const label = trigger.locator(".searchable-picker__trigger-label");
  await expect(label).toHaveText("Codex");
  await expectUnclippedText(label);
  // The visual fixture is a development harness named "Codex", so it shows
  // the generic mark: brands follow the curated catalogue id, not the name.
  await expect(logo).toHaveAttribute("data-harness-icon", "generic");
  const logoBox = await bounds(logo);
  expect(logoBox.width).toBeGreaterThan(0);
  expect(logoBox.x + logoBox.width).toBeLessThanOrEqual(
    (await bounds(label)).x + SUBPIXEL,
  );
  expect(
    await logo.evaluate((node) => getComputedStyle(node).maskImage),
  ).toContain("/harness-generic.svg");
  expect(await backgroundColor(trigger)).toBe("rgba(0, 0, 0, 0)");
  expect(await backgroundColor(composer.locator(".composer__harness"))).toBe(
    "rgba(0, 0, 0, 0)",
  );
});

test("a long harness name shows whole or not at all as the composer narrows", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openScenario(page, "agent-single", "agent");
  // Rename the fixture harness to the longest curated name.
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    useWorkbenchStore.setState((state) => ({
      harnesses: state.harnesses.map((harness) => ({
        ...harness,
        name: "Claude Code",
      })),
    }));
  });
  const composer = page.locator("form.composer").first();
  const trigger = composer.getByRole("button", { name: "Harness" });
  const label = trigger.locator(".searchable-picker__trigger-label");
  await expect(label).toHaveText("Claude Code");
  const shown: boolean[] = [];
  for (const width of [480, 400, 360, 321, 300, 281]) {
    await composer.evaluate((element, content) => {
      const style = getComputedStyle(element);
      element.style.width = `${content + Number.parseFloat(style.borderLeftWidth) + Number.parseFloat(style.borderRightWidth)}px`;
    }, width);
    // Let the resize observers run, as they do before the next paint.
    await page.evaluate(
      () =>
        new Promise((resolve) =>
          requestAnimationFrame(() => requestAnimationFrame(resolve)),
        ),
    );
    await expect(trigger.locator(".harness-logo")).toBeVisible();
    const named = await label.isVisible();
    if (named) await expectUnclippedText(label);
    else expect((await bounds(trigger)).width).toBeGreaterThanOrEqual(24);
    shown.push(named);
  }
  // A comfortable composer names the harness; the name only ever goes away
  // as the composer narrows, and never comes back truncated.
  expect(shown[0]).toBe(true);
  expect(
    shown.indexOf(false) === -1 ||
      !shown.slice(shown.indexOf(false)).includes(true),
  ).toBe(true);
});

test("the browser toolbar shows the attached chat's title, truncating only long ones", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 400 });
  const select = page.getByRole("combobox", { name: "Chat for browser tab" });
  await page.goto("/tests/ui/fixtures/browser-address.html?chat=Chat%3A%202");
  await expectUnclippedText(select);
  const address = await bounds(page.locator(".browser-surface__address"));
  expect(address.width).toBeGreaterThan(0);

  await page.goto(
    `/tests/ui/fixtures/browser-address.html?chat=${encodeURIComponent(
      "Chat: Walking skeleton for the agent workbench",
    )}`,
  );
  await expect(select).toBeVisible();
  const toolbar = await bounds(page.locator(".browser-surface__toolbar"));
  const box = await bounds(select);
  expectInside(box, toolbar);
  // A long title is capped and ellipsised, never squeezed to its arrow.
  expect(box.width).toBeGreaterThanOrEqual(120);
  expect(
    await select.evaluate((node) => getComputedStyle(node).textOverflow),
  ).toBe("ellipsis");
});
