import { expect, test } from "./shared/test";
import {
  openAgentScenario,
  openAgentSnapshotDiff,
} from "./resourceVisualHelpers";
import { settleChatProjections } from "./shared/visualBrowserPrimitives";
import { revealWorkedContent } from "./agentVisualHelpers";

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
    const scope = changes.getByRole("combobox", { name: "Changes scope" });
    await expect(scope).toHaveValue("lastTurn");
    // The fixture's edit belongs to the first turn; the last turn made none,
    // so Last turn says so, and Unstaged lists the project's Git change
    // without attributing it to this Chat. (The browser backend has no
    // scoped Git diff; the per-file Diff below shows the saved edit.)
    await expect(changes.getByText("No changes in this scope.")).toBeVisible();
    await scope.selectOption("unstaged");
    await expect(
      changes.getByText("Project Git changes · not attributed to this Chat"),
    ).toBeVisible();
    await expect(
      changes.getByText("src/workbench/AgentChatPanes.tsx", { exact: true }),
    ).toBeVisible();
    await scope.selectOption("lastTurn");
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
    // "+" opens a native browser tab, which the browser runtime cannot host;
    // the New tab tools page itself is covered by cp48-new-tab.visual.ts.
    await expect(
      pane.getByRole("button", { name: "New browser tab", exact: true }),
    ).toBeVisible();
  });

  test("gives a Diff filename room beside a New tab before truncating", async ({
    page,
  }) => {
    await openAgentScenario(page, "diff");
    await openAgentSnapshotDiff(page);
    await page.evaluate(async () => {
      const { useWorkbenchStore } = await sageImport("/src/app/store.ts");
      const state = useWorkbenchStore.getState();
      useWorkbenchStore.setState({
        tabs: [
          ...state.tabs,
          {
            id: "browser:new-tab",
            kind: "browser",
            title: "New tab",
            position: state.tabs.length,
            pinned: false,
            dirty: false,
          },
        ],
      });
    });
    const pane = page.getByLabel("Agent artifacts", { exact: true });
    const diff = pane.locator('.tab[data-resource-kind="diff"]');
    const browser = pane.locator('.tab[data-resource-kind="browser"]');
    await expect(browser).toBeVisible();
    const basenameFits = await diff
      .locator(".tab__basename")
      .evaluate((node) => node.scrollWidth <= node.clientWidth);
    expect(basenameFits).toBe(true);
    const [diffBox, browserBox] = await Promise.all([
      diff.boundingBox(),
      browser.boundingBox(),
    ]);
    expect(browserBox!.width).toBeLessThan(diffBox!.width);
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
      const { useWorkbenchStore } = await sageImport("/src/app/store.ts");
      const { getWorkbenchPorts } = await sageImport(
        "/src/ports/workbenchRuntime.ts",
      );
      const { createInjectedEditorWatchDelivery } = await sageImport(
        "/src/domain/editorWatch.ts",
      );
      const state = useWorkbenchStore.getState();
      // The shown Agent File is the active editor's resource; the scenario
      // also holds other File tabs.
      const resource = state.activeEditor?.resource;
      if (!resource) throw new Error("File fixture is missing");
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
      // Native document opening refuses mixed line endings; reload through
      // that refusal rather than writing the error state directly.
      getWorkbenchPorts().editor.openEditorDocument = async () => {
        throw Object.assign(new Error("Mixed line endings are view-only."), {
          kind: "mixedLineEndings",
        });
      };
      useWorkbenchStore.setState({ activeEditor: null });
      await useWorkbenchStore.getState().loadEditor(resource);
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
    await settleChatProjections(page);
    await page.evaluate(async () => {
      const { installPopoverSubagents } = await sageImport(
        "/tests/ui/fixtures/popoverSubagentsFixture.ts",
      );
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
    // The Subagents section loads on demand; Tab only reaches it once shown.
    await expect(
      resources.getByRole("region", { name: "Subagents" }).getByRole("button"),
    ).not.toHaveCount(0);
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
    // The completed Turn keeps its work under a closed Worked disclosure.
    const reviewer = page
      .locator(".agent-chat-pane")
      .getByRole("button", { name: "Open subagent Reviewer", exact: true });
    await revealWorkedContent(reviewer);
    await reviewer.click();
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
