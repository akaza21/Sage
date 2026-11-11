import { expect, test, type Locator, type Page } from "@playwright/test";

import { openScenario } from "./shared/visualBrowserPrimitives";

async function closeChatSessions(page: Page) {
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

test("an unavailable harness's reason reads on two lines inside the menu", async ({
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
  const reason = list
    .getByRole("option", { name: /^Unavailable Codex/ })
    .locator(".searchable-picker__reason");
  await expect(reason).toHaveText(
    "Sage did not admit this harness and will not start it.",
  );
  const layout = await reason.evaluate((element) => {
    const range = document.createRange();
    range.selectNodeContents(element);
    const lines = new Set(
      [...range.getClientRects()].map((rect) => Math.round(rect.top)),
    );
    return {
      lines: lines.size,
      clipped: element.scrollHeight > element.clientHeight + 1,
    };
  });
  expect(layout).toEqual({ lines: 2, clipped: false });
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
  await expect(effort.locator("svg").last()).toBeVisible();
  const model = composer.getByRole("button", { name: "Model", exact: true });
  const treatment = (control: Locator) =>
    control.evaluate((element) => {
      const style = getComputedStyle(element);
      return [style.height, style.fontSize, style.fontWeight];
    });
  expect(await treatment(effort)).toEqual(await treatment(model));
});
