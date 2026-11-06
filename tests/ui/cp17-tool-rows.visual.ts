import { expect, test, type Page } from "./shared/test";

import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

type Spec = {
  state: "running" | "completed";
  tools: Array<{
    kind: "execute" | "mcp" | "webSearch";
    toolState: "completed" | "inProgress" | "failed";
    input?: string;
    output?: string;
    server?: string;
    tool?: string;
    afterText?: string;
  }>;
};

/** Replaces the waiting scenario's turn with the given tool rows. */
async function applyTools(page: Page, spec: Spec) {
  await page.evaluate(async (spec) => {
    const [{ buildVisualScenario }, { acpProjectionStoreFor }, ids] =
      await Promise.all([
        sageImport("/tests/ui/fixtures/visualScenarioShell.ts"),
        sageImport("/src/app/acpProjectionRegistry.ts"),
        sageImport("/tests/ui/fixtures/visualFixtureIds.ts"),
      ]);
    const scenario = buildVisualScenario("waiting");
    const chatPage = scenario.pages[0];
    const turn = chatPage.turns[0];
    const prompt = turn.items[0];
    // The waiting turn carries its own live search; drop it so the turn
    // holds exactly the rows this spec describes.
    turn.items = turn.items.filter((item) => item.kind !== "toolCall");
    turn.state = spec.state;
    turn.terminalAt =
      spec.state === "completed" ? turn.createdAt + 36_000 : null;
    let ordinal = 4;
    for (const [index, tool] of spec.tools.entries()) {
      const presentation =
        tool.kind === "execute"
          ? {
              kind: "execute",
              label: tool.input,
              summary: tool.input,
              cwd: null,
              exitCode: tool.toolState === "completed" ? 0 : null,
              outputTail: null,
              outputTruncated: false,
              resources: [],
              activity: "ordinary",
            }
          : tool.kind === "mcp"
            ? {
                kind: "mcp",
                label: tool.tool,
                serverLabel: tool.server,
                toolLabel: tool.tool,
                argumentsSummary: null,
                resultSummary: null,
              }
            : { kind: "webSearch", label: "Web search" };
      turn.items.push({
        ...prompt,
        id: `tool-row-${index}` as typeof prompt.id,
        ordinal: ordinal++,
        kind: "toolCall",
        lifecycle: tool.toolState === "inProgress" ? "streaming" : "completed",
        toolState: tool.toolState,
        terminalAt:
          tool.toolState === "inProgress" ? null : turn.createdAt + 1_000,
        payload: {
          presentation,
          inputSummary: tool.input ?? null,
          outputSummary: tool.output ?? null,
        },
      } as never);
      if (tool.afterText)
        turn.items.push({
          ...prompt,
          id: `tool-row-text-${index}` as typeof prompt.id,
          ordinal: ordinal++,
          payload: { role: "assistant", text: tool.afterText },
        } as never);
    }
    acpProjectionStoreFor(chatPage.chatId).getState().applyAuthoritativePage(
      {
        projectId: ids.PROJECT_ID,
        projectGeneration: ids.GENERATION,
        chatId: chatPage.chatId,
      },
      chatPage,
    );
  }, spec);
}

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 1936, height: 1048 });
  await openScenario(page, "waiting", "agent");
  await settleChatProjections(page);
});

test("CP-17 tool rows: one live row shimmers below the finished groups", async ({
  page,
}) => {
  await applyTools(page, {
    state: "running",
    tools: [
      {
        kind: "execute",
        toolState: "completed",
        input: "rg --files",
        afterText: "Now the build.",
      },
      { kind: "execute", toolState: "inProgress", input: "node build.js" },
    ],
  });
  const labels = page.locator(
    ".acp-tool-rollup > summary .acp-tool-rollup__label",
  );
  await expect(labels).toHaveText(["Ran rg --files", "Running node build.js"]);
  await expect(page.locator(".acp-tool-rollup .acp-live-shimmer")).toHaveCount(
    1,
  );
  await expect(labels.last()).toHaveClass(/acp-live-shimmer/);
  await expect(page.locator(".acp-tool-rollup .acp-tool-running")).toHaveCount(
    0,
  );
  await expect(
    page.locator(".acp-tool-rollup > summary .acp-state"),
  ).toHaveCount(0);
});

