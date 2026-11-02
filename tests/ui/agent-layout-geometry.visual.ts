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

const SECONDARY_ACTIONS = [
  { role: "button", name: "Ask about this page" },
  { role: "combobox", name: "Chat for browser tab" },
  { role: "button", name: "Attach tab to chat" },
] as const;

test("a narrow browser toolbar keeps the address usable and every action reachable", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1100, height: 600 });
  const title = "Chat: Walking skeleton for the agent workbench";
  for (const width of [339, 420, 560]) {
    await page.goto(
      `/tests/ui/fixtures/browser-address.html?chat=${encodeURIComponent(
        title,
      )}&width=${width}`,
    );
    const toolbar = page.locator(".browser-surface__toolbar");
    const edge = await bounds(toolbar);
    expect(Math.round(edge.width)).toBe(width);

    // The address never falls below its token minimum.
    const address = page.getByRole("textbox", { name: "Address or search" });
    const minimum = await page.evaluate(() =>
      parseFloat(
        getComputedStyle(document.documentElement).getPropertyValue(
          "--browser-address-min-width",
        ),
      ),
    );
    expect(minimum).toBeGreaterThan(0);
    expect((await bounds(address)).width).toBeGreaterThanOrEqual(
      minimum - SUBPIXEL,
    );

    // Whatever the toolbar shows is whole, inside it and apart.
    const shown: Locator[] = [address];
    for (const control of await toolbar
      .locator("button, select")
      .filter({ visible: true })
      .all())
      shown.push(control);
    const boxes = [];
    for (const control of shown) {
      const box = await bounds(control);
      expectInside(box, edge);
      boxes.push(box);
    }
    for (const [i, a] of boxes.entries())
      for (const b of boxes.slice(i + 1))
        expect(
          a.x + a.width <= b.x + SUBPIXEL || b.x + b.width <= a.x + SUBPIXEL,
        ).toBe(true);

    // Each secondary action is on the toolbar, or in the More menu.
    const more = toolbar.getByRole("button", { name: "More options" });
    await expectHitTestable(page, more);
    const inToolbar = await toolbar
      .getByRole("button", { name: "Downloads" })
      .isVisible();
    await more.click();
    const menu = page.getByRole("menu", { name: "Browser menu" });
    await expect(menu).toBeVisible();
    for (const action of SECONDARY_ACTIONS) {
      const control = page.getByRole(action.role, { name: action.name });
      await expectHitTestable(page, control);
      const moved = await control.evaluate(
        (node) => node.closest('[role="menu"]') !== null,
      );
      // Page actions move only after Downloads has, and only when needed.
      expect(moved).toBe(width === 339);
      if (moved && action.role === "combobox")
        expect((await bounds(control)).width).toBeGreaterThanOrEqual(120);
    }

    // Attach behaves the same wherever it is: the fixture has no native
    // browser, so the attempt reports its failure.
    await page.getByRole("button", { name: "Attach tab to chat" }).click();
    await expect(page.getByRole("alert")).toHaveText(
      "Could not attach this tab to the selected chat.",
    );

    // Downloads opens the same popover from the menu as from the toolbar.
    if (!(await menu.isVisible())) await more.click();
    await menu.getByRole("menuitem", { name: "Downloads" }).click();
    await expect(page.getByRole("dialog", { name: "Downloads" })).toBeVisible();
    expect(inToolbar).toBe(width === 560);
  }

  // Widening the pane brings the moved actions back to the toolbar.
  await page.keyboard.press("Escape");
  await page.locator(".browser-surface").evaluate((node) => {
    (node as HTMLElement).style.width = "339px";
  });
  const toolbar = page.locator(".browser-surface__toolbar");
  await expect(toolbar.getByRole("button", { name: "Downloads" })).toBeHidden();
  await expect(page.getByRole("combobox")).toBeHidden();
  await page.locator(".browser-surface").evaluate((node) => {
    (node as HTMLElement).style.width = "560px";
  });
  await expect(
    toolbar.getByRole("button", { name: "Downloads" }),
  ).toBeVisible();
  await expectHitTestable(
    page,
    toolbar.getByRole("combobox", { name: "Chat for browser tab" }),
  );
});

