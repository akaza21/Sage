import { expect, test, type Page } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

async function installTurn(page: Page, stopped: boolean) {
  await page.evaluate(async (stopped) => {
    const [{ acpProjectionStoreFor }, ids] = await Promise.all([
      import("/src/app/acpProjectionRegistry.ts"),
      import("/tests/ui/fixtures/visualFixtureIds.ts"),
    ]);
    for (const projection of document.querySelectorAll<HTMLElement>(
      ".acp-chat-projection[data-chat-id]",
    )) {
      const chatId = projection.dataset.chatId;
      if (!chatId) throw new Error("Chat pane has no owner");
      const store = acpProjectionStoreFor(chatId);
      const current = structuredClone(store.getState().page);
      if (!current) throw new Error("Chat projection is not settled");
      current.projectionRevision = String(
        BigInt(current.projectionRevision) + 1n,
      ) as typeof current.projectionRevision;
      const turn = current.turns.at(-1);
      if (!turn) throw new Error("Expected a turn");
      const prompt = turn.items.find(
        (item) => item.kind === "text" && item.payload.role === "user",
      );
      if (!prompt || prompt.kind !== "text") throw new Error("Expected prompt");
      const answer = turn.items.find(
        (item) => item.kind === "text" && item.payload.role === "assistant",
      ) ?? {
        ...prompt,
        id: `cp28-answer-${chatId}` as typeof prompt.id,
        ordinal: 2,
        payload: { role: "assistant" as const, text: "" },
      };
      if (answer.kind !== "text") throw new Error("Expected text answer");
      answer.payload.text =
        "Retained answer with a long token: " + "long-content-".repeat(100);
      answer.lifecycle = stopped ? "cancelled" : "streaming";
      turn.items = [prompt, answer];
      turn.activities = [];
      turn.state = stopped ? "interrupted" : "running";
      turn.terminalReason = stopped ? { kind: "cancelledByUser" } : undefined;
      turn.terminalAt = stopped ? turn.updatedAt : null;
      store.getState().applyAuthoritativePage(
        {
          projectId: ids.PROJECT_ID,
          projectGeneration: ids.GENERATION,
          chatId: current.chatId,
        },
        current,
      );
    }
  }, stopped);
}

test.use({ deviceScaleFactor: 1 });

/** The latest answer's row; earlier Turns keep their own (s2/f0149). */
const footerActions = ".message-list__content > .chat-transcript-actions";

for (const width of [1888, 720, 480]) {
  for (const scenario of ["agent-single", "agent-dual"] as const) {
    test(`CP-28 Stop preserves answer/action columns at ${width}px in ${scenario}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 1096 });
      await openScenario(page, scenario, "agent");
      await settleChatProjections(page);
      const panes = page.locator(".agent-chat-pane");
      // Below the two-pane floor only the focused Chat stays visible.
      await expect(panes).toHaveCount(1);
      await installTurn(page, false);
      const bounds = [];
      for (const pane of await panes.all()) {
        await expect(pane.locator(footerActions)).toHaveCount(0);
        const answer = pane.locator(".acp-message--assistant").last();
        await expect(answer).toBeVisible();
        bounds.push(await answer.boundingBox());
      }
      await installTurn(page, true);
      for (const [index, pane] of (await panes.all()).entries()) {
        await expect(
          pane
            .locator(".acp-turn")
            .last()
            .getByRole("status", { name: "Cancelled by you" }),
        ).toBeVisible();
        const answer = pane.locator(".acp-message--assistant").last();
        const before = bounds[index];
        const after = await answer.boundingBox();
        const actions = await pane.locator(footerActions).boundingBox();
        const header = await pane
          .locator(".acp-turn__header")
          .last()
          .boundingBox();
        expect(before && after && actions && header).toBeTruthy();
        expect(after!.x).toBeCloseTo(before!.x, 0);
        expect(after!.width).toBeCloseTo(before!.width, 0);
        expect(actions!.x).toBeCloseTo(after!.x, 0);
        expect(actions!.width).toBeCloseTo(after!.width, 0);
        expect(header!.x).toBeCloseTo(after!.x, 0);
        expect(header!.width).toBeCloseTo(after!.width, 0);
        await expect(
          pane
            .locator(".acp-turn")
            .last()
            .getByRole("status", { name: "Cancelled by you" }),
        ).toHaveCount(1);
        await expect(
          pane.getByText("Interrupted", { exact: true }),
        ).toHaveCount(0);
        expect(
          await pane.evaluate(
            (element) => element.scrollWidth <= element.clientWidth + 1,
          ),
        ).toBe(true);
      }
    });
  }
}
