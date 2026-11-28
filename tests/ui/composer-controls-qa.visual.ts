import { expect, test } from "@playwright/test";
import { settledComposerFit } from "./shared/composerGeometry";
import {
  openScenario,
  settleChatProjections,
  waitForAnimations,
} from "./shared/visualBrowserPrimitives";

test("keyboard focus stays on the model chip while the row compacts and expands", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/surfaces/composerEffortVisualFixture.ts");
    fixture.installVisualComposerEffortFixture();
  });
  const composer = page.locator("form.composer").first();
  await settledComposerFit(composer);
  const model = composer.getByRole("button", { name: "Model", exact: true });
  await expect(model.locator(".composer__model-effort")).toHaveText("Medium");
  await composer.getByRole("combobox", { name: "Message agent" }).focus();
  for (
    let index = 0;
    index < 10 &&
    !(await model.evaluate((node) => node === document.activeElement));
    index++
  )
    await page.keyboard.press("Tab");
  await expect(model).toBeFocused();
  await composer.evaluate((element) => (element.style.width = "220px"));
  expect(await settledComposerFit(composer)).toContain("model");
  await expect(model).toBeFocused();
  await expect(model).toHaveAttribute("aria-expanded", "false");
  await composer.evaluate((element) => (element.style.width = "560px"));
  expect(await settledComposerFit(composer)).not.toContain("model");
  await expect(model).toBeFocused();
});

test("dense fitting unmounts the usage meter and restores current usage on expansion", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openScenario(page, "agent-composer-context-known", "agent");
  await settleChatProjections(page);
  const composer = page.locator("form.composer").first();
  const meter = composer.getByRole("progressbar");
  await expect(meter).toBeVisible();
  await composer.evaluate((element) => (element.style.width = "140px"));
  expect(await settledComposerFit(composer)).toContain("dense");
  await expect(composer.locator("[role=progressbar]")).toHaveCount(0);
  await expect(composer.locator(".composer__context")).toHaveCount(0);
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const { acpProjectionStoreFor } =
      await import("/src/app/acpProjectionRegistry.ts");
    const chat = useWorkbenchStore
      .getState()
      .tabs.find((tab) => tab.kind === "chat")!;
    const state = acpProjectionStoreFor(chat.id).getState();
    state.applyAuthoritativePage(state.scope!, {
      ...state.page!,
      turns: state.page!.turns.map((turn) => ({
        ...turn,
        usage: { ...turn.usage!, totalTokens: 12345 },
      })),
    });
  });
  await expect(composer.locator("[role=progressbar]")).toHaveCount(0);
  await composer.evaluate((element) => (element.style.width = "560px"));
  expect(await settledComposerFit(composer)).not.toContain("dense");
  await expect(meter).toBeVisible();
  await expect(meter).toHaveAttribute("aria-valuenow", "12345");
});

for (const width of [1888, 720]) {
  test(`effort stays attached to the model chip across resize at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 1096 });
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    await page.evaluate(async () => {
      const fixture =
        await import("/tests/ui/fixtures/surfaces/composerEffortVisualFixture.ts");
      fixture.installVisualComposerEffortFixture();
    });
    const composer = page.locator("form.composer").first();
    await settledComposerFit(composer);
    const trigger = composer.getByRole("button", {
      name: "Model",
      exact: true,
    });
    await trigger.click();
    const panel = page.getByRole("dialog", { name: "Select model" });
    await expect(
      panel.getByRole("slider", { name: "Thinking level" }),
    ).toHaveAttribute("aria-valuetext", "Medium");
    for (const next of [width, width - 100, width]) {
      await page.setViewportSize({ width: next, height: 1096 });
      await expect(panel).toBeVisible();
      await waitForAnimations(panel);
      await expect
        .poll(async () => {
          const anchor = (await trigger.boundingBox())!;
          const box = (await panel.boundingBox())!;
          // Opening upward, the panel sits on the changed-files summary's top
          // edge rather than covering it.
          const edits = await page
            .locator(".chat-edit-summary-zone:not(:empty)")
            .boundingBox();
          const aboveEdge = edits ? Math.min(edits.y, anchor.y) : anchor.y;
          return Math.min(
            Math.abs(box.y + box.height - aboveEdge),
            Math.abs(box.y - anchor.y - anchor.height),
          );
        })
        .toBeLessThanOrEqual(5);
    }
    await page.keyboard.press("Escape");
    await expect(panel).toHaveCount(0);
    await expect(trigger).toBeFocused();
  });
}

test("Composer options never takes the thinking level as the row compacts", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await page.evaluate(async () => {
    const fixture =
      await import("/tests/ui/fixtures/surfaces/composerEffortVisualFixture.ts");
    fixture.installVisualComposerEffortFixture();
  });
  const composer = page.locator("form.composer").first();
  await composer.evaluate((element) => (element.style.width = "220px"));
  expect(await settledComposerFit(composer)).toContain("effort");
  const more = composer.getByRole("button", { name: "Composer options" });
  await more.click();
  const menu = page.getByRole("menu", { name: "Composer options menu" });
  await expect(menu).toBeVisible();
  await expect(
    menu.getByRole("menuitem", { name: "Thinking level" }),
  ).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(more).toBeFocused();
  // At the narrowest fit the level stays one click away, in the model popover.
  const model = composer.getByRole("button", { name: "Model", exact: true });
  await model.click();
  await expect(
    page
      .getByRole("dialog", { name: "Select model" })
      .getByRole("slider", { name: "Thinking level" }),
  ).toHaveAttribute("aria-valuetext", "Medium");
  await page.keyboard.press("Escape");
  await expect(model).toBeFocused();
});

test("harness and access keep their whole names in the narrowest chat pane", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openScenario(page, "agent-composer-context-known", "agent");
  await settleChatProjections(page);
  const composer = page.locator("form.composer").first();
  // The narrowest chat pane (360 px) leaves 328 px for the column and 320 px
  // inside the composer's outer frame.
  await composer.evaluate((element) => (element.style.width = "320px"));
  await settledComposerFit(composer);
  const whole = (selector: string) =>
    composer.locator(selector).evaluate((label) => ({
      shown: getComputedStyle(label).display !== "none",
      clipped: label.scrollWidth > label.clientWidth + 1,
      text: label.textContent,
    }));
  const harness = await whole(
    ".composer__harness .searchable-picker__trigger-label",
  );
  expect(harness).toMatchObject({ shown: true, clipped: false });
  expect(harness.text).not.toBe("");
  const access = await whole(".composer__access-label");
  expect(access).toMatchObject({ shown: true, clipped: false });
  expect(access.text).not.toBe("");
});
