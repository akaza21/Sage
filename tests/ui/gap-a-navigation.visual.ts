import { expect, test } from "@playwright/test";
import { openScenario } from "./shared/visualBrowserPrimitives";

test("Back and Forward restore admitted chats and Settings with focus and one pane", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1380, height: 880 });
  await openScenario(page, "agent-single", "agent");
  const before = await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const state = useWorkbenchStore.getState();
    return { tabs: state.tabs.map((tab) => tab.id), chatId: state.activeTabId };
  });
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("region", {
    name: "Agent settings",
    exact: true,
  });
  await expect(settings).toBeVisible();
  await settings.getByRole("button", { name: "Chats", exact: true }).click();
  await page.getByRole("button", { name: "Go back", exact: true }).click();
  await expect(
    settings.getByRole("heading", { name: "Appearance", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Go back", exact: true }).click();
  await expect(settings).toHaveCount(0);
  await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
  await expect(
    page.locator(".agent-chat-pane--focused textarea"),
  ).toBeFocused();
  await page.getByRole("button", { name: "Go forward", exact: true }).click();
  await expect(settings).toBeVisible();
  await expect(settings.getByRole("searchbox")).toBeFocused();
  expect(
    await page.evaluate(async () => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      const state = useWorkbenchStore.getState();
      return {
        tabs: state.tabs.map((tab) => tab.id),
        chatId: state.activeTabId,
      };
    }),
  ).toEqual(before);
});
