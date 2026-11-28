import { expect, test, type Page } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";
import {
  dragAnswerPhrase,
  openSelectionScenario,
} from "./shared/transcriptSelection";

// One column rule for the Agent chat: min(768px, pane - 32px), centred, with
// text a further 8px in. Before it, a narrow pane left chat text 4px from the
// left edge and 12px from the right (the scrollbar ate one side).
const WIDTHS = [1380, 1100, 900, 760, 640, 520, 420];
const MIN_TEXT_GUTTER = 24;
const MAX_COLUMN = 768;

type Edges = { left: number; right: number; width: number };

/** Per visible chat pane, gaps from its edges to each box's content edges. */
function measureGutters(page: Page) {
  return page.evaluate(() =>
    [...document.querySelectorAll<HTMLElement>(".agent-chat-pane")]
      .filter((pane) => pane.offsetParent !== null)
      .map((pane) => {
        const edge = pane.getBoundingClientRect();
        const gaps = (element: Element | null, content = false) => {
          if (!element) return null;
          const rect = element.getBoundingClientRect();
          const style = getComputedStyle(element);
          const inset = (side: "Left" | "Right") =>
            content
              ? parseFloat(style[`padding${side}`]) +
                parseFloat(style[`border${side}Width`])
              : 0;
          return {
            left: rect.left + inset("Left") - edge.left,
            right: edge.right - (rect.right - inset("Right")),
            width: rect.width,
          };
        };
        const all = (selector: string) =>
          [...pane.querySelectorAll(selector)].map((node) => gaps(node)!);
        return {
          pane: edge.width,
          transcriptColumn: gaps(pane.querySelector(".message-list__content")),
          dockColumn: gaps(pane.querySelector(".chat-dock-frame")),
          user: all(".acp-message--user, .message--user"),
          userText: [
            ...pane.querySelectorAll(".acp-message--user, .message--user"),
          ].map((node) => gaps(node, true)!),
          assistant: all(".acp-message--assistant, .message--assistant"),
          composerText: gaps(
            pane.querySelector("form.composer textarea"),
            true,
          ),
        };
      }),
  );
}

function expectCentredColumn(column: Edges | null, pane: number) {
  expect(column).not.toBeNull();
  expect(column!.width).toBeLessThanOrEqual(MAX_COLUMN);
  expect(Math.abs(column!.left - column!.right)).toBeLessThanOrEqual(1);
  expect(column!.left).toBeGreaterThanOrEqual(16 - 0.5);
  // Narrow panes give the column everything but the two 16px gutters.
  if (pane - 32 < MAX_COLUMN)
    expect(Math.abs(column!.width - (pane - 32))).toBeLessThanOrEqual(1);
}

function expectTextGutters(boxes: Edges[]) {
  for (const box of boxes) {
    expect(box.left).toBeGreaterThanOrEqual(MIN_TEXT_GUTTER - 0.5);
    expect(box.right).toBeGreaterThanOrEqual(MIN_TEXT_GUTTER - 0.5);
  }
}

async function openDiff(page: Page) {
  await page.evaluate(async () => {
    const path = "/tests/ui/fixtures/cp27bResourceFixture.ts";
    const fixture = await import(path);
    await fixture.installCp27bResource("diff");
  });
  const tab = page
    .getByLabel("Agent artifacts", { exact: true })
    .locator('.tab[data-resource-kind="diff"]')
    .getByRole("tab");
  await tab.click();
  await expect(tab).toHaveAttribute("aria-selected", "true");
}

for (const width of WIDTHS) {
  for (const rightPane of [false, true]) {
    test(`chat text keeps 24px symmetric gutters at ${width}px${rightPane ? " beside the right pane" : ""}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 900 });
      await openScenario(page, "agent-single", "agent");
      await settleChatProjections(page);
      if (rightPane) {
        await openDiff(page);
        // Below the inline floor the pane covers the chat; nothing to measure.
        if (await page.locator(".workbench__body--artifact-overlay").count()) {
          await expect(page.locator(".agent-chat-pane")).toBeHidden();
          return;
        }
      }
      await expect(page.locator(".agent-chat-pane")).toBeVisible();
      const [gutters] = await measureGutters(page);
      expectPaneGutters(gutters);
    });
  }
}

test("both side-by-side chats keep 24px symmetric gutters", async ({
  page,
}) => {
  await openSelectionScenario(page);
  await page.setViewportSize({ width: 1380, height: 900 });
  await dragAnswerPhrase(page);
  await page
    .getByRole("button", { name: "Ask in side chat", exact: true })
    .click();
  await expect(page.locator(".agent-chat-pane")).toHaveCount(2);
  const [parent, side] = await measureGutters(page);
  expectPaneGutters(parent);
  // The fresh side chat has no turns yet; its column and composer still obey.
  expectPaneGutters(side, false);
});

function expectPaneGutters(
  gutters: Awaited<ReturnType<typeof measureGutters>>[number],
  withTurns = true,
) {
  if (withTurns) {
    expect(gutters.user.length).toBeGreaterThan(0);
    expect(gutters.assistant.length).toBeGreaterThan(0);
  }
  expectCentredColumn(gutters.transcriptColumn, gutters.pane);
  expectCentredColumn(gutters.dockColumn, gutters.pane);
  expect(gutters.dockColumn!.width).toBeCloseTo(
    gutters.transcriptColumn!.width,
    0,
  );
  expectTextGutters(gutters.user);
  expectTextGutters(gutters.userText);
  expectTextGutters(gutters.assistant);
  expectTextGutters([gutters.composerText!]);
}
