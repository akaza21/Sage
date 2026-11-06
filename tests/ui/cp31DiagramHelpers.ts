import { expect, type Locator, type Page } from "@playwright/test";
import {
  settleChatProjections,
  settleRendering,
} from "./shared/visualBrowserPrimitives";

export async function installDiagrams(page: Page, source: string) {
  await expect(
    page.locator(".acp-chat-projection[data-chat-id]").first(),
  ).toBeVisible();
  await settleChatProjections(page);
  await page.evaluate(async (source) => {
    const [{ acpProjectionStoreFor }, ids] = await Promise.all([
      sageImport("/src/app/acpProjectionRegistry.ts"),
      sageImport("/tests/ui/fixtures/visualFixtureIds.ts"),
    ]);
    for (const projection of document.querySelectorAll<HTMLElement>(
      ".acp-chat-projection[data-chat-id]",
    )) {
      const chatId = projection.dataset.chatId;
      if (!chatId) throw new Error("Chat pane has no owner");
      const store = acpProjectionStoreFor(chatId);
      const current = structuredClone(store.getState().page);
      const turn = current?.turns.at(-1);
      if (!current || !turn) throw new Error("Expected settled chat turn");
      current.projectionRevision = String(
        BigInt(current.projectionRevision) + 1n,
      ) as typeof current.projectionRevision;
      const prompt = turn.items.find(
        (item) => item.kind === "text" && item.payload.role === "user",
      );
      if (!prompt || prompt.kind !== "text") throw new Error("Expected prompt");
      const answer = {
        ...prompt,
        id: `cp31-answer-${chatId}` as typeof prompt.id,
        ordinal: 2,
        lifecycle: "completed" as const,
        payload: {
          role: "assistant" as const,
          text: `\`\`\`mermaid\n${source}\n\`\`\``,
        },
      };
      turn.items = [prompt, answer];
      turn.activities = [];
      turn.state = "completed";
      turn.terminalAt = turn.updatedAt;
      store.getState().applyAuthoritativePage(
        {
          projectId: ids.PROJECT_ID,
          projectGeneration: ids.GENERATION,
          chatId: current.chatId,
        },
        current,
      );
    }
  }, source);
}

/**
 * The labelled diagram scroller is a keyboard stop exactly when it overflows:
 * it carries no tabindex, and the browser makes only an overflowing scroller
 * focusable.
 */
export async function expectScrollStop(
  page: Page,
  figure: Locator,
  overflowing: boolean,
) {
  const scroller = figure.locator(".acp-mermaid");
  await expect(scroller).not.toHaveAttribute("tabindex");
  await expect(scroller).toHaveAttribute("role", "region");
  await expect(scroller).toHaveAttribute("aria-label", /scroll area$/u);
  await scroller.evaluate((element) => {
    element.scrollLeft = 0;
    (document.activeElement as HTMLElement | null)?.blur();
    (element as HTMLElement).focus();
  });
  expect(
    await scroller.evaluate((element) => document.activeElement === element),
  ).toBe(overflowing);
  if (!overflowing) return;
  await settleRendering(page);
  // Native arrow scrolling can outlive its first positive offset. Finish it
  // before the next media-mode pass resets this same scroller to zero.
  // A wrapped promise acknowledges listener registration before input, even
  // when native scrolling finishes instantly. Dispose its handle on failure.
  const completion = await scroller.evaluateHandle((element) => ({
    finished: new Promise<void>((resolve) => {
      element.addEventListener("scrollend", () => resolve(), { once: true });
    }),
  }));
  try {
    await page.keyboard.press("ArrowRight");
    await expect
      .poll(() => scroller.evaluate((element) => element.scrollLeft))
      .toBeGreaterThan(0);
    await completion.evaluate(({ finished }) => finished);
  } finally {
    await completion.dispose();
  }
  await scroller.evaluate((element) => (element as HTMLElement).blur());
}
