import { expect, test, type Locator, type Page } from "@playwright/test";

import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

// What the activity rail and the sidebar splitters render in each state,
// read from computed styles. Colours are resolved through a probe inside the
// mode root, so each expectation names a role instead of a colour literal.

type Line = {
  width: string;
  fill: string;
  shadow: string;
  radius: string;
  outline: string;
};

function line(target: Locator): Promise<Line> {
  return target.evaluate((node) => {
    const after = getComputedStyle(node, "::after");
    return {
      width: after.width,
      fill: after.backgroundColor,
      shadow: after.boxShadow,
      radius: after.borderRadius,
      outline: getComputedStyle(node).outlineStyle,
    };
  });
}

function resolveFill(page: Page, scope: string, value: string) {
  return page.evaluate(
    ([scopeSelector, fill]) => {
      const host = document.querySelector(scopeSelector);
      if (!host) throw new Error(`missing ${scopeSelector}`);
      const probe = document.createElement("span");
      probe.style.backgroundColor = fill;
      host.append(probe);
      const resolved = getComputedStyle(probe).backgroundColor;
      probe.remove();
      return resolved;
    },
    [scope, value] as const,
  );
}

async function hover(page: Page, target: Locator) {
  const box = await target.boundingBox();
  expect(box).not.toBeNull();
  await page.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2);
}

async function focusByKeyboard(page: Page, start: Locator, target: Locator) {
  await start.focus();
  for (let step = 0; step < 80; step += 1) {
    if (await target.evaluate((node) => node === document.activeElement)) {
      break;
    }
    await page.keyboard.press("Tab");
  }
  await expect(target).toBeFocused();
  expect(await target.evaluate((node) => node.matches(":focus-visible"))).toBe(
    true,
  );
}

// A pointer drag keeps the rail hovered, so the drag state is read while the
// pointer is still over it.
async function drag(page: Page, target: Locator) {
  await hover(page, target);
  await page.mouse.down();
  await expect(target).toHaveClass(/workbench-sidebar-splitter--active/);
  expect(await target.evaluate((node) => node.matches(":hover"))).toBe(true);
}

async function moveAway(page: Page) {
  await page.mouse.move(1, 1);
}

