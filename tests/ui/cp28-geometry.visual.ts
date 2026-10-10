import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
  settleTranscriptAtEnd,
} from "./shared/visualBrowserPrimitives";

test.use({ deviceScaleFactor: 1 });

async function installAnswers(page: Page, text: string, queued = false) {
  await page.evaluate(
    async ({ text, queued }) => {
      const [{ acpProjectionStoreFor }, { useWorkbenchStore }, ids] =
        await Promise.all([
          import("/src/app/acpProjectionRegistry.ts"),
          import("/src/app/store.ts"),
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
        const turn = current.turns[0];
        const answer = turn.items.find(
          (item) => item.kind === "text" && item.payload.role === "assistant",
        );
        if (!answer || answer.kind !== "text")
          throw new Error("Expected an answer");
        answer.payload.text = text;
        current.turns = [turn];
        current.projectionRevision = String(
          BigInt(current.projectionRevision) + 1n,
        ) as typeof current.projectionRevision;
        store.getState().applyAuthoritativePage(
          {
            projectId: ids.PROJECT_ID,
            projectGeneration: ids.GENERATION,
            chatId: current.chatId,
          },
          current,
        );
        if (queued) {
          const state = useWorkbenchStore.getState();
          const ownership = state.chatUiById[chatId];
          if (!ownership) throw new Error("Chat composer has no owner");
          const bindingId = state.harnesses.find(
            (harness) => harness.id === state.selectedHarnessId,
          )?.bindingId;
          if (!bindingId) throw new Error("Chat composer has no binding");
          useWorkbenchStore.setState({
            chatUiById: {
              ...state.chatUiById,
              [chatId]: {
                ...ownership,
                queuedPrompts: [
                  {
                    id: `cp28-queued-${chatId}`,
                    text: "Queued follow-up",
                    attachedContext: [],
                    bindingId,
                    modelId: null,
                    status: "blocked",
                  },
                ],
              },
            },
          });
        }
      }
    },
    { text, queued },
  );
}

async function expectColumnAlignment(pane: Locator) {
  const answer = await pane
    .locator(".acp-message--assistant")
    .last()
    .boundingBox();
  const composer = await pane.locator(".composer").boundingBox();
  expect(answer && composer).toBeTruthy();
  expect(composer!.x).toBeCloseTo(answer!.x, 0);
  expect(composer!.width).toBeCloseTo(answer!.width, 0);
  expect(
    await pane.evaluate(
      (element) => element.scrollWidth <= element.clientWidth + 1,
    ),
  ).toBe(true);
}

for (const width of [1888, 720, 480]) {
  for (const scenario of [
    "agent-single",
    "agent-dual",
    "agent-composer-goal",
  ] as const) {
    test(`CP-28 composer shares the transcript column with and without overflow at ${width}px in ${scenario}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 1096 });
      await openScenario(page, scenario, "agent");
      await settleChatProjections(page);
      const panes = page.locator(".agent-chat-pane");
      await expect(panes).toHaveCount(
        scenario === "agent-dual" && width === 1888 ? 2 : 1,
      );
      await installAnswers(page, "Short completed answer.");
      for (const pane of await panes.all()) {
        expect(
          await pane
            .locator(".message-list")
            .evaluate((element) => element.scrollHeight > element.clientHeight),
        ).toBe(false);
        await expectColumnAlignment(pane);
      }
      await installAnswers(
        page,
        "Long wrapped answer with retained content. ".repeat(400),
        true,
      );
      await settleTranscriptAtEnd(page);
      for (const pane of await panes.all()) {
        expect(
          await pane
            .locator(".message-list")
            .evaluate((element) => element.scrollHeight > element.clientHeight),
        ).toBe(true);
        await expect(
          pane.getByText("Queued follow-up", { exact: true }),
        ).toBeVisible();
        if (scenario === "agent-composer-goal")
          await expect(
            pane.getByText("Pursuing goal", { exact: true }),
          ).toBeVisible();
        await expectColumnAlignment(pane);
      }
    });
  }
}

test("CP-28 Worked chevron persists without hover and toggles with the keyboard; the first date sits above the prompt", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1888, height: 1096 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  const pane = page.locator(".agent-chat-pane").first();
  const disclosure = pane.locator("details.chat-transcript-work").first();
  const summary = disclosure.locator(":scope > summary");
  await page.mouse.move(0, 0);
  const indicator = await summary.evaluate((element) => {
    const style = getComputedStyle(element, "::after");
    return {
      display: style.display,
      opacity: style.opacity,
      border: style.borderLeftWidth,
    };
  });
  expect(indicator).toEqual({ display: "block", opacity: "1", border: "5px" });
  await summary.focus();
  await page.keyboard.press("Enter");
  await expect(disclosure).toHaveAttribute("open", "");
  await page.keyboard.press("Space");
  await expect(disclosure).not.toHaveAttribute("open");
  const dates = pane.locator(".chat-transcript-day");
  await expect(dates).toHaveCount(1);
  await expect(dates.first()).toContainText("Today");
  const date = await dates.first().boundingBox();
  const prompt = await pane.locator(".acp-message--user").first().boundingBox();
  expect(date && prompt).toBeTruthy();
  expect(date!.y + date!.height).toBeLessThanOrEqual(prompt!.y);
});

test("CP-28 Chat table text is flush with prose, list text hangs in one gutter, and native numbering is preserved", async ({
  page,
}) => {
  await page.setViewportSize({ width: 720, height: 1096 });
  await openScenario(page, "rich", "agent");
  await settleChatProjections(page);
  const listItem = "Wrapped list prose ".repeat(10);
  await installAnswers(
    page,
    [
      "Prose edge.",
      "",
      "| Database | Status |",
      "| --- | ---: |",
      "| SQLite | Ready |",
      "",
      `- ${listItem}`,
      "",
      ...[1, 9, 10, 100].flatMap((start) => [`${start}. ${listItem}`, ""]),
    ].join("\n"),
  );
  const markdown = page.locator(".acp-message--assistant .acp-markdown");
  const geometry = await markdown.evaluate((element) => {
    const textRects = (node: Element) => {
      const range = document.createRange();
      range.selectNodeContents(node);
      return Array.from(range.getClientRects())
        .filter((rect) => rect.width > 0)
        .map((rect) => ({ x: rect.x, y: rect.y }));
    };
    const header = element.querySelector("th")!;
    const body = element.querySelector("td")!;
    return {
      prose: textRects(element.querySelector("p")!)[0].x,
      header: textRects(header)[0].x,
      body: textRects(body)[0].x,
      headerSize: getComputedStyle(header).fontSize,
      bodySize: getComputedStyle(body).fontSize,
      headerWeight: getComputedStyle(header).fontWeight,
      bodyWeight: getComputedStyle(body).fontWeight,
      lists: Array.from(element.querySelectorAll("li")).map(textRects),
      starts: Array.from(element.querySelectorAll("ol")).map(
        (list) => list.start,
      ),
      alignment: getComputedStyle(element.querySelector("td:last-child")!)
        .textAlign,
    };
  });
  expect(geometry.header).toBeCloseTo(geometry.prose, 0);
  expect(geometry.body).toBeCloseTo(geometry.prose, 0);
  expect(geometry.headerSize).toBe(geometry.bodySize);
  expect(geometry.headerWeight).toBe("600");
  expect(geometry.bodyWeight).toBe("400");
  expect(geometry.starts).toEqual([1, 9, 10, 100]);
  expect(geometry.alignment).toBe("right");
  for (const lines of geometry.lists) {
    expect(new Set(lines.map((line) => line.y)).size).toBeGreaterThan(1);
    for (const line of lines)
      expect(line.x).toBeCloseTo(geometry.lists[0][0].x, 0);
  }
});

test("CP-28 code panels retain a visible boundary and Worked indicator in forced colors", async ({
  page,
}) => {
  await page.emulateMedia({ forcedColors: "active" });
  await page.setViewportSize({ width: 720, height: 1096 });
  await openScenario(page, "rich", "agent");
  await settleChatProjections(page);
  await installAnswers(page, "```python\nprint('retained')\n```");
  const code = page.getByRole("region", { name: "Python code" });
  await expect(code.getByRole("button", { name: "Wrap code" })).toBeVisible();
  await expect(code.getByRole("button", { name: "Copy code" })).toBeVisible();
  expect(
    await code.evaluate((element) => getComputedStyle(element).borderTopWidth),
  ).toBe("1px");
  expect(
    await page
      .locator(".chat-transcript-work > summary")
      .first()
      .evaluate((element) => getComputedStyle(element, "::after").display),
  ).toBe("block");
});
