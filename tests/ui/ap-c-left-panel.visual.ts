import { expect, test, type Page } from "@playwright/test";
import {
  waitForAnimations,
  openScenario,
  takeCanonicalScreenshot,
} from "./shared/visualBrowserPrimitives";

async function installRailBrowseFixture(page: Page) {
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const { projectChatSearchDesktop } =
      await import("/src/features/chatSearch/chatSearchDesktop.ts");
    const state = useWorkbenchStore.getState();
    const chats = Array.from({ length: 10 }, (_, position) => ({
      id: `rail-fixture-${position}`,
      title: `Agent rail task ${position}`,
      kind: "chat" as const,
      position: state.tabs.length + position,
      pinned: false,
      dirty: false,
      status:
        position === 8
          ? ("waiting" as const)
          : position === 9
            ? ("working" as const)
            : ("idle" as const),
      updatedAt: Date.UTC(2026, 7, 31, 9, 29, 0) + position,
    }));
    useWorkbenchStore.setState({
      tabs: [...state.tabs, ...chats],
      activateChat: state.activateTab,
    });
    projectChatSearchDesktop.search = async (request) => ({
      projectId: request.projectId,
      projectGeneration: request.projectGeneration,
      transitionEpoch: request.expectedTransitionEpoch,
      query: request.query,
      results:
        request.projectId === state.project?.id
          ? chats
              .filter((chat) => chat.title.includes(request.query))
              .map((chat) => ({
                chatId: chat.id,
                title: chat.title,
                attached: true,
                matchedTitle: true,
                snippet: null,
              }))
          : [],
      hasMore: false,
      elapsedMs: 1,
    });
  });
}

test("AP-C project disclosure, context actions, and honest project menu", async ({
  page,
}) => {
  await openScenario(page, "agent-dual", "agent");
  const sidebar = page.getByRole("complementary", {
    name: "Projects and chats",
  });
  const projects = sidebar.getByRole("region", { name: "Projects" });
  const disclosure = projects.getByRole("button", {
    name: "Project Sage",
    exact: true,
  });
  await disclosure.click();
  await expect(disclosure).toHaveAttribute("aria-expanded", "false");
  await expect(projects.locator(".project-chat-row")).toHaveCount(0);
  await disclosure.click();
  const row = projects.locator(".project-chat-row").first();
  await row.click({ button: "right" });
  const menu = page.getByRole("menu", { name: /^Actions for / });
  await expect(
    menu.getByRole("menuitem", { name: "Mark unread" }),
  ).toBeVisible();
  await expect(
    menu.getByRole("menuitem", { name: "Rename chat" }),
  ).toBeVisible();
  await takeCanonicalScreenshot(page, "ap-c-chat-context-menu.png");
  await page.keyboard.press("Escape");
  await projects
    .getByRole("button", { name: "Project actions for Sage", exact: true })
    .click();
  const projectMenu = page.getByRole("menu", {
    name: "Project actions for Sage",
  });
  await expect(
    projectMenu.getByRole("menuitem", { name: "New chat in project" }),
  ).toBeEnabled();
  await expect(
    projectMenu.getByRole("menuitem", { name: /^Rename project:/ }),
  ).toBeDisabled();
  await expect(
    projectMenu.getByRole("menuitem", { name: /^Remove project:/ }),
  ).toBeDisabled();
  await expect(
    projectMenu.getByRole("menuitem", { name: /^Reveal project:/ }),
  ).toBeDisabled();
  await takeCanonicalScreenshot(page, "ap-c-project-menu.png");
});

