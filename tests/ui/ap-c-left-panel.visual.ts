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

test("AP-C chat rows fill on hover and keep the selected fill", async ({
  page,
}) => {
  await openScenario(page, "agent-dual", "agent");
  await installRailBrowseFixture(page);
  const rows = page
    .getByRole("region", { name: "Projects" })
    .locator(".project-chat-row");
  const fill = (index: number) =>
    rows.nth(index).evaluate((row) => getComputedStyle(row).backgroundColor);
  const tokens = await page.evaluate(() => {
    const probe = document.createElement("div");
    document.querySelector(".workbench--agent")!.append(probe);
    const resolve = (token: string) => {
      probe.style.backgroundColor = `var(${token})`;
      return getComputedStyle(probe).backgroundColor;
    };
    const resolved = {
      hover: resolve("--agent-shell-hover"),
      selected: resolve("--agent-shell-selected"),
    };
    probe.remove();
    return resolved;
  });
  await rows.nth(0).locator(".project-chat").click();
  await expect(rows.nth(0)).toHaveClass(/project-chat-row--active/);
  await rows.nth(1).hover();
  await expect.poll(() => fill(1)).toBe(tokens.hover);
  await rows.nth(0).hover();
  await expect.poll(() => fill(0)).toBe(tokens.selected);
});

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

test("AP-C status filters find approval and running chats beyond eight idle rows", async ({
  page,
}) => {
  await openScenario(page, "agent-dual", "agent");
  await installRailBrowseFixture(page);
  const sidebar = page.getByRole("complementary", {
    name: "Projects and chats",
  });
  await sidebar.getByRole("button", { name: "Chat list options" }).click();
  await page
    .getByRole("menuitemradio", { name: "Status", exact: true })
    .click();
  await page.getByRole("menuitemradio", { name: "Needs Attention" }).click();
  await page.keyboard.press("Escape");
  await expect(
    sidebar.getByRole("button", {
      name: "Open chat Agent rail task 8",
      exact: true,
    }),
  ).toBeVisible();
  await takeCanonicalScreenshot(page, "ap-c-attention-overflow.png");
  await sidebar.getByRole("button", { name: "Chat list options" }).click();
  await page
    .getByRole("menuitemradio", { name: "Working", exact: true })
    .click();
  await page.keyboard.press("Escape");
  await expect(
    sidebar.getByRole("button", {
      name: "Open chat Agent rail task 9",
      exact: true,
    }),
  ).toBeVisible();
});

test("AP-C Show more browses and opens a hidden chat without query text", async ({
  page,
}) => {
  await openScenario(page, "agent-dual", "agent");
  await installRailBrowseFixture(page);
  const sidebar = page.getByRole("complementary", {
    name: "Projects and chats",
  });
  const projects = sidebar.getByRole("region", { name: "Projects" });
  await expect(
    projects.getByRole("button", {
      name: "Open chat Agent rail task 8",
      exact: true,
    }),
  ).toHaveCount(0);
  await projects.getByRole("button", { name: "Show more" }).click();
  const query = page.getByRole("searchbox", { name: "Search Chats" });
  await expect(query).toBeFocused();
  await expect(query).toHaveValue("");
  await page.clock.runFor(250);
  const result = page.getByRole("option", { name: /^Agent rail task 8/ });
  await expect(result).toBeVisible();
  await takeCanonicalScreenshot(page, "ap-c-browse-hidden-chats.png");
  await result.click();
  await expect(page.getByRole("dialog", { name: "Search Chats" })).toHaveCount(
    0,
  );
  await expect
    .poll(() =>
      page.evaluate(async () => {
        const { useWorkbenchStore } = await import("/src/app/store.ts");
        return useWorkbenchStore.getState().activeTabId;
      }),
    )
    .toBe("rail-fixture-8");
});
