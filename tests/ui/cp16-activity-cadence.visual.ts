import { expect, test } from "@playwright/test";

import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

test("CP-16 refs 08/13: primary activity rows keep a 32 px scale cadence and admitted order", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1936, height: 1048 });
  await openScenario(page, "waiting", "agent");
  await settleChatProjections(page);
  await expect(page.locator(".acp-tool-rollup").first()).toBeVisible();
  await page.evaluate(async () => {
    const [{ buildVisualScenario }, { acpProjectionStoreFor }, ids] =
      await Promise.all([
        import("/tests/ui/fixtures/visualScenarioShell.ts"),
        import("/src/app/acpProjectionRegistry.ts"),
        import("/tests/ui/fixtures/visualFixtureIds.ts"),
      ]);
    const chatPage = buildVisualScenario("waiting").pages[0];
    const turn = chatPage.turns[0];
    const basis = turn.items[0];
    turn.state = "running";
    turn.items = [
      basis,
      ...["First note", "Second note"].map((text, index) => ({
        ...basis,
        id: `cp16-prose-${index}` as typeof basis.id,
        ordinal: index === 0 ? 2 : 7,
        kind: "text" as const,
        payload: { role: "assistant" as const, text },
      })),
      ...["Ran one", "Ran two", "Ran three", "Ran four"].map(
        (label, index) => ({
          ...basis,
          id: `cp16-command-${index}` as typeof basis.id,
          ordinal: index + 3,
          kind: "toolCall" as const,
          toolState: "completed" as const,
          payload: {
            presentation: {
              kind: "execute" as const,
              cwd: null,
              exitCode: 0,
              outputTail: null,
              outputTruncated: false,
              label,
              summary: label,
              resources: [],
              activity:
                index % 2 === 0
                  ? ("ordinary" as const)
                  : ("declaredTest" as const),
            },
            inputSummary: null,
          },
        }),
      ),
    ] as typeof turn.items;
    acpProjectionStoreFor(chatPage.chatId).getState().applyAuthoritativePage(
      {
        projectId: ids.PROJECT_ID,
        projectGeneration: ids.GENERATION,
        chatId: chatPage.chatId,
      },
      chatPage,
    );
  });
  const pane = page.locator(".agent-chat-pane");
  const work = pane.locator(".chat-transcript-work");
  await expect(
    work.locator(
      ".acp-message--assistant .acp-markdown, .acp-tool-rollup__label",
    ),
  ).toHaveText([
    "First note",
    "Ran 1 command",
    "Ran tests · 1 command",
    "Ran 1 command",
    "Ran tests · 1 command",
    "Second note",
  ]);
  const rows = work.locator(".acp-tool-rollup__label");
  const y = await rows.evaluateAll((elements) =>
    elements.map((element) => element.getBoundingClientRect().top),
  );
  expect(y).toHaveLength(4);
  for (let index = 1; index < y.length; index += 1) {
    expect(y[index] - y[index - 1]).toBeGreaterThanOrEqual(31);
    expect(y[index] - y[index - 1]).toBeLessThanOrEqual(33);
  }
  await expect(pane).toHaveScreenshot("cp16-activity-cadence.png");
});

test("CP-16 refs 01/06/07: observed subagents have coloured start and done glyphs", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1936, height: 1048 });
  await openScenario(page, "waiting", "agent");
  await settleChatProjections(page);
  await expect(page.locator(".acp-tool-rollup").first()).toBeVisible();
  await page.evaluate(async () => {
    const [{ buildVisualScenario }, { acpProjectionStoreFor }, ids] =
      await Promise.all([
        import("/tests/ui/fixtures/visualScenarioShell.ts"),
        import("/src/app/acpProjectionRegistry.ts"),
        import("/tests/ui/fixtures/visualFixtureIds.ts"),
      ]);
    const chatPage = buildVisualScenario("waiting").pages[0];
    const turn = chatPage.turns[0];
    const basis = turn.items[0];
    turn.items.push(
      {
        ...basis,
        id: "cp16-agent-working" as typeof basis.id,
        ordinal: 4,
        kind: "collaboration",
        lifecycle: "completed",
        payload: {
          activityId: null,
          label: "spawnAgent",
          summary: "",
          receiverLabels: ["Reviewer"],
          operation: "spawnAgent",
          agentKeys: ["reviewer-key"],
          agentStates: [["reviewer-key", "running"]],
        },
      } as typeof basis,
      {
        ...basis,
        id: "cp16-agent-group-start" as typeof basis.id,
        ordinal: 5,
        kind: "collaboration",
        lifecycle: "completed",
        payload: {
          activityId: null,
          label: "spawnAgent",
          summary: "",
          receiverLabels: ["Builder", "Tester"],
          operation: "spawnAgent",
          agentKeys: ["builder-key", "tester-key"],
          agentStates: [
            ["builder-key", "running"],
            ["tester-key", "running"],
          ],
        },
      } as typeof basis,
      {
        ...basis,
        id: "cp16-agent-finished" as typeof basis.id,
        ordinal: 6,
        kind: "collaboration",
        lifecycle: "completed",
        payload: {
          activityId: null,
          label: "wait",
          summary: "",
          receiverLabels: ["Builder", "Tester"],
          operation: "wait",
          agentKeys: ["builder-key", "tester-key"],
          agentStates: [
            ["builder-key", "completed"],
            ["tester-key", "completed"],
          ],
        },
      } as typeof basis,
    );
    acpProjectionStoreFor(chatPage.chatId).getState().applyAuthoritativePage(
      {
        projectId: ids.PROJECT_ID,
        projectGeneration: ids.GENERATION,
        chatId: chatPage.chatId,
      },
      chatPage,
    );
  });
  const pane = page.locator(".agent-chat-pane");
  await expect(pane.getByText("Reviewer started working")).toBeVisible();
  await expect(pane.getByText("Builder and Tester finished")).toBeVisible();
  await expect(pane.locator(".acp-subagent-row__glyph")).toHaveCount(5);
  await expect(pane.locator(".chat-transcript-work__groups")).toHaveScreenshot(
    "cp16-subagent-rows.png",
  );
  await page.getByRole("button", { name: "Show chat resources" }).click();
  const popover = page.getByRole("dialog", { name: "Chat resources" });
  await expect(popover.getByLabel("Recent subagents working: 1")).toBeVisible();
  await expect(popover.getByLabel("Recent subagents done: 2")).toBeVisible();
  await expect(popover.locator(".agent-task-header__agent-glyph")).toHaveCount(
    1,
  );
  await expect(popover).toHaveScreenshot("cp16-subagent-counts.png");
});
