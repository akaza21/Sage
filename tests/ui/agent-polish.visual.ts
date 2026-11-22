import { expect, test } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
  expectNoViewportOverflow,
} from "./shared/visualBrowserPrimitives";

test("reserves the collapsed rail column while its controls load", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1380, height: 880 });
  await openScenario(page, "agent-single", "agent");
  let release = () => {};
  const ready = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route(
    "**/src/workbench/AgentCollapsedNavigation.tsx*",
    async (route) => {
      await ready;
      await route.continue();
    },
  );
  try {
    await page.setViewportSize({ width: 480, height: 880 });
    await expect(page.locator(".workbench")).toHaveClass(
      /workbench--navigation-collapsed/,
    );
    const center = (await page.locator(".center-region").boundingBox())!;
    expect(center.x).toBe(46);
    expect(center.width).toBe(434);
  } finally {
    release();
  }
  await expect(
    page.getByRole("navigation", { name: "Primary navigation" }),
  ).toBeVisible();
  const loadedCenter = (await page.locator(".center-region").boundingBox())!;
  expect(loadedCenter.x).toBe(46);
  expect(loadedCenter.width).toBe(434);
});

test("grows and shrinks the composer within its scroll bound", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1380, height: 880 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  const input = page.getByRole("combobox", { name: "Message agent" }).first();
  await expect(input).toBeEnabled();
  await input.fill("One line");
  const short = (await input.boundingBox())!.height;
  await input.fill(Array.from({ length: 8 }, (_, i) => `Line ${i}`).join("\n"));
  const grown = (await input.boundingBox())!.height;
  expect(grown).toBeGreaterThan(short + 50);
  expect(grown).toBeLessThanOrEqual(180);
  await input.fill("Line\n".repeat(40));
  expect((await input.boundingBox())!.height).toBe(grown);
  expect(
    await input.evaluate((node) => node.scrollHeight > node.clientHeight),
  ).toBe(true);
  await input.fill("");
  expect((await input.boundingBox())!.height).toBe(short);
  await expectNoViewportOverflow(page);
});

test("keeps errors beside Send inside the joined composer with harness controls on its surface", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1380, height: 880 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await page.evaluate(async () => {
    const { installChatChromeFixture } =
      await import("/tests/ui/fixtures/chatChromeFixture.ts");
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    installChatChromeFixture("codex", true);
    useWorkbenchStore.setState({
      error: "native project transition did not commit",
    });
  });
  const form = page.locator("form.composer").first();
  const frame = page.locator(".chat-dock-frame").first();
  const dock = page.locator(".chat-dock").first();
  const error = frame.getByRole("alert");
  await expect(error).toHaveText(
    "Error: native project transition did not commit",
  );
  await expect(dock.getByRole("alert")).toHaveCount(1);
  const strip = frame.getByRole("group", { name: "Chat location" });
  // The location strip loads on demand; measure the notice after it mounts.
  await expect(strip).toBeVisible();
  const errorBox = (await error.boundingBox())!;
  const stripBox = (await strip.boundingBox())!;
  const dockBox = (await dock.boundingBox())!;
  expect(errorBox.y).toBeGreaterThanOrEqual(dockBox.y);
  const toolbarBox = (await form.locator(".composer__toolbar").boundingBox())!;
  expect(errorBox.y + errorBox.height).toBeLessThanOrEqual(toolbarBox.y);
  expect(
    Math.abs(stripBox.y + stripBox.height - dockBox.y),
  ).toBeLessThanOrEqual(0.5);
  const body = form.locator(".composer__body");
  await expect(body).toHaveCSS("overflow-y", "auto");
  await expect(body).toHaveCSS("border-top-left-radius", "0px");
  await expect(dock).toHaveCSS("border-top-left-radius", "16px");
  await expect(dock).toHaveCSS("overflow", "clip");
  const harness = form.getByRole("button", { name: "Harness", exact: true });
  await expect(harness).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
  expect((await harness.boundingBox())!.height).toBe(28);
  await expect(frame).toHaveScreenshot("agent-polish-composer-notice.png");
  await harness.click();
  await expect(page.getByRole("listbox")).toBeVisible();
  await expect(dock).toHaveCSS("overflow", "clip");
  expect(
    await page
      .getByRole("listbox")
      .evaluate((element) => element.closest("form")),
  ).toBeNull();
});

