import { expect, test } from "@playwright/test";

import { openScenario } from "./shared/visualBrowserPrimitives";

test("CP-17(d) refs 07/13/17: Subagent row opens a bounded right-pane tab at 560 px", async ({
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
    turn.items.push({
      ...basis,
      id: "cp17-subagent-spawn" as typeof basis.id,
      ordinal: 4,
      kind: "collaboration",
      lifecycle: "completed",
      payload: {
        activityId: null,
        label: "spawnAgent",
        summary: "",
        receiverLabels: ["Reviewer"],
        operation: "spawnAgent",
        agentKeys: ["private-child-key"],
        agentStates: [["private-child-key", "running"]],
      },
    } as typeof basis);
    acpProjectionStoreFor(chatPage.chatId).getState().applyAuthoritativePage(
      {
        projectId: ids.PROJECT_ID,
        projectGeneration: ids.GENERATION,
        chatId: chatPage.chatId,
      },
      chatPage,
    );
  });
  await page.addStyleTag({
    content:
      ".agent-chat-panes { grid-template-columns: 560px !important; justify-content: center; }",
  });
  const pane = page.locator(".agent-chat-pane");
  await expect(pane).toHaveCSS("width", "560px");
  await pane.getByRole("button", { name: /Reviewer started working/ }).click();
  const rightPane = page.getByRole("complementary", {
    name: "Agent artifacts",
  });
  await expect(
    rightPane.getByRole("tab", { name: "Subagent" }),
  ).toHaveAttribute("aria-selected", "true");
  await expect(
    rightPane.getByRole("heading", { name: "Reviewer" }),
  ).toBeVisible();
  await expect(
    rightPane.getByText(
      "Child transcript is unavailable. Try reopening this view.",
      { exact: true },
    ),
  ).toBeVisible();
  const canvasBox = await rightPane
    .locator(".agent-artifact-pane__canvas")
    .boundingBox();
  const copyBox = await rightPane
    .getByText("Child transcript is unavailable. Try reopening this view.", {
      exact: true,
    })
    .boundingBox();
  expect(canvasBox && copyBox && copyBox.x >= canvasBox.x + 12).toBeTruthy();
  await expect(rightPane).not.toContainText("private-child-key");
  await expect(page).toHaveScreenshot("cp17-subagent-560.png");
});
