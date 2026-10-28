import { expect, test } from "@playwright/test";
import {
  openScenario,
  takeCanonicalScreenshot,
} from "./shared/visualBrowserPrimitives";

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

test("AP-C sidebar search and Ctrl+K share a focused live palette", async ({
  page,
}) => {
  await openScenario(page, "agent-dual", "agent");
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