test.describe("Editor activity rail and tool-rail splitter states", () => {
  test.use({ viewport: { width: 1380, height: 880 } });

  test("keeps hairline highlight roles in normal, forced-colour and reduced-motion modes", async ({
    page,
  }) => {
    await openScenario(page, "editor", "editor");
    const scope = ".workbench--editor";
    const explorer = page.getByRole("button", {
      name: "Explorer",
      exact: true,
    });
    const search = page.getByRole("button", { name: "Search", exact: true });
    const splitter = page.getByRole("separator", {
      name: "Resize Editor tool rail",
    });
    await expect(explorer).toHaveAttribute("aria-expanded", "true");

    const buttonBox = await explorer.boundingBox();
    expect(buttonBox).toMatchObject({ width: 40, height: 36 });

    const activeFill = await resolveFill(
      page,
      scope,
      "var(--highlight-active-fill)",
    );
    const activeEdge = await resolveFill(
      page,
      scope,
      "var(--highlight-active-edge)",
    );
    const selectionEdge = await resolveFill(
      page,
      scope,
      "var(--highlight-selection-edge)",
    );
    const focusEdge = await resolveFill(
      page,
      scope,
      "var(--highlight-focus-edge)",
    );

    await moveAway(page);
    expect(
      await explorer.evaluate((node) => getComputedStyle(node).backgroundColor),
    ).toBe(activeFill);
    expect(await line(explorer)).toMatchObject({
      width: "1px",
      radius: "0px",
      fill: activeEdge,
    });
    await hover(page, explorer);
    expect(
      await explorer.evaluate((node) => getComputedStyle(node).backgroundColor),
    ).toBe(activeFill);

    await moveAway(page);
    expect(await line(splitter)).toMatchObject({
      width: "1px",
      fill: "rgba(0, 0, 0, 0)",
    });
    await hover(page, splitter);
    const hovered = await line(splitter);
    expect(hovered).toMatchObject({ width: "1px", fill: selectionEdge });
    expect(hovered.shadow).not.toBe("none");
    await drag(page, splitter);
    const dragged = await line(splitter);
    expect(dragged).toMatchObject({ width: "1px", fill: focusEdge });
    expect(dragged.shadow).not.toBe("none");
    await page.mouse.up();
    await moveAway(page);
    await focusByKeyboard(page, search, splitter);
    expect(await line(splitter)).toMatchObject({
      width: "1px",
      fill: focusEdge,
      outline: "none",
    });

    await page.emulateMedia({ forcedColors: "active" });
    const highlight = await resolveFill(page, scope, "Highlight");
    await moveAway(page);
    expect(await line(explorer)).toMatchObject({
      width: "2px",
      fill: highlight,
    });
    await splitter.evaluate((node) => (node as HTMLElement).blur());
    expect(await line(splitter)).toMatchObject({ width: "1px" });
    await hover(page, splitter);
    expect(await line(splitter)).toMatchObject({
      width: "2px",
      fill: highlight,
    });
    await drag(page, splitter);
    expect(await line(splitter)).toMatchObject({
      width: "2px",
      fill: highlight,
    });
    await page.mouse.up();
    await moveAway(page);
    await focusByKeyboard(page, search, splitter);
    const forcedFocus = await line(splitter);
    expect(forcedFocus).toMatchObject({ width: "2px", fill: highlight });
    expect(forcedFocus.outline).not.toBe("none");

    await page.emulateMedia({ forcedColors: "none", reducedMotion: "reduce" });
    const durations = await explorer.evaluate((node) =>
      getComputedStyle(node).transitionDuration.split(/,\s*/),
    );
    expect(durations.every((duration) => duration === "0s")).toBe(true);
  });
});

test.describe("Agent sidebar splitter states", () => {
  test.use({ viewport: { width: 1380, height: 880 } });

  test("draws the rail edge once and lights the splitter on hover, drag and focus", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    const scope = ".workbench--agent";
    await page.getByRole("button", { name: /^Show files and preview/ }).click();
    const leading = page.getByRole("separator", {
      name: "Resize project and chat rail",
    });
    const trailing = page.getByRole("separator", {
      name: "Resize files and preview",
    });
    await expect(trailing).toBeVisible();

    const subtle = await resolveFill(page, scope, "var(--color-border-subtle)");
    const unpainted = await resolveFill(page, scope, "transparent");
    const accent = await resolveFill(page, scope, "var(--color-accent)");
    const focusEdge = await resolveFill(
      page,
      scope,
      "var(--highlight-focus-edge)",
    );

    await moveAway(page);
    // The rail's border is the edge at rest; the splitter adds no second line.
    expect(await line(leading)).toMatchObject({
      width: "2px",
      fill: unpainted,
    });
    expect(await line(trailing)).toMatchObject({ width: "1px", fill: subtle });
    await hover(page, leading);
    expect(await line(leading)).toMatchObject({
      width: "2px",
      fill: accent,
      shadow: "none",
    });
    await drag(page, leading);
    expect(await line(leading)).toMatchObject({ width: "2px", fill: accent });
    await page.mouse.up();
    await hover(page, trailing);
    expect(await line(trailing)).toMatchObject({ width: "1px", fill: accent });
    await moveAway(page);

    const start = page.getByRole("button", { name: "Hide projects and chats" });
    await focusByKeyboard(page, start, leading);
    expect(await line(leading)).toMatchObject({
      width: "1px",
      fill: focusEdge,
    });

    await page.emulateMedia({ forcedColors: "active" });
    const highlight = await resolveFill(page, scope, "Highlight");
    expect(await line(leading)).toMatchObject({
      width: "2px",
      fill: highlight,
    });
    expect(await line(trailing)).toMatchObject({ width: "2px" });
  });
});