test("AP-C long chat titles truncate before the trailing time or state", async ({
  page,
}) => {
  await openScenario(page, "agent-dual", "agent");
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const state = useWorkbenchStore.getState();
    useWorkbenchStore.setState({
      tabs: state.tabs.map((tab) =>
        tab.kind === "chat"
          ? {
              ...tab,
              title: `${tab.title} with a long title that must truncate`,
              updatedAt: Date.now() - 3 * 60_000,
            }
          : tab,
      ),
    });
  });
  const sidebar = page.getByRole("complementary", {
    name: "Projects and chats",
  });
  const rows = sidebar.locator(".project-chat-row:has(.project-chat__state)");
  await expect(rows.first()).toBeVisible();
  const overlaps = await rows.evaluateAll((nodes) =>
    nodes.flatMap((row) => {
      const title = row.querySelector(".project-chat__title-label");
      const trailing = row.querySelector(".project-chat__state");
      if (!title || !trailing) return [];
      const titleRight = title.getBoundingClientRect().right;
      const trailingLeft = trailing.getBoundingClientRect().left;
      return titleRight > trailingLeft
        ? [`${title.textContent}: ${titleRight} > ${trailingLeft}`]
        : [];
    }),
  );
  expect(overlaps).toEqual([]);
});

test("AP-C sidebar search and Ctrl+K share a focused live palette", async ({
  page,
}) => {
  await openScenario(page, "agent-dual", "agent");
  await installRailBrowseFixture(page);
  const sidebar = page.getByRole("complementary", {
    name: "Projects and chats",
  });
  const trigger = sidebar.getByRole("button", {
    name: "Search chats",
    exact: true,
  });
  await trigger.click();
  const query = page.getByRole("searchbox", { name: "Search Chats" });
  await expect(query).toBeFocused();
  await expect(
    page
      .getByRole("dialog")
      .getByRole("button", { name: "Search", exact: true }),
  ).toHaveCount(0);
  await query.fill("Agent");
  await page.clock.runFor(250);
  const results = page.getByRole("listbox", { name: "Chat search results" });
  await expect(results.getByRole("option").first()).toBeVisible();
  expect(await results.getByRole("option").count()).toBeLessThanOrEqual(64);
  await takeCanonicalScreenshot(page, "ap-c-live-chat-search.png");
  await query.press("Escape");
  await expect(trigger).toBeFocused();
  await page.keyboard.press("Control+k");
  await expect(query).toBeFocused();
  await expect(query).toHaveValue("");
});

test("Search Chats keeps Cancel reachable when a pending search fails", async ({
  page,
}) => {
  await page.setViewportSize({ width: 690, height: 440 });
  await openScenario(page, "agent-composer-narrow", "agent");
  const pending = await page.evaluateHandle(async () => {
    const { projectChatSearchDesktop } =
      await import("/src/features/chatSearch/chatSearchDesktop.ts");
    let reject!: (reason: Error) => void;
    projectChatSearchDesktop.search = () =>
      new Promise<never>((_, fail) => {
        reject = fail;
      });
    return { fail: () => reject(new Error("Search became unavailable.")) };
  });
  const trigger = page
    .locator("header.title-bar")
    .getByRole("button", { name: "Search chats" });
  await trigger.click();
  const dialog = page.getByRole("dialog", { name: "Search Chats" });
  await page.clock.runFor(250);
  await expect(dialog.getByRole("status")).toHaveText("Searching…");
  const cancel = dialog.getByRole("button", { name: "Cancel", exact: true });
  await cancel.click({ trial: true });
  await waitForAnimations(dialog);
  const before = (await cancel.boundingBox())!;
  await page.mouse.move(
    before.x + before.width / 2,
    before.y + before.height / 2,
  );
  await page.mouse.down();
  await pending.evaluate((request) => request.fail());
  await expect(dialog.getByRole("alert")).toHaveText(
    "Search became unavailable.",
  );
  const after = (await cancel.boundingBox())!;
  expect({ x: after.x, y: after.y }).toEqual({ x: before.x, y: before.y });
  await page.mouse.up();
  await expect(dialog).toHaveCount(0);
  await expect(trigger).toBeFocused();
  await pending.dispose();
});

