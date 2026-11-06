import { expect, test } from "./shared/test";
import { settledComposerFit } from "./shared/composerGeometry";
import {
  openScenario,
  settleChatProjections,
  waitForAnimations,
} from "./shared/visualBrowserPrimitives";

for (const control of ["effort", "project"]) {
  test(`keyboard focus follows the ${control} control through overflow and back`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1888, height: 1096 });
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    await page.evaluate(async () => {
      const fixture = await sageImport(
        "/tests/ui/fixtures/surfaces/composerEffortVisualFixture.ts",
      );
      fixture.installVisualComposerEffortFixture();
    });
    const composer = page.locator("form.composer").first();
    await settledComposerFit(composer);
    const trigger =
      control === "effort"
        ? composer.getByRole("button", { name: "Thinking level: Medium" })
        : composer.locator(".project-scope button");
    await composer.getByRole("textbox", { name: "Message agent" }).focus();
    for (
      let index = 0;
      index < 10 &&
      !(await trigger.evaluate((node) => node === document.activeElement));
      index++
    )
      await page.keyboard.press("Tab");
    await expect(trigger).toBeFocused();
    await expect(trigger).toHaveAttribute("aria-expanded", "false");
    await composer.evaluate((element) => (element.style.width = "220px"));
    expect(await settledComposerFit(composer)).toContain(`${control}-menu`);
    const more = composer.getByRole("button", { name: "Composer options" });
    await expect(more).toBeFocused();
    await expect(more).toHaveAttribute("aria-expanded", "false");
    await composer.evaluate((element) => (element.style.width = "560px"));
    await settledComposerFit(composer);
    await expect(trigger).toBeFocused();
    // Once the user leaves the stand-in, a later resize must not steal focus.
    await composer.evaluate((element) => (element.style.width = "220px"));
    await settledComposerFit(composer);
    await expect(more).toBeFocused();
    await page.keyboard.press("Tab");
    const next = await page.evaluate(() =>
      document.activeElement?.getAttribute("aria-label"),
    );
    await composer.evaluate((element) => (element.style.width = "560px"));
    await settledComposerFit(composer);
    expect(
      await page.evaluate(() =>
        document.activeElement?.getAttribute("aria-label"),
      ),
    ).toBe(next);
  });
}

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
    const { useWorkbenchStore } = await sageImport("/src/app/store.ts");
    const { acpProjectionStoreFor } = await sageImport(
      "/src/app/acpProjectionRegistry.ts",
    );
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
  test(`effort stays attached to its trigger across resize at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 1096 });
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    await page.evaluate(async () => {
      const fixture = await sageImport(
        "/tests/ui/fixtures/surfaces/composerEffortVisualFixture.ts",
      );
      fixture.installVisualComposerEffortFixture();
    });
    const composer = page.locator("form.composer").first();
    await settledComposerFit(composer);
    const trigger = composer.getByRole("button", {
      name: "Thinking level: Medium",
    });
    await trigger.click();
    const panel = page.getByRole("dialog", { name: "Thinking level" });
    for (const next of [width, width - 100, width]) {
      await page.setViewportSize({ width: next, height: 1096 });
      await expect(panel).toBeVisible();
      await waitForAnimations(panel);
      await expect
        .poll(async () => {
          const anchor = (await trigger.boundingBox())!;
          const box = (await panel.boundingBox())!;
          return Math.min(
            Math.abs(box.y + box.height - anchor.y),
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

test("Composer options keeps fitting and unmounts overflowed controls", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await page.evaluate(async () => {
    const fixture = await sageImport(
      "/tests/ui/fixtures/surfaces/composerEffortVisualFixture.ts",
    );
    fixture.installVisualComposerEffortFixture();
  });
  const composer = page.locator("form.composer").first();
  await composer.evaluate((element) => (element.style.width = "220px"));
  expect(await settledComposerFit(composer)).toContain("effort-menu");
  await expect(
    composer.getByRole("button", { name: /^Thinking level:/ }),
  ).toHaveCount(0);
  await expect(composer.locator(".project-scope button")).toHaveCount(0);
  const more = composer.getByRole("button", { name: "Composer options" });
  await more.click();
  const menu = page.getByRole("menu", { name: "Composer options menu" });
  await expect(
    menu.getByRole("menuitem", { name: "Thinking level" }),
  ).toBeVisible();
  await composer.evaluate((element) => (element.style.width = "560px"));
  expect(await settledComposerFit(composer)).not.toContain("effort-menu");
  await expect(
    composer.getByRole("button", { name: "Thinking level: Medium" }),
  ).toBeVisible();
  await expect(
    menu.getByRole("menuitem", { name: "Thinking level" }),
  ).toHaveCount(0);
  await composer.evaluate((element) => (element.style.width = "220px"));
  expect(await settledComposerFit(composer)).toContain("effort-menu");
  await expect(
    menu.getByRole("menuitem", { name: "Thinking level" }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  await expect(
    composer.getByRole("button", { name: /^Thinking level:/ }),
  ).toHaveCount(0);
  await more.click();
  await menu.getByRole("menuitem", { name: "Thinking level" }).click();
  await expect(
    page.getByRole("dialog", { name: "Thinking level" }),
  ).toBeVisible();
  await expect(composer.locator("button.composer__effort-picker")).toHaveCount(
    0,
  );
  await page.keyboard.press("Escape");
  await expect(more).toBeFocused();
});
