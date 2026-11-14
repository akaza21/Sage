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

test("clips notice fills inside the composer and keeps harness controls on its surface", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1380, height: 880 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  const form = page.locator("form.composer").first();
  const dock = page.locator(".chat-dock").first();
  await form.locator(".composer__body").evaluate((body) => {
    const notice = document.createElement("div");
    notice.className = "composer__notice";
    notice.textContent =
      "No reviewed local binding is available for this harness.";
    body.prepend(notice);
  });
  const body = form.locator(".composer__body");
  await expect(body).toHaveCSS("overflow-y", "auto");
  await expect(body).toHaveCSS("border-top-left-radius", "0px");
  await expect(dock).toHaveCSS("border-top-left-radius", "16px");
  await expect(dock).toHaveCSS("overflow", "clip");
  const harness = form.getByRole("button", { name: "Harness", exact: true });
  await expect(harness).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
  expect((await harness.boundingBox())!.height).toBe(28);
  await expect(dock).toHaveScreenshot("agent-polish-composer-notice.png");
  await harness.click();
  await expect(page.getByRole("listbox")).toBeVisible();
  await expect(dock).toHaveCSS("overflow", "clip");
  expect(
    await page
      .getByRole("listbox")
      .evaluate((element) => element.closest("form")),
  ).toBeNull();
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
