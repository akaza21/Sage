import { expect, test, type Page } from "@playwright/test";

import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

async function installSubagents(
  page: Page,
  kind: "children" | "tasks" | "pi" | "opencode" | "omp",
) {
  await openScenario(page, "waiting", "agent");
  await expect(page.locator(".acp-chat-projection").first()).toBeVisible();
  await settleChatProjections(page);
  await page.evaluate(async (mode) => {
    const [{ buildVisualScenario }, { acpProjectionStoreFor }, ids] =
      await Promise.all([
        import("/tests/ui/fixtures/visualScenarioShell.ts"),
        import("/src/app/acpProjectionRegistry.ts"),
        import("/tests/ui/fixtures/visualFixtureIds.ts"),
      ]);
    const chatPage = buildVisualScenario("waiting").pages[0];
    const turn = chatPage.turns[0];
    const basis = turn.items[0];
    turn.activities = [];
    turn.items =
      mode === "pi" || mode === "opencode" || mode === "omp"
        ? [
            basis,
            {
              ...basis,
              id: "cp32c-ordinary" as typeof basis.id,
              ordinal: 1,
              kind: mode === "omp" ? "thoughtSummary" : "toolCall",
              toolState: mode === "omp" ? null : "completed",
              payload:
                mode === "omp"
                  ? { summary: "Inspecting the project" }
                  : {
                      presentation:
                        mode === "opencode"
                          ? { kind: "read", label: "Read file", resources: [] }
                          : {
                              kind: "execute",
                              label: "Run checks",
                              summary: null,
                              cwd: null,
                              exitCode: 0,
                              outputTail: "Checks passed",
                              outputTruncated: false,
                              resources: [],
                              activity: "ordinary",
                            },
                      inputSummary: null,
                    },
            } as typeof basis,
          ]
        : Array.from(
            { length: mode === "children" ? 300 : 2 },
            (_, index) =>
              ({
                ...basis,
                id: `cp32c-child-${index}` as typeof basis.id,
                ordinal: index,
                lifecycle: "completed",
                kind: mode === "children" ? "collaboration" : "toolCall",
                toolState:
                  mode === "children"
                    ? null
                    : index
                      ? "completed"
                      : "inProgress",
                payload:
                  mode === "children"
                    ? {
                        activityId: null,
                        label: "spawnAgent",
                        summary: "",
                        receiverLabels: [
                          index === 299
                            ? "Reviewer"
                            : index % 2
                              ? ""
                              : `Observed agent ${index} with a long supplied label`,
                        ],
                        operation: "spawnAgent",
                        agentKeys: [`private-child-${index}`],
                        agentStates: [
                          [
                            `private-child-${index}`,
                            index % 2 ? "completed" : "running",
                          ],
                        ],
                      }
                    : {
                        presentation: {
                          kind: "subagent",
                          label: index ? "Review tests" : "Inspect layout",
                          activityId: null,
                          taskLifecycle: true,
                        },
                        inputSummary: null,
                      },
              }) as typeof basis,
          );
    chatPage.sessions = chatPage.sessions.map((session) => ({
      ...session,
      capabilities: {
        ...session.capabilities,
        childTranscriptRead: false,
        childConversationProjection: mode === "children",
        collaborationEventProjection: mode === "children",
      },
    }));
    acpProjectionStoreFor(chatPage.chatId).getState().applyAuthoritativePage(
      {
        projectId: ids.PROJECT_ID,
        projectGeneration: ids.GENERATION,
        chatId: chatPage.chatId,
      },
      chatPage,
    );
  }, kind);
}