test("AP-C sidebar options organize chats in one list and sort them", async ({
  page,
}) => {
  await openScenario(page, "agent-dual", "agent");
  await installRailBrowseFixture(page);
  const sidebar = page.getByRole("complementary", {
    name: "Projects and chats",
  });
  const options = sidebar.getByRole("button", { name: "Sidebar options" });
  await options.click();
  await page.getByRole("menuitem", { name: "Organize sidebar" }).hover();
  await page.getByRole("menuitemradio", { name: "In one list" }).click();
  await expect(options).toBeFocused();
  await expect(sidebar.getByRole("region", { name: "Projects" })).toHaveCount(
    0,
  );
  const recents = sidebar.getByRole("region", { name: "Recents" });
  const titles = recents.locator(".project-chat__title-label");
  await expect(titles.first()).toHaveText("Agent rail task 9");

  await options.click();
  await page
    .getByRole("menuitem", { name: "Sort chats by" })
    .press("ArrowRight");
  const manual = page.getByRole("menuitemradio", { name: "Manual order" });
  await expect(
    page.getByRole("menuitemradio", { name: "Last updated" }),
  ).toHaveAttribute("aria-checked", "true");
  await manual.click();
  await expect(titles.first()).not.toHaveText("Agent rail task 9");
  // Manual order follows each chat's place in its project, not its activity.
  const order = await titles.allTextContents();
  expect(order.indexOf("Agent rail task 0")).toBeGreaterThanOrEqual(0);
  expect(order.indexOf("Agent rail task 0")).toBeLessThan(
    order.indexOf("Agent rail task 1"),
  );
});

test("AP-C Show more lists a project's hidden chats in place", async ({
  page,
}) => {
  await openScenario(page, "agent-dual", "agent");
  await installRailBrowseFixture(page);
  const sidebar = page.getByRole("complementary", {
    name: "Projects and chats",
  });
  const projects = sidebar.getByRole("region", { name: "Projects" });
  const hidden = projects.getByRole("button", {
    name: "Open chat Agent rail task 0",
    exact: true,
  });
  await expect(hidden).toHaveCount(0);
  await projects.getByRole("button", { name: "Show more" }).first().click();
  await expect(page.getByRole("dialog", { name: "Search Chats" })).toHaveCount(
    0,
  );
  await hidden.click();
  await expect
    .poll(() =>
      page.evaluate(async () => {
        const { useWorkbenchStore } = await import("/src/app/store.ts");
        return useWorkbenchStore.getState().activeTabId;
      }),
    )
    .toBe("rail-fixture-0");
  await projects.getByRole("button", { name: "Show less" }).first().click();
  await expect(
    projects.getByRole("button", {
      name: "Open chat Agent rail task 1",
      exact: true,
    }),
  ).toHaveCount(0);
});

test("AP-C hover Pin and Archive take a real pointer click without opening the chat", async ({
  page,
}) => {
  await openScenario(page, "agent-dual", "agent");
  await installRailBrowseFixture(page);
  // The fixture's chats live only in the store, so pin and archive change the
  // store's projection directly; what is under test is where the click lands.
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    useWorkbenchStore.setState({
      pinChat: async (chatId: string) =>
        useWorkbenchStore.setState((state) => ({
          tabs: state.tabs.map((tab) =>
            tab.id === chatId ? { ...tab, pinned: !tab.pinned } : tab,
          ),
        })),
      archiveChat: async (chatId: string) => {
        useWorkbenchStore.setState((state) => ({
          archivedChats: [{ id: chatId, title: chatId }],
          tabs: state.tabs.filter((tab) => tab.id !== chatId),
        }));
        return true;
      },
    });
  });
  const sidebar = page.getByRole("complementary", {
    name: "Projects and chats",
  });
  const projects = sidebar.getByRole("region", { name: "Projects" });
  const activeChat = () =>
    page.evaluate(async () => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      return useWorkbenchStore.getState().activeTabId;
    });
  const before = await activeChat();
  const clickCentre = async (name: string) => {
    const button = projects.getByRole("button", { name, exact: true });
    await button
      .locator("xpath=ancestor::*[contains(@class,'project-chat-row')][1]")
      .hover();
    const box = await button.boundingBox();
    expect(box).not.toBeNull();
    await page.mouse.click(box!.x + box!.width / 2, box!.y + box!.height / 2);
  };

  // Idle rows carry no state glyph, so their title button spans the whole
  // row, under the actions.
  await clickCentre("Pin Agent rail task 7");
  await expect(
    sidebar
      .getByRole("region", { name: "Pinned" })
      .getByRole("button", { name: /^Open chat Agent rail task 7/ }),
  ).toBeVisible();
  expect(await activeChat()).toBe(before);

  await clickCentre("Archive Agent rail task 6");
  await expect(
    projects.getByRole("button", {
      name: "Open chat Agent rail task 6",
      exact: true,
    }),
  ).toHaveCount(0);
  await expect(
    sidebar.getByRole("button", { name: "Undo archive" }),
  ).toBeVisible();
  expect(await activeChat()).toBe(before);
});

