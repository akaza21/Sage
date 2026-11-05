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
  // Codex s4/f0244: one row names both children while one still works.
  const row = page.locator(".acp-subagent-row");
  await expect(row).toHaveCount(1);
  await expect(row).toHaveText("Count math and Count readme started working");
  // Each child is its 14 px coloured shape, 8 px apart (f0485), not a letter.
  const glyphs = await row
    .locator(".acp-subagent-row__glyph")
    .evaluateAll((nodes) =>
      nodes.map((node) => {
        const box = node.getBoundingClientRect();
        return {
          text: node.textContent,
          width: box.width,
          height: box.height,
          left: box.left,
          right: box.right,
          clip: getComputedStyle(node).clipPath,
        };
      }),
    );
  expect(glyphs).toHaveLength(2);
  for (const glyph of glyphs) {
    expect([glyph.text, glyph.width, glyph.height]).toEqual(["", 14, 14]);
    expect(glyph.clip).toMatch(/^polygon\(/);
  }
  expect(glyphs[1].left - glyphs[0].right).toBe(8);
  expect(glyphs[0].clip).not.toBe(glyphs[1].clip);
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
  // The QA layout scales the resource pane with the viewport and preserves
  // the Chat reading column; the tab and list retain their reference styling.
  const chat = await page.locator(".agent-chat-panes").boundingBox();
  const pane = await rightPane.boundingBox();
  expect(pane!.width / (pane!.width + chat!.width)).toBeCloseTo(0.51, 2);
  // The label's text, not its stretched box, starts just after the person
  // icon (f0244), which is drawn in CSS rather than shipped as an SVG.
  const gap = await rightPane
    .getByRole("tab", { name: "Subagents" })
    .evaluate((tab) => {
      if (tab.querySelector("svg")) return Number.NaN;
      const icon = tab.querySelector(".tab__person")!.getBoundingClientRect();
      const range = document.createRange();
      range.selectNodeContents(tab.querySelector(".tab__basename")!);
      return range.getBoundingClientRect().left - icon.right;
    });
  expect(gap).toBeLessThanOrEqual(8);
  for (const avatar of await rightPane.locator(".subagent-avatar").all()) {
    const box = await avatar.boundingBox();
    expect([Math.round(box!.width), Math.round(box!.height)]).toEqual([20, 20]);
  }
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
  const avatar = await header.locator(".subagent-avatar").boundingBox();
  expect([Math.round(avatar!.width), Math.round(avatar!.height)]).toEqual([
    16, 16,
  ]);
  // The browser fixture serves no child transcript, so the body shows the
  // unavailable notice; capture only the Codex s4/f0487 header row.
  await expect(header).toHaveScreenshot("subagent-child-header.png");
  // A child transcript has no author row (f0485): its meta header is hidden.
  const meta = await rightPane
    .locator(".subagent-surface")
    .evaluate((surface) => {
      const transcript = document.createElement("div");
      transcript.className = "subagent-surface__transcript";
      transcript.innerHTML =
        '<article class="acp-message acp-message--assistant"><header class="acp-message__meta">Agent</header></article>';
      surface.append(transcript);
      const display = getComputedStyle(
        transcript.querySelector(".acp-message__meta")!,
      ).display;
      transcript.remove();
      return display;
    });
  expect(meta).toBe("none");
  await header.getByRole("button", { name: "Back to subagents" }).click();
  await expect(
    rightPane.getByRole("region", { name: "Done subagents" }),
  ).toContainText("Count readme");
});
