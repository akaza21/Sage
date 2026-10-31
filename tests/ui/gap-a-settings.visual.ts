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
  await page.screenshot({ path: "output/playwright/gap-a-settings-1380.png" });
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
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    useWorkbenchStore.setState((state) => ({
      harnesses: state.harnesses.map((harness) => ({
        ...harness,
        name: "A discovered harness with a deliberately very long readable name",
      })),
      models: state.models.map((model) => ({
        ...model,
        name: `A discovered model with a deliberately very long readable name ${model.id}`,
      })),
    }));
  });
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
  await page.screenshot({ path: "output/playwright/gap-a-settings-480.png" });
  await expectNoViewportOverflow(page);
});

test("Closing browser Settings restores a live launcher or falls back to the active chat", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1380, height: 880 });
  await openScenario(page, "file", "agent");
  await expect(
    page.getByRole("textbox", {
      name: "Edit src/workbench/AgentChatPanes.tsx",
    }),
  ).toBeVisible();
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const { useAgentArtifactAdmissionStore } =
      await import("/src/app/agentArtifactAdmissionStore.ts");
    const { useAgentArtifactFollowStore } =
      await import("/src/app/agentArtifactFollowStore.ts");
    const state = useWorkbenchStore.getState();
    const owner = useAgentArtifactAdmissionStore.getState().admissions[0].owner;
    useWorkbenchStore.setState({
      tabs: [
        ...state.tabs,
        {
          id: "gap-a-browser",
          kind: "browser",
          title: "New tab",
          position: state.tabs.length,
          dirty: false,
          pinned: false,
        },
      ],
    });
    useAgentArtifactFollowStore
      .getState()
      .pin(owner, { kind: "tab", tabId: "gap-a-browser" });
  });
  await page.getByRole("button", { name: "More options", exact: true }).click();
  await page
    .getByRole("menuitem", { name: "Browser settings", exact: true })
    .click();
  const settings = page.getByRole("region", {
    name: "Agent settings",
    exact: true,
  });
  await expect(settings).toBeVisible();
  await settings.getByRole("button", { name: "Close settings" }).click();
  await expect(
    page.getByRole("button", { name: "More options", exact: true }),
  ).toBeFocused();
  await page.getByRole("button", { name: "More options", exact: true }).click();
  await page
    .getByRole("menuitem", { name: "Browser settings", exact: true })
    .click();
  await expect(settings).toBeVisible();
  await page.evaluate(async () => {
    const { useAgentSettingsState } =
      await import("/src/features/settings/agentSettingsState.ts");
    useAgentSettingsState.getState().launcher?.remove();
  });
  await settings.getByRole("button", { name: "Close settings" }).click();
  await expect(
    page.locator(".agent-chat-pane--focused textarea"),
  ).toBeFocused();
});