test("AP-C resting on a chat row shows its title, age and project beside the rail", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1872, height: 1080 });
  await openScenario(page, "agent-dual", "agent");
  await installRailBrowseFixture(page);
  const projectName = await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    // Pin the clock so the age reads the same on every run.
    Date.now = () => Date.UTC(2026, 9, 1, 9, 29, 0);
    return useWorkbenchStore.getState().project?.name ?? "";
  });
  expect(projectName).not.toBe("");
  const sidebar = page.locator(".project-sidebar");
  const title = sidebar
    .getByRole("region", { name: "Projects" })
    .getByRole("button", { name: "Open chat Agent rail task 7", exact: true });
  const row = title.locator(
    "xpath=ancestor::*[contains(@class,'project-chat-row')][1]",
  );
  const card = page.locator(".agent-rail-hover-card");
  // No native tooltip competes with the card.
  await expect(title).not.toHaveAttribute("title");

  await row.hover();
  await expect(card).toBeVisible();
  await expect(card.locator("strong")).toHaveText("Agent rail task 7");
  await expect(card).toContainText("1mo");
  await expect(card).toContainText(projectName);
  await expect(card).toHaveAttribute("aria-hidden", "true");
  const rowBox = (await row.boundingBox())!;
  const railBox = (await sidebar.boundingBox())!;
  const cardBox = (await card.boundingBox())!;
  // Level with the row, starting at the rail's edge and reaching over the
  // center pane rather than covering the rows below.
  expect(Math.abs(cardBox.y - rowBox.y)).toBeLessThanOrEqual(1);
  expect(Math.abs(cardBox.x - (rowBox.x + rowBox.width))).toBeLessThanOrEqual(
    2,
  );
  expect(cardBox.x + cardBox.width).toBeGreaterThan(
    railBox.x + railBox.width + 200,
  );
  // Hit-testing follows paint order: once the card accepts hits it is the
  // topmost box over the center pane.
  const point = {
    x: cardBox.x + cardBox.width - 16,
    y: cardBox.y + cardBox.height / 2,
  };
  const onTop = await card.evaluate((node, { x, y }) => {
    (node as HTMLElement).style.pointerEvents = "auto";
    const hit = document.elementFromPoint(x, y);
    (node as HTMLElement).style.pointerEvents = "";
    return Boolean(hit && node.contains(hit));
  }, point);
  expect(onTop).toBe(true);
  // Normally it never takes the pointer.
  expect(
    await page.evaluate(
      ({ x, y }) =>
        document.elementFromPoint(x, y)?.closest(".agent-rail-hover-card") ??
        null,
      point,
    ),
  ).toBeNull();

  await page.mouse.move(900, 900);
  await expect(card).toHaveCount(0);
});

