import { expect, test, type Page } from "@playwright/test";

import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

type Child = { key: string; name: string; state: string };

/** Spawns one collaboration item per child into the waiting scenario. */
async function seedChildren(page: Page, children: Child[], finished: string[]) {
  await page.setViewportSize({ width: 1936, height: 1048 });
  await openScenario(page, "waiting", "agent");
  await settleChatProjections(page);
  const seeded = await page.evaluate(
    async ({ children, finished }) => {
      const [{ buildVisualScenario }, { acpProjectionStoreFor }, ids] =
        await Promise.all([
          import("/tests/ui/fixtures/visualScenarioShell.ts"),
          import("/src/app/acpProjectionRegistry.ts"),
          import("/tests/ui/fixtures/visualFixtureIds.ts"),
        ]);
      const chatPage = buildVisualScenario("waiting").pages[0];
      const turn = chatPage.turns[0];
      const basis = turn.items[0];
      const now = Date.now();
      children.forEach((child, index) =>
        turn.items.push({
          ...basis,
          id: `cp17-spawn-${child.key}` as typeof basis.id,
          ordinal: 4 + index,
          kind: "collaboration",
          lifecycle: "completed",
          createdAt: now - 25_000,
          updatedAt: now,
          payload: {
            activityId: null,
            label: "spawnAgent",
            summary: "",
            receiverLabels: [child.name],
            operation: "spawnAgent",
            agentKeys: [`private-${child.key}`],
            agentStates: [[`private-${child.key}`, child.state]],
          },
        } as typeof basis),
      );
      if (finished.length)
        turn.items.push({
          ...basis,
          id: "cp17-wait" as typeof basis.id,
          ordinal: 4 + children.length,
          kind: "collaboration",
          lifecycle: "completed",
          createdAt: now,
          updatedAt: now,
          payload: {
            activityId: null,
            label: "wait",
            summary: "",
            receiverLabels: finished.map(
              (key) => children.find((child) => child.key === key)!.name,
            ),
            operation: "wait",
            agentKeys: finished.map((key) => `private-${key}`),
            agentStates: finished.map((key) => [`private-${key}`, "completed"]),
          },
        } as typeof basis);
      return acpProjectionStoreFor(chatPage.chatId)
        .getState()
        .applyAuthoritativePage(
          {
            projectId: ids.PROJECT_ID,
            projectGeneration: ids.GENERATION,
            chatId: chatPage.chatId,
          },
          chatPage,
        );
    },
    { children, finished },
  );
  expect(seeded).toBe("accepted");
}

const counting = [
  { key: "math", name: "Count math", state: "running" },
  { key: "readme", name: "Count readme", state: "running" },
];

test("CP-17(d) refs 07/13/17: Subagent row opens a bounded right-pane tab at 560 px", async ({
  page,
}) => {
  await seedChildren(
    page,
    [{ key: "child-key", name: "Reviewer", state: "running" }],
    [],
  );
  await page.addStyleTag({
    content:
      ".agent-chat-panes { grid-template-columns: 560px !important; justify-content: center; }",
  });
  const pane = page.locator(".agent-chat-pane");
  await expect(pane).toHaveCSS("width", "560px");
  await pane.getByRole("button", { name: "Open subagent Reviewer" }).click();
  const rightPane = page.getByLabel("Agent artifacts", { exact: true });
  await expect(
    rightPane.getByRole("tab", { name: "Subagents" }),
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

test("Codex s4/f0485: hovering the second grouped name lights only that name", async ({
  page,
}) => {
  await seedChildren(page, counting, ["math", "readme"]);
  const row = page
    .locator(".acp-subagent-row")
    .filter({ hasText: "Count math and Count readme finished" });
  await expect(row).toBeVisible();
  await expect(row.locator(".acp-live-shimmer")).toHaveCount(0);
  const first = row.getByRole("button", { name: "Open subagent Count math" });
  const second = row.getByRole("button", {
    name: "Open subagent Count readme",
  });
  const resting = await second.evaluate((node) => getComputedStyle(node).color);
  await second.hover();
  await expect
    .poll(() => second.evaluate((node) => getComputedStyle(node).color))
    .not.toBe(resting);
  expect(await first.evaluate((node) => getComputedStyle(node).color)).toBe(
    resting,
  );
  expect(
    await second.evaluate((node) => getComputedStyle(node).textDecorationLine),
  ).toBe("none");
  await expect(row).toHaveScreenshot("subagents-grouped-hover.png");
});

test("Codex s4/f0244: the Subagents tab lists Active · 1 and Done · 1", async ({
  page,
}) => {
  await seedChildren(
    page,
    [
      { key: "math", name: "Count math", state: "completed" },
      { key: "readme", name: "Count readme", state: "running" },
    ],
    [],
  );
  // Codex s4: the list is reached from a child view's back arrow; the tab
  // exists only once a child has been opened.
  await page
    .getByRole("button", { name: "Open subagent Count readme" })
    .click();
  const rightPane = page.getByLabel("Agent artifacts", { exact: true });
  await expect(
    rightPane.getByRole("tab", { name: "Subagents" }),
  ).toHaveAttribute("aria-selected", "true");
  await rightPane.getByRole("button", { name: "Back to subagents" }).click();
  const active = rightPane.getByRole("region", { name: "Active subagents" });
  const done = rightPane.getByRole("region", { name: "Done subagents" });
  await expect(active.getByRole("heading")).toHaveText("Active · 1");
  await expect(done.getByRole("heading")).toHaveText("Done · 1");
  await expect(active).toContainText("Count readme");
  await expect(active).toContainText("Working");
  await expect(done).toContainText("Count math");
  await expect(done).toContainText("just now");
  await expect(rightPane).not.toContainText("private-");
  await expect(rightPane).toHaveScreenshot("subagents-tab-active-done.png");
});

test("Codex s4/f0487: clicking a grouped name opens that child's header", async ({
  page,
}) => {
  await seedChildren(page, counting, ["math", "readme"]);
  // The spawn row and the finished row both name the child; use the latter.
  await page
    .getByRole("status", {
      name: "Count math and Count readme finished",
      exact: true,
    })
    .getByRole("button", { name: "Open subagent Count readme" })
    .click();
  const rightPane = page.getByLabel("Agent artifacts", { exact: true });
  const header = rightPane.locator(".subagent-surface header");
  await expect(
    header.getByRole("button", { name: "Back to subagents" }),
  ).toBeVisible();
  await expect(
    header.getByRole("heading", { level: 1, name: "Count readme" }),
  ).toBeVisible();
  // The visual fixture's wire names no spawn model, so none is shown.
  await expect(header.locator(".subagent-surface__model")).toHaveCount(0);
  await expect(header).toContainText("Worked for 25s");
  // The browser fixture serves no child transcript, so the body shows the
  // unavailable notice; capture only the Codex s4/f0487 header row.
  await expect(header).toHaveScreenshot("subagent-child-header.png");
  await header.getByRole("button", { name: "Back to subagents" }).click();
  await expect(
    rightPane.getByRole("region", { name: "Done subagents" }),
  ).toContainText("Count readme");
});
