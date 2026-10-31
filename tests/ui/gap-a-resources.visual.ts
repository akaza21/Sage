import { expect, test } from "@playwright/test";
import { openAgentScenario, setResourceState } from "./resourceVisualHelpers";
import { expectNoViewportOverflow } from "./shared/visualBrowserPrimitives";

test("Files uses integrated controls and keeps focused row rings inside its scroller", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1380, height: 880 });
  await openAgentScenario(page, "file");
  await expect(
    page.getByRole("textbox", {
      name: "Edit src/workbench/AgentChatPanes.tsx",
    }),
  ).toBeVisible();
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const { useAgentArtifactAdmissionStore } =
      await import("/src/app/agentArtifactAdmissionStore.ts");
    const state = useWorkbenchStore.getState();
    const admission = useAgentArtifactAdmissionStore.getState().admissions[0];
    const base = state.tabs.find((tab) => tab.kind === "file")!;
    const tabs = Array.from({ length: 24 }, (_, index) => ({
      ...base,
      id: `gap-a-file-${index}`,
      position: state.tabs.length + index,
      resource: {
        ...base.resource!,
        relativePath: `src/${"long-directory/".repeat(8)}file-${index}.ts`,
      },
    }));
    useWorkbenchStore.setState({ tabs: [...state.tabs, ...tabs] });
    for (const tab of tabs)
      useAgentArtifactAdmissionStore
        .getState()
        .admit({ ...admission, tabId: tab.id });
  });
  const launcher = page.locator(".agent-chat-pane--focused textarea");
  await launcher.focus();
  await page.keyboard.press("Control+p");
  const files = page.getByRole("dialog", { name: "Files", exact: true });
  await expect(files).toBeVisible();
  const input = files.getByRole("textbox", { name: "Filter chat files" });
  expect(
    await input.evaluate((node) =>
      Number.parseFloat(getComputedStyle(node).borderRadius),
    ),
  ).toBeGreaterThan(0);
  const row = files
    .getByRole("group", { name: "Chat files" })
    .getByRole("button")
    .first();
  await input.focus();
  await page.keyboard.press("Tab");
  await expect(row).toBeFocused();
  expect(
    await row.evaluate((node) => {
      const style = getComputedStyle(node);
      return (
        Number.parseFloat(style.outlineOffset) <=
        -Number.parseFloat(style.outlineWidth)
      );
    }),
  ).toBe(true);
  const last = files
    .getByRole("group", { name: "Chat files" })
    .getByRole("button")
    .last();
  await last.focus();
  await expect(last).toBeFocused();
  expect(
    await last.evaluate((node) => {
      const bounds = node.getBoundingClientRect();
      const list = node
        .closest(".agent-tool-panel__list")!
        .getBoundingClientRect();
      const style = getComputedStyle(node);
      return (
        bounds.top >= list.top &&
        bounds.bottom <= list.bottom &&
        Number.parseFloat(style.outlineOffset) < 0
      );
    }),
  ).toBe(true);
  await input.fill("file-23.ts");
  await expect(
    files.getByRole("group", { name: "Chat files" }).getByRole("button"),
  ).toHaveCount(1);
  await input.press("Enter");
  await expect(files).toHaveCount(0);
  expect(
    await page.evaluate(async () => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      const { useAgentArtifactFollowStore } =
        await import("/src/app/agentArtifactFollowStore.ts");
      const state = useWorkbenchStore.getState();
      const record = useAgentArtifactFollowStore
        .getState()
        .records.find((entry) => entry.owner.chatId === state.activeTabId);
      return { chatId: state.activeTabId, selected: record?.selected };
    }),
  ).toEqual({
    chatId: "visual-chat-file",
    selected: { kind: "tab", tabId: "gap-a-file-23" },
  });
  await expect(
    page.locator('.file-surface[aria-label$="file-23.ts"]'),
  ).toBeVisible();
  await launcher.focus();
  await page.keyboard.press("Control+p");
  await expect(files).toBeVisible();
  await files.getByRole("button", { name: "Close", exact: true }).click();
  await expect(files).toHaveCount(0);
  await expect(launcher).toBeFocused();
  await expectNoViewportOverflow(page);
});

test("An unbroken native error wraps inside a 320px resource pane", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1380, height: 880 });
  await openAgentScenario(page, "file");
  await expect(
    page.getByRole("textbox", {
      name: "Edit src/workbench/AgentChatPanes.tsx",
    }),
  ).toBeVisible();
  await setResourceState(page, "file-error");
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const editor = useWorkbenchStore.getState().activeEditor;
    if (editor.phase === "error")
      useWorkbenchStore.setState({
        activeEditor: { ...editor, message: `D:\\${"a".repeat(500)}` },
      });
  });
  const detail = page
    .locator(".resource-state__detail")
    .filter({ hasText: "aaaa" });
  await detail.evaluate((node) => {
    const pane = node.closest<HTMLElement>(".agent-artifact-pane")!;
    pane.style.width = "320px";
  });
  expect(
    await detail.evaluate((node) => {
      const bounds = node.getBoundingClientRect();
      const pane = node
        .closest(".agent-artifact-pane")!
        .getBoundingClientRect();
      return {
        wraps: bounds.height > 30,
        contained: bounds.left >= pane.left && bounds.right <= pane.right,
        noOverflow: node.scrollWidth <= node.clientWidth,
      };
    }),
  ).toEqual({ wraps: true, contained: true, noOverflow: true });
  await expectNoViewportOverflow(page);
});
