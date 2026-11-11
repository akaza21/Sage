import { expect, test, type Locator, type Page } from "@playwright/test";

import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

async function closeChatSessions(page: Page) {
  // A late projection load would reopen the sessions after this edit.
  await settleChatProjections(page);
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const { acpProjectionStoreFor } =
      await import("/src/app/acpProjectionRegistry.ts");
    const chat = useWorkbenchStore
      .getState()
      .tabs.find((tab) => tab.kind === "chat")!;
    const projection = acpProjectionStoreFor(chat.id).getState();
    projection.applyAuthoritativePage(projection.scope!, {
      ...projection.page!,
      sessions: projection.page!.sessions.map((session) => ({
        ...session,
        state: "closed",
      })),
    });
  });
}

test("an unavailable harness's reason reads in full inside the menu", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await openScenario(page, "agent-picker-harness-model", "agent");
  await page
    .locator("form.composer")
    .first()
    .getByRole("button", { name: "Harness", exact: true })
    .click();
  const list = page.getByRole("listbox", { name: "Harness", exact: true });
  const option = list.getByRole("option", { name: /^Unavailable Codex/ });
  const reason = option.locator(".searchable-picker__reason");
  // The row's tooltip carries the full reason; the inline copy must show it
  // whole on at most two lines rather than cutting it to one.
  const fullReason = await option.getAttribute("title");
  expect(fullReason).toBeTruthy();
  await expect(reason).toHaveText(fullReason!);
  const layout = await reason.evaluate((element) => {
    const range = document.createRange();
    range.selectNodeContents(element);
    const lines = new Set(
      [...range.getClientRects()].map((rect) => Math.round(rect.top)),
    );
    return {
      lines: lines.size,
      clipped:
        element.scrollHeight > element.clientHeight + 1 ||
        element.scrollWidth > element.clientWidth + 1,
    };
  });
  expect(layout.lines).toBeLessThanOrEqual(2);
  expect(layout.clipped).toBe(false);
  const menu = (await list.boundingBox())!;
  expect(menu.x).toBeGreaterThanOrEqual(0);
  expect(menu.x + menu.width).toBeLessThanOrEqual(1280);
});

test("thinking level waiting for a session is a disabled picker like Model", async ({
  page,
}) => {
  await openScenario(page, "agent-single", "agent");
  await closeChatSessions(page);
  const composer = page.locator("form.composer").first();
  const effort = composer.getByRole("button", {
    name: "Thinking level",
    exact: true,
  });
  await expect(effort).toBeDisabled();
  await expect(effort).toHaveAccessibleDescription(/.+/);
  await expect(effort).toHaveAttribute("title", /.+/);
  await expect(effort.locator(".composer__effort-chevron")).toBeVisible();
  const model = composer.getByRole("button", { name: "Model", exact: true });
  const treatment = (control: Locator) =>
    control.evaluate((element) => {
      const style = getComputedStyle(element);
      return [style.height, style.fontSize, style.fontWeight];
    });
  expect(await treatment(effort)).toEqual(await treatment(model));
});
