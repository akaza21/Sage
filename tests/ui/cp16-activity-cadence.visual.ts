import { expect, test } from "@playwright/test";

import { openScenario } from "./shared/visualBrowserPrimitives";

test("CP-16 refs 08/13: primary activity rows keep a 32 px scale cadence and admitted order", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1936, height: 1048 });
  await openScenario(page, "waiting", "agent");
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
              activity: index % 2 === 0 ? "ordinary" as const : "declaredTest" as const,
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
    work.locator(".acp-message--assistant .acp-markdown, .acp-tool-rollup__label"),
  ).toHaveText([
    "First note",
    "Ran commands",
    "Ran tests",
    "Ran commands",
    "Ran tests",
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
