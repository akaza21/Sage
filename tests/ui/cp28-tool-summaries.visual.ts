import { expect, test } from "@playwright/test";

import { revealWorkedContent } from "./agentVisualHelpers";
import {
  expectNoViewportOverflow,
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

test("CP-28 regular tool summary computes to muted 400 text", async ({
  page,
}) => {
  await page.setViewportSize({ width: 720, height: 880 });
  await openScenario(page, "agent-command-rollup", "agent");
  await settleChatProjections(page);
  const row = page.locator(".acp-tool-rollup");
  await revealWorkedContent(row);
  const label = row.locator(".acp-tool-rollup__label");
  await expect(label).toHaveText("Ran 2 commands");
  const typography = await label.evaluate((node) => {
    const element = node as HTMLElement;
    const style = getComputedStyle(element);
    const color = style.color;
    const fontWeight = style.fontWeight;
    const previous = element.style.color;
    element.style.color = "var(--color-text-muted)";
    const muted = getComputedStyle(element).color;
    element.style.color = previous;
    return { color, muted, fontWeight };
  });
  expect(typography.fontWeight).toBe("400");
  expect(typography.color).toBe(typography.muted);
});

for (const layout of [
  { width: 720, scenario: "agent-command-rollup" as const, panes: 1 },
  { width: 1888, scenario: "agent-dual" as const, panes: 2 },
]) {
  test(`CP-28 long native command stays one line at ${layout.width}px in ${layout.panes} pane(s)`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: layout.width, height: 1048 });
    await openScenario(page, layout.scenario, "agent");
    await settleChatProjections(page);
    await expect(page.locator(".agent-chat-pane")).toHaveCount(layout.panes);
    await page.evaluate(async () => {
      const [{ acpProjectionStoreFor }, { buildVisualScenario }] =
        await Promise.all([
          import("/src/app/acpProjectionRegistry.ts"),
          import("/tests/ui/fixtures/visualScenarioShell.ts"),
        ]);
      const template = buildVisualScenario(
        "agent-command-rollup",
      ).pages[0].turns[0].items.find((item) => item.kind === "toolCall");
      if (!template || template.kind !== "toolCall")
        throw new Error("command template missing");
      for (const projection of document.querySelectorAll<HTMLElement>(
        ".acp-chat-projection[data-chat-id]",
      )) {
        const store = acpProjectionStoreFor(projection.dataset.chatId as never);
        const { page: current, scope } = store.getState();
        if (!current || !scope) throw new Error("projection did not settle");
        const turn = current.turns[0];
        const command = {
          ...template,
          projectId: current.projectId,
          chatId: current.chatId,
          sessionId: turn.sessionId,
          turnId: turn.id,
          runtimeEpoch: turn.runtimeEpoch,
          ordinal: 1,
          toolState: "inProgress" as const,
          lifecycle: "streaming" as const,
          terminalAt: null,
          payload: {
            ...template.payload,
            inputSummary:
              "npm run check -- " + "very-long-argument ".repeat(100),
            presentation: { ...template.payload.presentation, summary: null },
          },
        };
        const disposition = store.getState().applyAuthoritativePage(scope, {
          ...current,
          turns: [
            { ...turn, state: "running", terminalAt: null, items: [command] },
          ],
        });
        if (disposition !== "accepted") throw new Error(disposition);
      }
    });
    const rows = page.locator('.acp-tool-rollup[data-activity-kind="execute"]');
    await expect(rows).toHaveCount(layout.panes);
    for (let index = 0; index < layout.panes; index += 1) {
      const row = rows.nth(index);
      const label = row.locator(".acp-tool-rollup__label");
      await expect(row).not.toHaveAttribute("open");
      await expect(label).toContainText("Running npm run check --");
      const geometry = await label.evaluate((node) => {
        const label = node as HTMLElement;
        const summary = label.closest("summary")!;
        const row = summary.closest("details")!;
        const style = getComputedStyle(label);
        return {
          textLength: label.textContent?.length,
          whiteSpace: style.whiteSpace,
          overflow: style.textOverflow,
          height: label.getBoundingClientRect().height,
          lineHeight: Number.parseFloat(style.lineHeight),
          ellipsized: label.scrollWidth > label.clientWidth,
          rowOverflow: row.scrollWidth - row.clientWidth,
          contained:
            label.getBoundingClientRect().right <=
            summary.getBoundingClientRect().right,
        };
      });
      expect(geometry.textLength).toBeLessThanOrEqual(520);
      expect(geometry.whiteSpace).toBe("nowrap");
      expect(geometry.overflow).toBe("ellipsis");
      expect(geometry.ellipsized).toBe(true);
      expect(geometry.height).toBeLessThanOrEqual(geometry.lineHeight + 1);
      expect(geometry.rowOverflow).toBeLessThanOrEqual(1);
      expect(geometry.contained).toBe(true);
      await row.locator("summary").click();
      const line = row.getByLabel("Command line");
      await expect(line).toBeVisible();
      expect((await line.textContent())?.length).toBeLessThanOrEqual(512);
    }
    await expectNoViewportOverflow(page);
  });
}
