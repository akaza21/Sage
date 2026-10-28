import { expect, test } from "@playwright/test";
import {
  openAgentScenario,
  openAgentSnapshotDiff,
} from "./resourceVisualHelpers";

test.describe("AP-F right pane (Codex references 26, 67 and 68)", () => {
  test.use({ viewport: { width: 1380, height: 880 } });

  test("opens Changes sections, returns to a per-file Diff and launches New tab tools", async ({
    page,
  }) => {
    await openAgentScenario(page, "diff");
    await openAgentSnapshotDiff(page);
    await page.keyboard.press("Control+Shift+G");
    const pane = page.getByLabel("Agent artifacts", { exact: true });
    await expect(
      pane.getByRole("tab", { name: "Changes", exact: true }),
    ).toHaveAttribute("aria-selected", "true");
    const changes = pane.getByRole("region", {
      name: "Chat changes",
      exact: true,
    });
    await expect(
      changes.getByRole("combobox", { name: "Changes scope" }),
    ).toHaveValue("lastTurn");
    await expect(
      changes.getByRole("table", { name: "Unified file diff" }),
    ).toBeVisible();
    await expect(pane).toHaveScreenshot("ap-f-changes-last-turn.png");
    await pane
      .getByRole("tab", {
        name: "Diff src/workbench/AgentChatPanes.tsx",
        exact: true,
      })
      .click();
    await expect(changes).toHaveCount(0);
    await expect(
      pane.getByRole("table", { name: "Unified file diff" }),
    ).toBeVisible();
    await page.keyboard.press("Control+Shift+G");
    await expect(changes).toBeVisible();
    await openAgentSnapshotDiff(page);
    await expect(changes).toHaveCount(0);
    await pane
      .getByRole("button", { name: "New browser tab", exact: true })
      .click();
    await expect(
      pane
        .locator(".browser-new-tab__tools")
        .getByRole("button", { name: "Changes", exact: true }),
    ).toBeVisible();
  });

  test("shows real edited resources and explains unsupported background tracking", async ({
    page,
  }) => {
    await openAgentScenario(page, "diff");
    await page
      .getByRole("button", { name: "Show chat resources", exact: true })
      .click();
    const resources = page.getByRole("dialog", {
      name: "Chat resources",
      exact: true,
    });
    await expect(
      resources.getByRole("button", { name: "Changes", exact: true }),
    ).toBeEnabled();
    await expect(resources.getByText(/recent files?/)).toBeVisible();
    await expect(
      resources.getByText("Background processes", { exact: true }),
    ).toBeVisible();
    await expect(
      resources.getByText("Process tracking is not exposed by this session.", {
        exact: true,
      }),
    ).toBeVisible();
    await expect(resources.getByText("Sources", { exact: true })).toBeVisible();
    await expect(resources).toHaveScreenshot("ap-f-resources.png");
    await resources
      .getByRole("button", { name: "Changes", exact: true })
      .click();
    await expect(resources).toHaveCount(0);
    await expect(
      page.getByRole("region", { name: "Chat changes", exact: true }),
    ).toBeVisible();
  });

  test("mixed-EOL File opens view-only without enabling save", async ({
    page,
  }) => {
    await openAgentScenario(page, "file");
    await page.evaluate(async () => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      const { getWorkbenchPorts } =
        await import("/src/ports/workbenchRuntime.ts");
      const state = useWorkbenchStore.getState();
      const tab = state.tabs.find((tab) => tab.kind === "file");
      if (!tab?.resource) throw new Error("File fixture is missing");
      const resource = tab.resource;
      getWorkbenchPorts().editor.readTextFile = async () => ({
        resource,
        content: "first\r\nsecond\n",
        byteLen: 14,
        lineEnding: "crlf",
        modifiedAt: null,
      });
      useWorkbenchStore.setState({
        activeEditor: {
          phase: "error",
          resource,
          message:
            "invalid request: mixed LF and CRLF files are not editable in this slice",
        },
      });
    });
    const view = page.getByRole("textbox", { name: /^View / });
    await expect(view).toHaveAttribute("readonly", "");
    await expect(view).toHaveValue("first\nsecond\n");
    await expect(page.getByText(/Mixed line endings/)).toBeVisible();
    await expect(
      page.getByLabel("Agent artifacts", { exact: true }),
    ).toHaveScreenshot("ap-f-mixed-eol-view-only.png");
  });
});
