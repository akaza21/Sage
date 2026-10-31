import { expect, test } from "@playwright/test";
import {
  openScenario,
  expectNoViewportOverflow,
} from "./shared/visualBrowserPrimitives";

test("Appearance density changes rail and Settings geometry at 16px without clipping", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1380, height: 880 });
  await openScenario(page, "agent-single", "agent");
  const row = page.locator(".project-chat").first();
  const before = await row.evaluate(
    (node) => node.getBoundingClientRect().height,
  );
  await page.evaluate(async () => {
    const { useUiAppearanceStore } =
      await import("/src/app/uiAppearanceStore.ts");
    useUiAppearanceStore.setState({
      appearance: { fontSize: 16, density: "compact" },
    });
  });
  expect(
    await row.evaluate((node) => node.getBoundingClientRect().height),
  ).toBeLessThan(before);
  await page.keyboard.press("Control+,");
  const nav = page.getByRole("button", { name: "Appearance", exact: true });
  const compact = await nav.evaluate(
    (node) => node.getBoundingClientRect().height,
  );
  await page.evaluate(async () => {
    const { useUiAppearanceStore } =
      await import("/src/app/uiAppearanceStore.ts");
    useUiAppearanceStore.setState({
      appearance: { fontSize: 16, density: "comfortable" },
    });
  });
  expect(
    await nav.evaluate((node) => node.getBoundingClientRect().height),
  ).toBeGreaterThan(compact);
  await expectNoViewportOverflow(page);
});

test("Settings fills the workspace and returns focus to its live launcher", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1380, height: 880 });
  await openScenario(page, "agent-single", "agent");
  const launcher = page.getByRole("button", {
    name: "Open settings",
    exact: true,
  });
  await launcher.click();
  const settings = page.getByRole("region", {
    name: "Agent settings",
    exact: true,
  });
  await expect(settings).toBeVisible();
  await expect(settings).not.toHaveAttribute("aria-modal");
  expect(
    await settings.evaluate((node) => node.getBoundingClientRect().x),
  ).toBe(0);
  await expect(
    page.getByRole("radio", { name: "Editor", exact: true }),
  ).toBeEnabled();
  await settings.getByRole("button", { name: "Close settings" }).click();
  await expect(launcher).toBeFocused();
  await launcher.click();
  await settings.getByRole("button", { name: "Close settings" }).focus();
  await page.keyboard.press("Escape");
  await expect(settings).toHaveCount(0);
  await expect(launcher).toBeFocused();
});

test("Narrow Settings controls fill their row and compact buttons retain their size", async ({
  page,
}) => {
  await page.setViewportSize({ width: 480, height: 880 });
  await openScenario(page, "agent-single", "agent");
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("region", {
    name: "Agent settings",
    exact: true,
  });
  await settings.getByRole("button", { name: "Chats", exact: true }).click();
  const control = settings.getByRole("combobox", { name: "Current harness" });
  expect(
    await control.evaluate((node) => {
      const row = node.closest(".agent-settings__row")!;
      return (
        node.getBoundingClientRect().width / row.getBoundingClientRect().width
      );
    }),
  ).toBeGreaterThan(0.9);
  await control.focus();
  await expect(control).toBeFocused();
  await expectNoViewportOverflow(page);
});