for (const width of [1888, 720, 480]) {
  test(`CP-32c counts, bounded geometry and explicit keyboard selection at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 1048 });
    await installSubagents(page, "children");
    const trigger = page
      .getByRole("button", { name: "Show chat resources" })
      .first();
    await expect(page.locator(".subagent-surface")).toHaveCount(0);
    await expect(trigger).toHaveAttribute("aria-expanded", "false");
    await trigger.focus();
    await trigger.press("Enter");
    const panel = page.getByRole("dialog", { name: "Chat resources" });
    const region = panel.getByRole("region", { name: "Subagents" });
    await expect(
      region.getByLabel("Recent subagents working: 64"),
    ).toBeVisible();
    await expect(region.getByLabel("Recent subagents done: 64")).toBeVisible();
    const rows = region.getByRole("button");
    await expect(rows).toHaveCount(128);
    // AP-F4: the panel opens on its first entry, Changes, above Subagents.
    const first = panel.getByRole("button", { name: "Changes", exact: true });
    await expect(first).toBeFocused();
    await expect(page.locator(".subagent-surface")).toHaveCount(0);
    const geometry = await panel.evaluate((popover) => {
      const box = popover.getBoundingClientRect();
      const list = popover.querySelector<HTMLElement>(
        ".agent-task-header__agent-list",
      )!;
      return {
        left: box.left,
        right: box.right,
        top: box.top,
        bottom: box.bottom,
        listHeight: list.clientHeight,
        scrollHeight: list.scrollHeight,
        listWidth: list.clientWidth,
        scrollWidth: list.scrollWidth,
      };
    });
    expect(geometry.left).toBeGreaterThanOrEqual(8);
    expect(geometry.right).toBeLessThanOrEqual(width - 8);
    expect(geometry.top).toBeGreaterThanOrEqual(8);
    expect(geometry.bottom).toBeLessThanOrEqual(1040);
    expect(geometry.listHeight).toBeGreaterThan(0);
    expect(geometry.listHeight).toBeLessThanOrEqual(224);
    expect(geometry.scrollHeight).toBeGreaterThan(geometry.listHeight);
    expect(geometry.scrollWidth).toBeLessThanOrEqual(geometry.listWidth);
    await expect(region).not.toContainText("private-child");
    await rows.first().focus();
    await rows.first().press("Escape");
    await expect(panel).toHaveCount(0);
    await expect(trigger).toBeFocused();
    await trigger.press("Enter");
    await first.press("Shift+Tab");
    await expect(panel).toHaveCount(0);
    await expect(trigger).toBeFocused();
    await trigger.press("Enter");
    const reviewer = region.getByRole("button", {
      name: "Open subagent Reviewer",
      exact: true,
    });
    await reviewer.scrollIntoViewIfNeeded();
    await expect(reviewer).toBeVisible();
    await reviewer.focus();
    await reviewer.press("Enter");
    await expect(panel).toHaveCount(0);
    await expect(
      page
        .locator(".subagent-surface")
        .getByRole("heading", { name: "Reviewer" }),
    ).toBeVisible();
    await expect(
      page.getByRole("tab", { name: "Subagents", exact: true }),
    ).toHaveAttribute("aria-selected", "true");
  });

  test(`CP-32c Claude Task metadata has counts but no child control at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 1048 });
    await installSubagents(page, "tasks");
    await page
      .getByRole("button", { name: "Show chat resources" })
      .first()
      .click();
    const region = page.getByRole("region", { name: "Subagents" });
    await expect(region).toContainText("1 working");
    await expect(region).toContainText("1 done");
    await expect(
      region.getByText("Inspect layout", { exact: true }),
    ).toBeVisible();
    await expect(
      region.getByText("Review tests", { exact: true }),
    ).toBeVisible();
    await expect(region.getByRole("button")).toHaveCount(0);
    await expect(page.locator(".subagent-surface")).toHaveCount(0);
  });

  for (const harness of ["pi", "opencode", "omp"] as const) {
    test(`CP-32c ${harness} normalized activity without subagent capability shows no surface at ${width}px`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 1048 });
      await installSubagents(page, harness);
      await page
        .getByRole("button", { name: "Show chat resources" })
        .first()
        .click();
      await expect(
        page.getByRole("dialog", { name: "Chat resources" }),
      ).toBeVisible();
      await expect(page.getByRole("region", { name: "Subagents" })).toHaveCount(
        0,
      );
      await expect(page.locator(".subagent-surface")).toHaveCount(0);
    });
  }
}