test("CP-17 tool rows: an integration group reads in Codex grammar and scrolls inside 220 px", async ({
  page,
}) => {
  const json = JSON.stringify(
    {
      hits: Array.from({ length: 12 }, (_, index) => ({
        url: `https://example.com/${index}`,
      })),
    },
    null,
    2,
  );
  await applyTools(page, {
    state: "completed",
    tools: [
      {
        kind: "mcp",
        toolState: "completed",
        server: "OpenAI Developer Docs",
        tool: "Search OpenAI Docs",
        output: json,
      },
      {
        kind: "webSearch",
        toolState: "completed",
        input: "node:test documentation",
      },
      {
        kind: "mcp",
        toolState: "failed",
        server: "node_repl",
        tool: "Run requested error probe",
        output: "sandbox probe",
      },
      {
        kind: "execute",
        toolState: "completed",
        input: "node -v",
        afterText: "Done.",
      },
    ],
  });
  await page.locator(".chat-transcript-work > summary").click();
  const group = page.locator(
    ".chat-transcript-work__groups > .acp-tool-rollup",
  );
  await expect(
    group.locator(":scope > summary .acp-tool-rollup__label"),
  ).toHaveText(
    "Used OpenAI Developer Docs, Search Service and node_repl integrations, ran a command",
  );
  // Worked reveals the group closed; it opens on its own click (s2/f0016).
  await expect(group).not.toHaveAttribute("open", "");
  await group.locator(":scope > summary").click();
  await expect(group).toHaveAttribute("open", "");
  // The open chevron turns down (s5/f0390).
  await expect(
    group.locator(":scope > summary .acp-tool-rollup__chevron"),
  ).toHaveCSS("transform", /matrix\(0, 1, -1, 0|matrix\(6\.1\d*e-17, 1, -1/);
  const members = group.locator(
    ":scope > .acp-tool-rollup__items > .acp-tool-rollup",
  );
  await expect(
    members.locator(":scope > summary .acp-tool-rollup__label"),
  ).toHaveText([
    "Search OpenAI Docs",
    "Web run",
    "Run requested error probe",
    "Ran node -v",
  ]);
  // Web and MCP rows use Codex's command glyph; a failed row stays muted.
  for (const index of [0, 1, 2])
    await expect(
      members.nth(index).locator(":scope > summary svg").first(),
    ).toHaveClass(/lucide-command/);
  const iconColor = (index: number) =>
    members
      .nth(index)
      .locator(":scope > summary svg")
      .first()
      .evaluate((node) => getComputedStyle(node).color);
  expect(await iconColor(2)).toBe(await iconColor(0));
  // Nested rows pitch 24 px (Codex about 22 px).
  const [firstBox, secondBox] = await Promise.all([
    members.nth(0).boundingBox(),
    members.nth(1).boundingBox(),
  ]);
  expect(secondBox!.y - firstBox!.y).toBe(24);
  // The web result is one plaintext block, with no query line (s5/f0405).
  await members.nth(1).locator(":scope > summary").click();
  await expect(
    members.nth(1).locator(".acp-tool-result .acp-code-block__language"),
  ).toHaveText(/plaintext/i);
  await expect(members.nth(1).locator("p")).toHaveCount(0);
  await members.nth(1).locator(":scope > summary").click();
  await members.nth(0).locator(":scope > summary").click();
  await expect(
    members.nth(0).locator(".acp-tool-result .acp-code-block"),
  ).toBeVisible();
  await members.nth(2).locator(":scope > summary").click();
  const error = members.nth(2).locator(".acp-tool-error");
  await expect(error).toHaveText("sandbox probe");
  await expect(error).toHaveCSS("border-top-color", "rgb(124, 33, 31)");
  const height = await group
    .locator(":scope > .acp-tool-rollup__items")
    .evaluate((node) => node.clientHeight);
  expect(height).toBeLessThanOrEqual(220);
  const result = await members
    .nth(0)
    .locator(".acp-tool-result pre")
    .evaluate((node) => node.clientHeight);
  expect(result).toBeLessThanOrEqual(150);
});