test("AP-C chat rows keep one box in both organize modes, with the dot where Archive appears", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1872, height: 1080 });
  await openScenario(page, "agent-dual", "agent");
  const geometry = () =>
    page.evaluate(() => {
      const edge = (node: Element | null) => {
        const box = node!.getBoundingClientRect();
        return {
          left: box.left,
          right: box.right,
          centre: box.left + box.width / 2,
        };
      };
      const rows = [...document.querySelectorAll(".project-chat-row")];
      const dotted = rows.find((row) =>
        row.querySelector(
          ".project-chat__state--unread, .project-chat__state--working",
        ),
      );
      return {
        row: edge(rows[0]),
        trigger: edge(document.querySelector(".agent-rail-options__trigger")),
        dot: dotted
          ? edge(dotted.querySelector(".project-chat__state")).centre
          : null,
        title: rows.map((row) => {
          const button = row.querySelector(".project-chat")!;
          return (
            button.getBoundingClientRect().right -
            parseFloat(getComputedStyle(button).paddingRight)
          );
        }),
      };
    });
  const byProject = await geometry();
  // A project's chats fill the whole list, like Recents rows.
  expect(byProject.row.left).toBe(
    (await page.locator(".project-entry__header").first().boundingBox())!.x,
  );
  // Every title clips at one point, with or without a trailing dot.
  expect(new Set(byProject.title).size).toBe(1);
  expect(byProject.dot).not.toBeNull();

  const row = page.locator(".project-chat-row").first();
  await row.hover();
  const archive = row.getByRole("button", { name: /^Archive / });
  const archiveBox = (await archive.boundingBox())!;
  expect(
    Math.abs(archiveBox.x + archiveBox.width / 2 - byProject.dot!),
  ).toBeLessThanOrEqual(1);
  const hoveredClip = await row
    .locator(".project-chat")
    .evaluate(
      (button) =>
        button.getBoundingClientRect().right -
        parseFloat(getComputedStyle(button).paddingRight),
    );
  const pinBox = (await row
    .getByRole("button", { name: /^(Pin|Unpin) / })
    .boundingBox())!;
  expect(hoveredClip).toBeLessThanOrEqual(pinBox.x);

  await page.mouse.move(900, 900);
  await page.evaluate(async () => {
    const prefs = await import("/src/workbench/agentRailViewPrefs.ts");
    prefs.setAgentRailOrganize("list");
  });
  await page.getByRole("region", { name: "Recents" }).waitFor();
  const oneList = await geometry();
  expect(oneList.row.right).toBe(byProject.row.right);
  expect(oneList.trigger).toEqual(byProject.trigger);
  expect(oneList.title[0]).toBe(byProject.title[0]);
});

test("AP-C sidebar options open from the trigger toward the center pane", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1872, height: 1080 });
  await openScenario(page, "agent-dual", "agent");
  const trigger = page.getByRole("button", { name: "Sidebar options" });
  await trigger.click();
  const menu = page.getByRole("menu", { name: "Sidebar options" });
  await expect(menu).toBeVisible();
  await waitForAnimations(menu);
  const triggerBox = (await trigger.boundingBox())!;
  const menuBox = (await menu.boundingBox())!;
  expect(Math.round(menuBox.x)).toBe(Math.round(triggerBox.x));
  await menu.getByRole("menuitem", { name: "Organize sidebar" }).click();
  const submenu = page.getByRole("menu", { name: "Organize sidebar" });
  await expect(submenu).toBeVisible();
  await waitForAnimations(submenu);
  expect((await submenu.boundingBox())!.x).toBeGreaterThanOrEqual(
    menuBox.x + menuBox.width,
  );
});

test("AP-C Create project offers only this computer as a folder location", async ({
  page,
}) => {
  await openScenario(page, "agent-dual", "agent");
  await page
    .getByRole("complementary", { name: "Projects and chats" })
    .getByRole("button", { name: "Create project" })
    .click();
  const dialog = page.getByRole("dialog", { name: "Create project" });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("Add a folder on this computer");

  const location = dialog.getByRole("button", { name: "this computer" });
  await expect(location).toHaveAttribute("aria-haspopup", "menu");
  const box = await location.boundingBox();
  await page.mouse.click(box!.x + box!.width / 2, box!.y + box!.height / 2);
  const menu = page.getByRole("menu", { name: "Folder location" });
  await expect(menu).toBeVisible();
  // Sage reaches only this machine: no remote devices are listed or offered.
  await expect(menu.getByRole("menuitemradio")).toHaveText(["This computer"]);
  await expect(
    menu.getByRole("menuitemradio", { name: "This computer" }),
  ).toHaveAttribute("aria-checked", "true");
  await expect(menu.getByRole("menuitemradio")).toBeFocused();

  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
  await expect(dialog).toBeVisible();
  await expect(location).toBeFocused();
});
