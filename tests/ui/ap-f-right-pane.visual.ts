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
      const { createInjectedEditorWatchDelivery } =
        await import("/src/domain/editorWatch.ts");
      const state = useWorkbenchStore.getState();
      const tab = state.tabs.find((tab) => tab.kind === "file");
      if (!tab?.resource) throw new Error("File fixture is missing");
      const resource = tab.resource;
      let content = "first\r\nsecond\n";
      getWorkbenchPorts().editor.readTextFile = async () => ({
        resource,
        content,
        byteLen: content.length,
        lineEnding: "crlf",
        modifiedAt: null,
      });
      getWorkbenchPorts().editor.watchEditorDocument = async () => {
        const injected = createInjectedEditorWatchDelivery();
        const receive = (event: Event) => {
          const notice = (
            event as CustomEvent<{
              kind: "changed" | "deleted";
              content?: string;
            }>
          ).detail;
          if (notice.content !== undefined) content = notice.content;
          injected.emit({ resource, generation: 1, kind: notice.kind });
        };
        window.addEventListener("ap-f-file-notice", receive);
        return {
          resource,
          delivery: injected.delivery,
          cancel: async () => {
            window.removeEventListener("ap-f-file-notice", receive);
            injected.close();
          },
        };
      };
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
    await page.evaluate(() =>
      window.dispatchEvent(
        new CustomEvent("ap-f-file-notice", {
          detail: { kind: "changed", content: "rewritten\r\ntext\n" },
        }),
      ),
    );
    await expect(view).toHaveValue("rewritten\ntext\n");
    await page.evaluate(() =>
      window.dispatchEvent(
        new CustomEvent("ap-f-file-notice", { detail: { kind: "deleted" } }),
      ),
    );
    await expect(view).toHaveCount(0);
    await expect(page.getByRole("alert")).toContainText(
      "This file was deleted outside Sage.",
    );
    await page
      .getByRole("button", { name: "Refresh file", exact: true })
      .click();
    await expect(view).toHaveValue("rewritten\ntext\n");
  });

  test("keeps resource subagents keyboard reachable and transcript names navigate out of Changes", async ({
    page,
  }) => {
    await openAgentScenario(page, "agent-single");
    await page.evaluate(async () => {
      const { installPopoverSubagents } =
        await import("/tests/ui/fixtures/popoverSubagentsFixture.ts");
      installPopoverSubagents("children");
    });
    const trigger = page.getByRole("button", {
      name: "Show chat resources",
      exact: true,
    });
    await trigger.click();
    const resources = page.getByRole("dialog", {
      name: "Chat resources",
      exact: true,
    });
    await expect(
      resources.getByRole("button", { name: "Changes", exact: true }),
    ).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(
      resources.getByRole("button", {
        name: "Open subagent Reviewer",
        exact: true,
      }),
    ).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(trigger).toBeFocused();
    await page.keyboard.press("Control+Shift+G");
    const changes = page.getByRole("region", {
      name: "Chat changes",
      exact: true,
    });
    await expect(changes).toBeVisible();
    await page
      .locator(".agent-chat-pane")
      .getByRole("button", { name: "Open subagent Reviewer", exact: true })
      .click();
    await expect(changes).toHaveCount(0);
    const pane = page.getByLabel("Agent artifacts", { exact: true });
    await expect(
      pane.getByRole("heading", { name: "Reviewer", exact: true }),
    ).toBeVisible();
    await expect(
      pane.getByRole("tab", { name: "Subagents", exact: true }),
    ).toHaveAttribute("aria-selected", "true");
  });
});
