import { expect, test } from "@playwright/test";
import { openScenario } from "./shared/visualBrowserPrimitives";

test.describe("CP-48 Agent chrome (Codex references 67 and 68)", () => {
  test.use({ viewport: { width: 1920, height: 1080 } });

  test("new tab has two-column tools and bounded, truncated supplied history", async ({
    page,
  }) => {
    await page.goto("/tests/ui/fixtures/browser-new-tab.html");
    const tools = page.locator(".browser-new-tab__tools");
    for (const name of [
      "Changes",
      "Terminal",
      "Files",
      "Side chat",
      "New page",
    ])
      await expect(
        tools.getByRole("button", { name, exact: true }),
      ).toBeVisible();
    const changes = tools.getByRole("button", { name: "Changes", exact: true });
    const terminal = tools.getByRole("button", {
      name: "Terminal",
      exact: true,
    });
    const files = tools.getByRole("button", { name: "Files", exact: true });
    const changesBox = await changes.boundingBox();
    const terminalBox = await terminal.boundingBox();
    const filesBox = await files.boundingBox();
    expect(changesBox!.y).toBe(terminalBox!.y);
    expect(filesBox!.y).toBeGreaterThan(changesBox!.y);
    await expect(changes).toHaveAttribute(
      "aria-keyshortcuts",
      "Control+Shift+G Meta+Shift+G",
    );
    await expect(terminal).toBeDisabled();
    await expect(
      tools.getByRole("button", { name: "Side chat", exact: true }),
    ).toBeDisabled();
    await expect(
      page.locator(".browser-new-tab__suggestions button"),
    ).toHaveCount(4);
    await expect(page.locator(".browser-surface")).toHaveScreenshot(
      "cp48-new-tab-tools.png",
    );
  });

  test("narrow tools remain keyboard-readable in forced colours and reduced motion", async ({
    page,
  }) => {
    await page.emulateMedia({
      forcedColors: "active",
      reducedMotion: "reduce",
    });
    await page.goto("/tests/ui/fixtures/browser-new-tab.html");
    await page.locator(".browser-surface").evaluate((node) => {
      const pane = node as HTMLElement;
      pane.style.width = "390px";
      pane.style.containerType = "inline-size";
    });
    const changes = page.getByRole("button", { name: "Changes", exact: true });
    await changes.focus();
    await expect(changes).toBeFocused();
    await expect(changes).toHaveCSS("outline-style", "solid");
    const columns = await page
      .locator(".browser-new-tab__tools")
      .evaluate((node) =>
        getComputedStyle(node).gridTemplateColumns.split(" "),
      );
    expect(columns).toHaveLength(1);
    await expect(page.locator(".browser-surface")).toHaveScreenshot(
      "cp48-new-tab-forced-colours.png",
    );
  });

  test("header icon order, focused More popover and Changes open the actual chat picker", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    const header = page.locator(".agent-task-header").first();
    const pencil = header.getByRole("button", {
      name: "Rename chat",
      exact: true,
    });
    const more = header.getByRole("button", {
      name: /Chat actions for .* task$/,
    });
    const resources = header.getByRole("button", {
      name: "Show chat resources",
    });
    const pencilBox = await pencil.boundingBox();
    const moreBox = await more.boundingBox();
    const resourcesBox = await resources.boundingBox();
    expect(pencilBox!.x).toBeLessThan(moreBox!.x);
    expect(moreBox!.x).toBeLessThan(resourcesBox!.x);
    // Codex 68 is a pointer open: the ring sits on the trigger, not an item.
    await more.click();
    const menu = page.getByRole("menu", { name: "Chat tools", exact: true });
    await expect(menu).toBeVisible();
    await expect(more).toHaveCSS("outline-style", "solid");
    const [menuBox, headerBox] = await Promise.all([
      menu.boundingBox(),
      header.boundingBox(),
    ]);
    expect(Math.round(menuBox!.width)).toBe(272);
    expect(Math.round(menuBox!.x + menuBox!.width)).toBe(
      Math.round(resourcesBox!.x + resourcesBox!.width),
    );
    expect(menuBox!.y).toBeGreaterThanOrEqual(
      headerBox!.y + headerBox!.height + 6,
    );
    await expect(page).toHaveScreenshot("cp48-header-more.png");
    await page.keyboard.press("Escape");
    await expect(more).toBeFocused();
    await page.keyboard.press("ArrowDown");
    await expect(
      menu.getByRole("menuitem", { name: /More chat actions in/ }),
    ).toBeFocused();
    await menu.getByRole("menuitem", { name: "Changes", exact: true }).click();
    await expect(
      page.getByRole("dialog", { name: "Changes", exact: true }),
    ).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(more).toBeFocused();
    await pencil.click();
    await expect(
      header.getByRole("textbox", { name: "New chat name" }),
    ).toBeFocused();
  });

  test("More popover truncates a long project name without scrolling sideways", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    const header = page.locator(".agent-task-header").first();
    const more = header.getByRole("button", {
      name: /Chat actions for .* task$/,
    });
    const menu = page.getByRole("menu", { name: "Chat tools", exact: true });
    const row = menu.locator(".agent-header-more__project");
    const name = row.locator("> span");
    const projectActions = menu.getByRole("menuitem", {
      name: /^More chat actions in /,
    });
    const changes = menu.getByRole("menuitem", {
      name: "Changes",
      exact: true,
    });
    const measure = () =>
      row.evaluate((rowNode) => {
        const menuNode = rowNode.closest<HTMLElement>('[role="menu"]')!;
        const nameNode = rowNode.querySelector<HTMLElement>(":scope > span")!;
        const actionNode =
          rowNode.querySelector<HTMLElement>('[role="menuitem"]')!;
        const menuBox = menuNode.getBoundingClientRect();
        const rowBox = rowNode.getBoundingClientRect();
        const nameBox = nameNode.getBoundingClientRect();
        const actionBox = actionNode.getBoundingClientRect();
        return {
          menuScrollWidth: menuNode.scrollWidth,
          menuClientWidth: menuNode.clientWidth,
          menuWidth: menuBox.width,
          rowScrollWidth: rowNode.scrollWidth,
          rowClientWidth: rowNode.clientWidth,
          nameScrollWidth: nameNode.scrollWidth,
          nameClientWidth: nameNode.clientWidth,
          nameRight: nameBox.right,
          actionLeft: actionBox.left,
          actionRight: actionBox.right,
          actionWidth: actionBox.width,
          rowRight: rowBox.right,
          menuRight: menuBox.right,
        };
      });

    // Short names render as before: whole, with the action pinned at 24 px.
    const shortName = await page.evaluate(async () => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      return useWorkbenchStore.getState().project!.name;
    });
    await more.click();
    await expect(menu).toBeVisible();
    await expect(name).toHaveText(shortName);
    const short = await measure();
    expect(short.nameScrollWidth).toBe(short.nameClientWidth);
    expect(Math.round(short.actionWidth)).toBe(24);
    expect(short.menuScrollWidth).toBe(short.menuClientWidth);
    await page.keyboard.press("Escape");
    await expect(more).toBeFocused();

    const longName = `Long project ${"name ".repeat(40)}`.slice(0, 200);
    expect(longName).toHaveLength(200);
    await page.evaluate(async (projectName) => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      useWorkbenchStore.setState((state) => ({
        project: { ...state.project!, name: projectName },
      }));
    }, longName);
    await more.click();
    await expect(menu).toBeVisible();

    // The full name stays available to pointer and assistive technology.
    await expect(name).toHaveAttribute("title", longName);
    await expect(name).toHaveText(longName);
    await expect(projectActions).toHaveAccessibleName(
      `More chat actions in ${longName}`,
    );

    const long = await measure();
    expect(Math.round(long.menuWidth)).toBe(272);
    expect(long.menuScrollWidth).toBe(long.menuClientWidth);
    expect(long.rowScrollWidth).toBe(long.rowClientWidth);
    // The name is clipped with an ellipsis rather than widening the row.
    expect(long.nameScrollWidth).toBeGreaterThan(long.nameClientWidth);
    await expect(name).toHaveCSS("text-overflow", "ellipsis");
    // The row's action and the Changes item stay whole inside the menu.
    expect(Math.round(long.actionWidth)).toBe(24);
    expect(long.nameRight).toBeLessThanOrEqual(long.actionLeft);
    expect(long.actionRight).toBeLessThanOrEqual(long.rowRight);
    expect(long.rowRight).toBeLessThanOrEqual(long.menuRight);
    await expect(projectActions).toBeInViewport({ ratio: 1 });
    await expect(changes).toBeInViewport({ ratio: 1 });
    const [changesBox, menuBox] = await Promise.all([
      changes.boundingBox(),
      menu.boundingBox(),
    ]);
    expect(changesBox!.x + changesBox!.width).toBeLessThanOrEqual(
      menuBox!.x + menuBox!.width,
    );
  });
});