async function expectHitTestable(page: Page, control: Locator) {
  const box = await bounds(control);
  const hit = await control.evaluate(
    (node, [x, y]) => {
      const top = document.elementFromPoint(x, y);
      return top !== null && (top === node || node.contains(top));
    },
    [box.x + box.width / 2, box.y + box.height / 2],
  );
  expect(hit).toBe(true);
}

test("the chat picker in the More menu changes chat by keyboard", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1100, height: 600 });
  await page.goto(
    "/tests/ui/fixtures/browser-address.html?chat=Chat%3A%201&also=Chat%3A%202&width=339",
  );
  await page.getByRole("button", { name: "More options" }).click();
  const menu = page.getByRole("menu", { name: "Browser menu" });
  const select = menu.getByRole("combobox", { name: "Chat for browser tab" });
  await select.focus();
  await page.keyboard.press("ArrowDown");
  await expect(select).toBeFocused();
  await expect(select.locator("option:checked")).toHaveText("Chat: 2");
  await page.keyboard.press("Home");
  await expect(select).toBeFocused();
  await expect(select.locator("option:checked")).toHaveText("Chat: 1");
  // Arrow keys elsewhere in the menu still move between its items.
  await menu.getByRole("menuitem", { name: "Find in page" }).focus();
  await page.keyboard.press("ArrowDown");
  await expect(menu.getByRole("menuitem", { name: "Print" })).toBeFocused();
});

interface NativeBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

/**
 * Opens a loaded page in the Agent browser over a native bridge that records
 * where Sage places the native page window. Attach fails, or with `attach`
 * succeeds and hands the tab to the chat's Agent, as Rust does.
 */
async function openNativeBrowser(
  page: Page,
  attach: "fail" | "succeed" = "fail",
) {
  await openScenario(page, "agent-single", "agent");
  await page.evaluate((attach) => {
    const placed: NativeBounds[] = [];
    Object.assign(window, { __placed: placed });
    const url = "https://example.com/";
    let attached: Record<string, unknown> | null = null;
    const replies: Record<string, (args: Record<string, unknown>) => unknown> =
      {
        browser_open: () => "browser-1",
        browser_status: () => ({
          tabId: "browser-1",
          url,
          title: "Example",
          revision: "1",
          loading: false,
          canGoBack: false,
          canGoForward: false,
          zoomPercent: 100,
          error: null,
        }),
        browser_set_bounds: ({ x, y, width, height }) => {
          placed.push({ x, y, width, height } as NativeBounds);
        },
        browser_downloads: () => [],
        browser_live_tabs: () => [],
        browser_permission_status: () => ({
          tabId: "browser-1",
          origin: "https://example.com",
          pending: [],
          decisions: [],
        }),
        attach_browser_tab: ({ projectId, chatId }) => {
          if (attach === "fail") throw new Error("visual fixture");
          attached = { projectId, chatId, tabId: "browser-1", url };
          return attached;
        },
        browser_attachment_status: ({ chatId }) =>
          attached?.chatId === chatId ? attached : null,
        browser_control: ({ action }) =>
          action === "agentControl" && attached
            ? {
                projectId: attached.projectId,
                chatId: attached.chatId,
                userControl: false,
              }
            : null,
        "plugin:event|listen": () => 1,
      };
    Object.assign(window, {
      __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} },
    });
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {
        invoke: async (command: string, args: Record<string, unknown> = {}) =>
          replies[command]?.(args) ?? null,
        transformCallback: () => 0,
        unregisterCallback: () => {},
      },
    });
  }, attach);
  // Re-render the workbench so it sees the desktop runtime.
  const modeSwitch = (name: string) =>
    page.getByRole("radio", { name, exact: true });
  await modeSwitch("Editor").check({ force: true });
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    await useWorkbenchStore.getState().openBrowser("https://example.com/");
  });
  await modeSwitch("Agent").check({ force: true });
  const surface = page.getByRole("region", { name: "Browser page" });
  await expect(
    surface.getByRole("textbox", { name: "Address or search" }),
  ).toHaveValue("example.com");
  return surface;
}

