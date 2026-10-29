import { expect, test } from "@playwright/test";
import {
  expectNoViewportOverflow,
  openScenario,
} from "./shared/visualBrowserPrimitives";
import {
  dragAnswerPhrase,
  openSelectionScenario,
} from "./shared/transcriptSelection";

for (const width of [480, 1380]) {
  test(`Agent settings stay contained and separate from Editor at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 880 });
    await openScenario(page, "agent-single", "agent");
    const chatId = await page.evaluate(async () => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      return useWorkbenchStore.getState().activeTabId;
    });
    await page
      .getByRole("button", { name: "Open settings", exact: true })
      .click();
    const settings = page.getByRole("region", {
      name: "Agent settings",
      exact: true,
    });
    await expect(settings).toBeVisible();
    await expect(
      page.getByRole("radio", { name: "Agent", exact: true }),
    ).toBeChecked();
    await expect(
      page.getByRole("complementary", { name: "Projects and chats" }),
    ).toHaveCount(0);
    await expect(
      settings.getByRole("combobox", { name: "Current harness" }),
    ).toBeVisible();
    await expect(
      settings.getByRole("combobox", {
        name: /Default (harness|model|effort)/,
      }),
    ).toHaveCount(0);
    await expect(
      settings.getByText("Effort defaults unavailable"),
    ).toBeVisible();
    await expect(settings).toHaveScreenshot(
      `agent-settings-general-${width}.png`,
    );
    const geometry = await settings.evaluate((element) => {
      const bounds = element.getBoundingClientRect();
      return {
        x: bounds.x,
        right: bounds.right,
        y: bounds.y,
        bottom: bounds.bottom,
      };
    });
    expect(geometry.x).toBeGreaterThanOrEqual(0);
    expect(geometry.right).toBeLessThanOrEqual(width);
    expect(geometry.y).toBeGreaterThanOrEqual(40);
    expect(geometry.bottom).toBeLessThanOrEqual(880);
    await expectNoViewportOverflow(page);

    const nav = settings.getByRole("navigation", {
      name: "Agent settings sections",
    });
    for (const section of [
      "Harnesses",
      "Browser",
      "Appearance",
      "Keyboard shortcuts",
      "Archived chats",
      "Notifications",
      "Approvals",
    ]) {
      await nav.getByRole("button", { name: section, exact: true }).click();
      await expect(
        settings.getByRole("heading", { name: section, exact: true }),
      ).toBeVisible();
      await expectNoViewportOverflow(page);
    }
    await expect(
      settings.getByRole("button", { name: "Reset Don't ask again" }),
    ).toHaveCount(0);
    await expect(
      settings.getByText(/does not expose a reset command/),
    ).toBeVisible();
    await expect(settings).toHaveScreenshot(
      `agent-settings-approvals-${width}.png`,
    );
    await nav
      .getByRole("button", { name: "Keyboard shortcuts", exact: true })
      .click();
    await expect(
      settings.getByRole("row", { name: /Save file/ }),
    ).toContainText("Ctrl+S");
    await expect(
      settings.getByRole("row", { name: /Open settings/ }),
    ).toContainText("Ctrl+,");
    await expect(
      settings.getByRole("row", { name: "New browser tab Ctrl+T" }),
    ).toBeVisible();
    await expect(
      settings.getByRole("row", { name: "Browser zoom in Ctrl++" }),
    ).toBeVisible();
    await settings
      .getByRole("heading", { name: "Browser", exact: true })
      .scrollIntoViewIfNeeded();
    await expect(settings).toHaveScreenshot(
      `agent-settings-keyboard-${width}.png`,
    );
    await nav.getByRole("button", { name: "Back to chats" }).click();
    await expect(settings).toHaveCount(0);
    expect(
      await page.evaluate(async () => {
        const { useWorkbenchStore } = await import("/src/app/store.ts");
        return useWorkbenchStore.getState().activeTabId;
      }),
    ).toBe(chatId);

    await page.keyboard.press("Control+,");
    await expect(settings).toBeVisible();
    await settings.getByRole("button", { name: "Back to chats" }).focus();
    await page.keyboard.press("Escape");
    await expect(settings).toHaveCount(0);
    // The radio is visually hidden inside its label, which takes the click.
    await page
      .getByRole("radiogroup", { name: "Window mode" })
      .locator("label", { hasText: "Editor" })
      .click();
    await expect(
      page.getByRole("radio", { name: "Editor", exact: true }),
    ).toBeChecked();
    await page
      .getByRole("button", { name: "Open settings", exact: true })
      .click();
    // At 480px the compact Editor's open tool drawer is modal, so the
    // canvas behind it is hidden from the accessibility tree while visible.
    const editor = page.getByRole("region", {
      name: "Settings",
      exact: true,
      includeHidden: true,
    });
    await expect(editor).toBeVisible();
    await expect(
      editor.getByRole("tab", {
        name: "Editor",
        exact: true,
        includeHidden: true,
      }),
    ).toBeVisible();
    await expect(
      editor.getByRole("button", { name: "Harnesses", includeHidden: true }),
    ).toHaveCount(0);
  });
}

test("Agent settings are loaded on demand rather than with the first chat", async ({
  page,
}) => {
  // Observe module requests from the network: the scenario's fake clock
  // replaces performance, so resource timing entries are not available.
  let settingsRequests = 0;
  page.on("request", (request) => {
    if (request.url().includes("/features/settings/AgentSettingsSurface"))
      settingsRequests += 1;
  });
  await openScenario(page, "agent-single", "agent");
  const requestedSettings = () => settingsRequests > 0;
  expect(requestedSettings()).toBe(false);
  await page
    .getByRole("button", { name: "Open settings", exact: true })
    .click();
  await expect(
    page.getByRole("region", { name: "Agent settings" }),
  ).toBeVisible();
  expect(requestedSettings()).toBe(true);
});

test("settings preserve the focused chat and split while a shared resource is active", async ({
  page,
}) => {
  // AP-A1: a split exists only after an explicit side-chat action.
  await openSelectionScenario(page);
  await dragAnswerPhrase(page);
  await page
    .getByRole("button", { name: "Ask in side chat", exact: true })
    .click();
  const panes = page.locator(".agent-chat-pane");
  await expect(panes).toHaveCount(2);
  const chatIds = await panes.evaluateAll((elements) =>
    elements.map((element) => element.getAttribute("data-chat-id")),
  );
  const focusedId = await page
    .locator(".agent-chat-pane--focused")
    .getAttribute("data-chat-id");
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    useWorkbenchStore.setState((state) => ({
      tabs: [
        ...state.tabs,
        {
          id: "file:settings-return",
          kind: "file",
          title: "Settings return resource",
          position: state.tabs.length,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "settings-return.ts" },
        },
      ],
      activeTabId: "file:settings-return",
    }));
  });
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("region", {
    name: "Agent settings",
    exact: true,
  });
  await expect(settings).toBeVisible();
  await expect(panes.first()).toBeHidden();
  await settings.getByRole("button", { name: "Back to chats" }).click();
  await expect(settings).toHaveCount(0);
  await expect(page.locator(".agent-chat-pane--focused")).toHaveAttribute(
    "data-chat-id",
    focusedId!,
  );
  expect(
    await panes.evaluateAll((elements) =>
      elements.map((element) => element.getAttribute("data-chat-id")),
    ),
  ).toEqual(chatIds);
});