test("shows a four-line local notice in full in a tall pane", async ({
  page,
}) => {
  await page.setViewportSize({ width: 480, height: 880 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  const message =
    "The harness could not complete the request. Sign in again to continue using this chat, then retry your message. Your draft is still available, and you can keep editing it while you reconnect.";
  await page.evaluate(async (message) => {
    const { installChatChromeFixture } =
      await import("/tests/ui/fixtures/chatChromeFixture.ts");
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    installChatChromeFixture("codex", true);
    useWorkbenchStore.setState({ error: message });
  }, message);
  const notices = page.getByRole("region", { name: "Composer notices" });
  await expect(notices.getByRole("alert")).toHaveText(`Error: ${message}`);
  const geometry = await notices.evaluate((node) => {
    const text = document.createRange();
    text.selectNodeContents(
      node.querySelector(".notice__body > span:not(.sr-only)")!,
    );
    return {
      lines: text.getClientRects().length,
      scrollHeight: node.scrollHeight,
      clientHeight: node.clientHeight,
    };
  });
  expect(geometry.lines).toBe(4);
  expect(geometry.scrollHeight).toBeLessThanOrEqual(geometry.clientHeight);
});

test("long errors scroll without hiding the input or toolbar in a narrow short pane", async ({
  page,
}) => {
  await page.setViewportSize({ width: 480, height: 420 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  const message = "The harness could not complete the request. "
    .repeat(48)
    .slice(0, 2048);
  await page.evaluate(async (message) => {
    const { installChatChromeFixture } =
      await import("/tests/ui/fixtures/chatChromeFixture.ts");
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    installChatChromeFixture("codex", true);
    useWorkbenchStore.setState((state) => ({
      chatUiById: {
        ...state.chatUiById,
        [state.activeTabId!]: {
          ...state.chatUiById[state.activeTabId!],
          error: message,
        },
      },
    }));
  }, message);
  const dock = page.locator(".chat-dock");
  const frame = page.locator(".chat-dock-frame");
  await expect(frame.getByRole("alert")).toHaveText(`Error: ${message}`);
  const input = dock.getByRole("combobox", { name: "Message agent" });
  const toolbar = dock.locator(".composer__toolbar");
  await input.fill("Try again");
  const bounds = (await dock.boundingBox())!;
  for (const control of [input, toolbar]) {
    const box = (await control.boundingBox())!;
    expect(box.y).toBeGreaterThanOrEqual(bounds.y);
    expect(box.y + box.height).toBeLessThanOrEqual(bounds.y + bounds.height);
    expect(box.x).toBeGreaterThanOrEqual(bounds.x);
    expect(box.x + box.width).toBeLessThanOrEqual(bounds.x + bounds.width);
  }
  await expect(input).toHaveValue("Try again");
  await dock
    .getByRole("button", { name: "Send message", exact: true })
    .click({ trial: true });
  const notices = frame.getByRole("region", { name: "Composer notices" });
  expect(
    await notices.evaluate((node) => node.scrollHeight > node.clientHeight),
  ).toBe(true);
  await notices.focus();
  await expect(notices).toBeFocused();
  await page.keyboard.press("End");
  await expect
    .poll(() => notices.evaluate((node) => node.scrollTop))
    .toBeGreaterThan(0);
  const frameBox = (await frame.boundingBox())!;
  const surfaceBox = (await page.locator(".chat-surface").boundingBox())!;
  expect(frameBox.height).toBeLessThanOrEqual(surfaceBox.height * 0.6 + 1);
  expect(frameBox.y + frameBox.height).toBeLessThanOrEqual(
    surfaceBox.y + surfaceBox.height,
  );
  await expectNoViewportOverflow(page);
});

test("keeps title-bar height stable and omits an empty Editor tab row", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1380, height: 880 });
  await openScenario(page, "agent-single", "agent");
  // The welcome canvas can load before the independently loaded tab strip.
  let release = () => {};
  const ready = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/src/workbench/TabStrip.tsx*", async (route) => {
    await ready;
    await route.continue();
  });
  const tabs = page.locator(".workbench__tabs");
  const tablist = tabs.getByRole("tablist", {
    name: "Tabs",
    includeHidden: true,
  });
  try {
    const title = page.locator(".title-bar");
    const agent = (await title.boundingBox())!;
    await expect(
      title.locator(".title-bar__brand, .title-bar__mark"),
    ).toHaveCount(0);
    await page.getByRole("radio", { name: "Editor", exact: true }).focus();
    await page.keyboard.press("Space");
    expect((await title.boundingBox())!.height).toBe(agent.height);
    await expect(
      page.getByRole("region", { name: "Editor welcome" }),
    ).toBeVisible();
    await expect(tablist).toHaveCount(0);
    expect((await tabs.boundingBox())!.height).toBe(0);
    await expect(page.locator(".editor-welcome__mark use")).toHaveAttribute(
      "href",
      "/sage-mark.svg#mark",
    );
  } finally {
    release();
  }
  await expect(tablist).toBeAttached();
  await expect(tablist.getByRole("tab", { includeHidden: true })).toHaveCount(
    0,
  );
  expect((await tabs.boundingBox())!.height).toBe(0);
});