const lastPlacement = (page: Page) =>
  page.evaluate(() =>
    (window as unknown as { __placed: NativeBounds[] }).__placed.at(-1),
  );

test("an attach error shows above the native page, never behind it", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const surface = await openNativeBrowser(page);
  await expect.poll(() => lastPlacement(page)).toBeTruthy();

  await surface.getByRole("button", { name: "Attach tab to chat" }).click();
  const error = surface.getByRole("alert");
  await expect(error).toHaveText(
    "Could not attach this tab to the selected chat.",
  );
  // It reads as an error, in the danger text colour.
  expect(await error.evaluate((node) => getComputedStyle(node).color)).toBe(
    await page.evaluate(() => {
      const probe = document.createElement("span");
      probe.style.color = "var(--color-danger)";
      document.body.append(probe);
      const color = getComputedStyle(probe).color;
      probe.remove();
      return color;
    }),
  );
  const shown = await bounds(error);
  // The native page is a window above Sage's DOM: whatever it covers is
  // hidden, so it must be placed wholly below the error.
  await expect
    .poll(async () => (await lastPlacement(page))!.y)
    .toBeGreaterThanOrEqual(shown.y + shown.height - SUBPIXEL);
  const toolbar = await bounds(surface.locator(".browser-surface__toolbar"));
  expect(shown.y).toBeGreaterThanOrEqual(toolbar.y + toolbar.height - SUBPIXEL);
});

test("an attached Agent's controls stack whole inside the More menu", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1100, height: 600 });
  // Toolbars too narrow to keep the page actions, even compacted.
  for (const width of [300, 339]) {
    await page.goto(
      `/tests/ui/fixtures/browser-address.html?chat=${encodeURIComponent(
        "Chat: Walking skeleton for the agent workbench",
      )}&agent&width=${width}`,
    );
    await page.getByRole("button", { name: "More options" }).click();
    const menu = page.getByRole("menu", { name: "Browser menu" });
    const group = menu.getByRole("group", { name: "Page actions" });
    const edge = await bounds(menu);
    const controls = [
      group.getByRole("button", { name: "Ask about this page" }),
      group.getByRole("combobox", { name: "Chat for browser tab" }),
      group.getByRole("button", { name: "Attach tab to chat" }),
      group.getByRole("status"),
      group.getByRole("button", { name: "Take over" }),
      group.getByRole("button", { name: "Stop agent" }),
    ];
    const boxes = [];
    for (const control of controls) {
      const box = await bounds(control);
      expectInside(box, edge);
      boxes.push(box);
    }
    for (const control of controls.filter((_, index) => index !== 3))
      await expectHitTestable(page, control);
    for (const [i, a] of boxes.entries())
      for (const b of boxes.slice(i + 1))
        expect(
          a.x + a.width <= b.x + SUBPIXEL ||
            b.x + b.width <= a.x + SUBPIXEL ||
            a.y + a.height <= b.y + SUBPIXEL ||
            b.y + b.height <= a.y + SUBPIXEL,
        ).toBe(true);
    // The two groups stack: the Agent's presence sits below the attachment.
    expect(boxes[3].y).toBeGreaterThanOrEqual(
      boxes[1].y + boxes[1].height - SUBPIXEL,
    );
    // Neither row is clipped by the menu.
    for (const row of [
      group.locator(".browser-surface__attachment"),
      group.locator(".browser-surface__agent-presence"),
    ])
      expect(
        await row.evaluate((node) => node.scrollWidth <= node.clientWidth),
      ).toBe(true);
    await expect(group.getByRole("status")).toHaveAttribute(
      "title",
      /Walking skeleton for the agent workbench/,
    );
  }
});

