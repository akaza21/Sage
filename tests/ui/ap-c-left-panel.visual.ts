import { expect, test, type Page } from "@playwright/test";
import {
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

test("AP-C resting on a chat row shows its full title and project", async ({
  page,
}) => {
  await openScenario(page, "agent-dual", "agent");
  await installRailBrowseFixture(page);
  const projectName = await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    return useWorkbenchStore.getState().project?.name ?? "";
  });
  expect(projectName).not.toBe("");
  const projects = page
    .getByRole("complementary", { name: "Projects and chats" })
    .getByRole("region", { name: "Projects" });
  const title = projects.getByRole("button", {
    name: "Open chat Agent rail task 7",
    exact: true,
  });
  const row = title.locator(
    "xpath=ancestor::*[contains(@class,'project-chat-row')][1]",
  );
  const card = row.locator(".project-chat__hover-card");
  // No native tooltip competes with the card.
  await expect(title).not.toHaveAttribute("title");
  await expect(card).toBeHidden();

  await row.hover();
  await expect(card).toBeVisible();
  expect(
    await card.evaluate((node) => [
      getComputedStyle(node, "::before").content,
      getComputedStyle(node, "::after").content,
    ]),
  ).toEqual(['"Agent rail task 7"', JSON.stringify(projectName)]);
  const rowBox = await row.boundingBox();
  const cardBox = await card.boundingBox();
  expect(cardBox!.y).toBeGreaterThanOrEqual(rowBox!.y + rowBox!.height);
  const point = { x: cardBox!.x + 8, y: cardBox!.y + cardBox!.height / 2 };
  // The card never takes the pointer, so the row under it stays clickable.
  expect(
    await page.evaluate(
      ({ x, y }) =>
        document.elementFromPoint(x, y)?.closest(".project-chat__hover-card") ??
        null,
      point,
    ),
  ).toBeNull();
  // Hit-testing follows paint order: once the card accepts hits it is the
  // topmost box there, so it paints over the next row rather than under it.
  const onTop = await card.evaluate((node, { x, y }) => {
    (node as HTMLElement).style.pointerEvents = "auto";
    const hit = document.elementFromPoint(x, y);
    (node as HTMLElement).style.pointerEvents = "";
    return Boolean(hit && node.contains(hit));
  }, point);
  expect(onTop).toBe(true);

  await page.mouse.move(0, 0);
  await expect(card).toBeHidden();
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