test("collapses the Agent sidebar at 480 and supports the sidebar shortcut", async ({
  page,
}) => {
  await page.setViewportSize({ width: 480, height: 880 });
  await openScenario(page, "agent-single", "agent");
  await expect(
    page.getByRole("button", { name: "Show projects and chats" }),
  ).toBeVisible();
  await expect(page.locator(".project-sidebar__scroll")).toHaveCount(0);
  const harnessStatus = page.locator(".agent-task-header__harness-status");
  const info = harnessStatus.locator("summary");
  await expect(info).toHaveAccessibleName("Harness status: Test simulator");
  await expect(info.locator("svg.lucide-info")).toBeVisible();
  await info.click();
  await expect(harnessStatus.locator("p")).toHaveText("Test simulator");
  await info.click();
  expect(
    (await page.locator(".agent-chat-pane").first().boundingBox())!.width,
  ).toBeGreaterThan(400);
  await page.keyboard.press("Control+b");
  await expect(
    page.getByRole("button", { name: "Hide projects and chats" }),
  ).toBeVisible();
  await page.keyboard.press("Control+b");
  await expect(
    page.getByRole("button", { name: "Show projects and chats" }),
  ).toBeVisible();
  await expectNoViewportOverflow(page);
});

test("keeps browser creation unavailable without native support", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1380, height: 880 });
  await openScenario(page, "agent-single", "agent");
  await page.getByRole("button", { name: /Show files and preview/ }).click();
  await expect(
    page.getByRole("button", { name: "New browser tab", exact: true }),
  ).toBeDisabled();
});

test("clips the resource tab close hover fill at its rounded corner", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1380, height: 880 });
  await openScenario(page, "file", "agent");
  const tab = page.locator(".agent-artifact-pane .tab--active").first();
  const close = tab.locator(".tab__close");
  await close.hover();
  expect(
    await tab.evaluate((node) => {
      const box = node.getBoundingClientRect();
      return node
        .querySelector(".tab__close")!
        .contains(document.elementFromPoint(box.right - 1.5, box.top + 1.5));
    }),
  ).toBe(false);
  await page.keyboard.press("Tab");
  await close.focus();
  await expect(close).toBeFocused();
  const focus = await close.evaluate((node) => {
    const style = getComputedStyle(node);
    return {
      visible: node.matches(":focus-visible"),
      width: parseFloat(style.outlineWidth),
      offset: parseFloat(style.outlineOffset),
    };
  });
  expect(focus.visible).toBe(true);
  expect(focus.width).toBeGreaterThan(0);
  expect(focus.offset).toBeLessThanOrEqual(-focus.width);
});