test("attaching the Agent moves only the toolbar actions that no longer fit", async ({
  page,
}) => {
  // A preview pane whose toolbar fits every action before the attach.
  await page.setViewportSize({ width: 1410, height: 900 });
  const surface = await openNativeBrowser(page, "succeed");
  const toolbar = surface.locator(".browser-surface__toolbar");
  const edge = await bounds(toolbar);
  expect(edge.width).toBeGreaterThan(600);
  expect(edge.width).toBeLessThan(640);
  // Before the attach nothing has moved into More.
  await expect(toolbar).not.toHaveAttribute("data-overflow-downloads", /.*/);

  await surface.getByRole("button", { name: "Attach tab to chat" }).click();
  await expect(surface.getByRole("status")).toContainText("Agent attached");
  // Detach, Take over and Stop fit, so they stay on the bar, whole and
  // reachable, and the address keeps its minimum beside them.
  const shown = [
    surface.getByRole("button", { name: "Detach tab from chat" }),
    surface.getByRole("button", { name: "Take over" }),
    surface.getByRole("button", { name: "Stop agent" }),
    surface.getByRole("button", { name: "More options" }),
    surface.getByRole("textbox", { name: "Address or search" }),
  ];
  const boxes = [];
  for (const control of shown) {
    const box = await bounds(control);
    expectInside(box, edge);
    await expectHitTestable(page, control);
    boxes.push(box);
  }
  for (const [i, a] of boxes.entries())
    for (const b of boxes.slice(i + 1))
      expect(
        a.x + a.width <= b.x + SUBPIXEL || b.x + b.width <= a.x + SUBPIXEL,
      ).toBe(true);
  expect(boxes[4].width).toBeGreaterThanOrEqual(
    await page.evaluate(() =>
      parseFloat(
        getComputedStyle(document.documentElement).getPropertyValue(
          "--browser-address-min-width",
        ),
      ),
    ),
  );
  // The page actions stayed on the bar; More holds no copy of them.
  await surface.getByRole("button", { name: "More options" }).click();
  await expect(
    surface.getByRole("group", { name: "Page actions" }),
  ).toHaveCount(0);
});

test("download progress in the open Downloads popover does not refit the toolbar", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1100, height: 600 });
  // A toolbar that has taken some compaction steps but keeps Downloads.
  await page.goto(
    `/tests/ui/fixtures/browser-address.html?chat=${encodeURIComponent(
      "Chat: Walking skeleton for the agent workbench",
    )}&width=480`,
  );
  const toolbar = page.locator(".browser-surface__toolbar");
  await expect(toolbar).toHaveAttribute("data-compact-chat", "");
  await toolbar.getByRole("button", { name: "Downloads" }).click();
  const popover = page.getByRole("dialog", { name: "Downloads" });
  await expect(popover).toBeVisible();
  const before = await toolbar.evaluate((node) =>
    [...node.querySelectorAll("button, select, input")].map((control) =>
      JSON.stringify(control.getBoundingClientRect()),
    ),
  );
  // Record every compaction attribute the toolbar writes while the popover's
  // content ticks, as a transfer's progress does.
  const writes = await page.evaluate(async () => {
    const toolbar = document.querySelector(".browser-surface__toolbar")!;
    const popover = document.querySelector(".browser-downloads__popover")!;
    let count = 0;
    const watch = new MutationObserver((records) => (count += records.length));
    watch.observe(toolbar, { attributes: true });
    const status = document.createElement("p");
    popover.append(status);
    for (let percent = 0; percent <= 100; percent += 5) {
      status.textContent = `${percent}%`;
      await new Promise((resolve) => requestAnimationFrame(resolve));
    }
    status.remove();
    await new Promise((resolve) => requestAnimationFrame(resolve));
    watch.disconnect();
    return count;
  });
  expect(writes).toBe(0);
  await expect(toolbar).toHaveAttribute("data-compact-chat", "");
  expect(
    await toolbar.evaluate((node) =>
      [...node.querySelectorAll("button, select, input")].map((control) =>
        JSON.stringify(control.getBoundingClientRect()),
      ),
    ),
  ).toEqual(before);
});
