import {
  geometryAnswers,
  geometryOrderedStarts,
  orderedGeometryAnswer,
} from "./fixtures/markdownAnswerInputs";
import { expect, test, type Locator, type Page } from "./shared/test";
import {
  openScenario,
  settleChatProjections,
  settleTranscriptAtEnd,
} from "./shared/visualBrowserPrimitives";

test.use({ deviceScaleFactor: 1 });

async function installAnswers(page: Page, text: string, queued = false) {
  await expect(page.locator(".acp-chat-projection").first()).toBeVisible();
  await settleChatProjections(page);
  await page.evaluate(
    async ({ text, queued }) => {
      const [
        { acpProjectionStoreFor },
        { useWorkbenchStore },
        ids,
        { buildVisualScenario },
      ] = await Promise.all([
        sageImport("/src/app/acpProjectionRegistry.ts"),
        sageImport("/src/app/store.ts"),
        sageImport("/tests/ui/fixtures/visualFixtureIds.ts"),
        sageImport("/tests/ui/fixtures/visualScenarioShell.ts"),
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
        const findAnswer = (items: typeof turn.items) =>
          items.find(
            (item) => item.kind === "text" && item.payload.role === "assistant",
          );
        // agent-dual's second Chat is mid-turn with no answer yet; give it
        // the rich fixture's completed answer so every pane is measured.
        let answer = findAnswer(turn.items);
        if (!answer) {
          answer = findAnswer(
            buildVisualScenario("rich").pages[0].turns[0].items,
          );
          if (answer) turn.items.push(answer);
        }
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

async function expectComposerControlsContained(pane: Locator) {
  const composer = pane.locator(".composer");
  const paneBox = await pane.boundingBox();
  const composerBox = await composer.boundingBox();
  expect(paneBox && composerBox).toBeTruthy();
  expect(composerBox!.x).toBeGreaterThanOrEqual(paneBox!.x);
  expect(composerBox!.x + composerBox!.width).toBeLessThanOrEqual(
    paneBox!.x + paneBox!.width,
  );
  for (const control of [
    composer.getByRole("button", { name: "Harness", exact: true }),
    composer.locator("span.composer__model-trigger"),
    composer.locator(".send-button"),
  ]) {
    await expect(control).toBeVisible();
    const controlBox = await control.boundingBox();
    expect(controlBox).toBeTruthy();
    expect(controlBox!.x).toBeGreaterThanOrEqual(composerBox!.x);
    expect(controlBox!.x + controlBox!.width).toBeLessThanOrEqual(
      composerBox!.x + composerBox!.width,
    );
  }
  expect(
    await composer
      .locator(".composer__toolbar")
      .evaluate((element) => element.scrollWidth <= element.clientWidth),
  ).toBe(true);
}

async function expectModelLeadingEdgeContained(model: Locator) {
  const labelEdges = await model.evaluate((element) => {
    // The leading rendered child (the icon is display:none while the label
    // fits) owns the trigger’s leading edge.
    const label = [...element.children].find(
      (child) => child.getClientRects().length > 0,
    );
    if (!label) throw new Error("Model trigger has no label");
    const range = document.createRange();
    range.selectNodeContents(label);
    return {
      labelLeft: range.getBoundingClientRect().left,
      triggerLeft: element.getBoundingClientRect().left,
    };
  });
  expect(labelEdges.labelLeft).toBeGreaterThanOrEqual(labelEdges.triggerLeft);
}

async function expectColumnAlignment(pane: Locator) {
  const answer = await pane
    .locator(".acp-message--assistant")
    .last()
    .boundingBox();
  const composer = await pane.locator(".composer").boundingBox();
  const dock = await pane.locator(".chat-dock").boundingBox();
  expect(answer && composer && dock).toBeTruthy();
  expect(dock!.x).toBeCloseTo(answer!.x, 0);
  expect(dock!.width).toBeCloseTo(answer!.width, 0);
  expect(composer!.x).toBe(dock!.x + 1);
  expect(composer!.width).toBe(dock!.width - 2);
  expect(
    await pane.evaluate(
      (element) => element.scrollWidth <= element.clientWidth + 1,
    ),
  ).toBe(true);
  await expectComposerControlsContained(pane);
}

for (const paneWidth of [360, 384]) {
  test(`CP-28 compact composer model label keeps its leading edge and toolbar controls inside a ${paneWidth}px resource Chat pane`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "file", "agent");
    await settleChatProjections(page);
    await expect(
      page.getByLabel("Agent artifacts", { exact: true }),
    ).toBeVisible();
    await page.addStyleTag({
      content: `.agent-chat-panes { grid-template-columns: ${paneWidth}px; }`,
    });
    const pane = page.locator(".agent-chat-pane").first();
    const paneBox = await pane.boundingBox();
    expect(paneBox).toBeTruthy();
    expect(paneBox!.width).toBeCloseTo(paneWidth, 0);
    const composer = pane.locator(".composer");
    // A singleton model is a passive identity label, not a picker (#98).
    const model = composer.locator("span.composer__model-trigger");
    await expect(model).toHaveAccessibleName("Model");
    await expect(
      composer.getByRole("button", { name: "Model", exact: true }),
    ).toHaveCount(0);
    // Wave 5 (Codex ref 36): a composer this wide keeps the model label and
    // truncates it; icon-only compaction starts at 320px.
    const label = model.locator(".composer__model-label");
    await expect(label).toBeVisible();
    const compactBox = await model.boundingBox();
    expect(compactBox!.width).toBeGreaterThanOrEqual(24);
    expect(compactBox!.height).toBeGreaterThanOrEqual(24);
    await expectModelLeadingEdgeContained(model);
    await expectComposerControlsContained(pane);
    await label.evaluate((element) => {
      element.textContent = "GPT-5 with an extended reasoning model name";
    });
    await expect(label).toBeVisible();
    expect(
      await label.evaluate(
        (element) =>
          element.scrollWidth > element.clientWidth &&
          getComputedStyle(element).textOverflow === "ellipsis",
      ),
    ).toBe(true);
    await expectModelLeadingEdgeContained(model);
    await expectComposerControlsContained(pane);
  });
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
      await expect(panes).toHaveCount(1);
      await installAnswers(page, geometryAnswers.short);
      for (const pane of await panes.all()) {
        expect(
          await pane
            .locator(".message-list")
            .evaluate((element) => element.scrollHeight > element.clientHeight),
        ).toBe(false);
        await expectColumnAlignment(pane);
      }
      await installAnswers(page, geometryAnswers.long, true);
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
  const chevron = summary.locator("svg.chat-transcript-work__chevron");
  await expect(chevron).toBeVisible();
  await expect(chevron).toHaveAttribute("aria-hidden", "true");
  const indicator = await chevron.evaluate((element) => {
    const style = getComputedStyle(element);
    return {
      display: style.display,
      opacity: style.opacity,
      fill: style.fill,
      stroke: style.stroke,
      color: style.color,
      transform: style.transform,
    };
  });
  expect(indicator.display).toBe("block");
  expect(indicator.opacity).toBe("1");
  expect(indicator.fill).toBe("none");
  expect(indicator.stroke).toBe(indicator.color);
  expect(indicator.transform).toBe("none");
  await summary.focus();
  await page.keyboard.press("Enter");
  await expect(disclosure).toHaveAttribute("open", "");
  await expect(chevron).toHaveCSS("transform", "matrix(0, 1, -1, 0, 0, 0)");
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

test("CP-28 answer Copy has a visible keyboard tooltip and announces feedback", async ({
  page,
}) => {
  await page.setViewportSize({ width: 720, height: 1096 });
  await openScenario(page, "rich", "agent");
  await settleChatProjections(page);
  await installAnswers(page, geometryAnswers.copy);
  await settleTranscriptAtEnd(page);
  await page.evaluate(() =>
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: async () => undefined },
    }),
  );
  const copy = page
    .locator(".chat-transcript-actions")
    .getByRole("button", { name: "Copy", exact: true });
  await page.keyboard.press("Tab");
  await copy.focus();
  // The accessible name flips to "Copied", so pin the same button node.
  const button = await copy.elementHandle();
  const tooltip = () =>
    button!.evaluate((element) => {
      const style = getComputedStyle(element, "::after");
      return {
        content: style.content,
        display: style.display,
        background: style.backgroundColor,
      };
    });
  await expect
    .poll(tooltip)
    .toMatchObject({ content: '"Copy"', display: "block" });
  expect((await tooltip()).background).not.toBe("rgba(0, 0, 0, 0)");
  await page.keyboard.press("Enter");
  await expect(page.locator('[data-acp-announcement="polite"]')).toHaveText(
    "Copied",
  );
  await expect
    .poll(tooltip)
    .toMatchObject({ content: '"Copied"', display: "block" });
});

test("CP-28 Chat table text is flush with prose, list text hangs in one gutter, and native numbering is preserved", async ({
  page,
}) => {
  await page.setViewportSize({ width: 720, height: 1096 });
  await openScenario(page, "rich", "agent");
  await settleChatProjections(page);
  await installAnswers(page, geometryAnswers.lists);
  const markdown = page.locator(".acp-message--assistant .acp-markdown");
  await expect(markdown.locator("th").first()).toHaveText("Database");
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
  for (const start of geometryOrderedStarts) {
    await installAnswers(page, orderedGeometryAnswer(start));
    const marker = await markdown.evaluate((element) => {
      const item = element.querySelector("ol li")!;
      const markerStyle = getComputedStyle(item, "::marker");
      const measurement = document.createElement("span");
      measurement.textContent = `${element.querySelector("ol")!.start}.\u00a0`;
      measurement.style.cssText =
        "position: absolute; visibility: hidden; white-space: pre;";
      measurement.style.font = markerStyle.font;
      measurement.style.fontVariantNumeric = markerStyle.fontVariantNumeric;
      item.append(measurement);
      const markerWidth = measurement.getBoundingClientRect().width;
      measurement.remove();
      return {
        x: item.getBoundingClientRect().left - markerWidth,
        width: markerWidth,
        proseLeft: element.querySelector("p")!.getBoundingClientRect().left,
      };
    });
    expect(marker.width).toBeGreaterThan(0);
    expect(marker.x, `native marker for ${start}`).toBeGreaterThanOrEqual(
      marker.proseLeft,
    );
  }
});

test("CP-28 forced colors keep Canvas code panels, highlighted pressed controls and a stroked keyboard chevron", async ({
  page,
}) => {
  await page.emulateMedia({ forcedColors: "active" });
  await page.setViewportSize({ width: 720, height: 1096 });
  // agent-single's first turn has activity, so its Worked chevron renders;
  // the rich fixture's activity-free turn has no Worked row since slice 4.
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await installAnswers(page, geometryAnswers.code);
  const code = page.getByRole("region", { name: "Python code" });
  await expect(code.getByRole("button", { name: "Wrap code" })).toBeVisible();
  await expect(code.getByRole("button", { name: "Copy code" })).toBeVisible();
  expect(
    await code.evaluate((element) => getComputedStyle(element).borderTopWidth),
  ).toBe("1px");
  const colors = await code.evaluate((element) => {
    const canvas = document.createElement("span");
    canvas.style.backgroundColor = "Canvas";
    element.append(canvas);
    const expected = getComputedStyle(canvas).backgroundColor;
    canvas.remove();
    return {
      panel: getComputedStyle(element).backgroundColor,
      canvas: expected,
    };
  });
  expect(colors.panel).toBe(colors.canvas);
  const wrap = code.getByRole("button", { name: "Wrap code" });
  await wrap.focus();
  await page.keyboard.press("Enter");
  await expect(wrap).toHaveAttribute("aria-pressed", "true");
  await wrap.evaluate((element) => element.blur());
  await page.mouse.move(0, 0);
  const actionColors = await wrap.evaluate((element) => {
    const swatch = document.createElement("span");
    swatch.style.backgroundColor = "Highlight";
    swatch.style.color = "HighlightText";
    element.append(swatch);
    const expected = getComputedStyle(swatch);
    const actual = getComputedStyle(element);
    const colors = {
      actualBackground: actual.backgroundColor,
      expectedBackground: expected.backgroundColor,
      actualColor: actual.color,
      expectedColor: expected.color,
    };
    swatch.remove();
    return colors;
  });
  expect(actionColors.actualBackground).toBe(actionColors.expectedBackground);
  expect(actionColors.actualColor).toBe(actionColors.expectedColor);
  const chevron = page.locator(".chat-transcript-work__chevron").first();
  await expect(chevron).toBeVisible();
  await expect(chevron).toHaveAttribute("fill", "none");
  await expect(chevron).toHaveAttribute("stroke", "currentColor");
  await expect(chevron.locator("path")).toHaveAttribute("d", "m9 18 6-6-6-6");
  const glyph = await chevron.evaluate((element) => {
    const style = getComputedStyle(element);
    return { fill: style.fill, stroke: style.stroke, color: style.color };
  });
  expect(glyph.fill).toBe("none");
  expect(glyph.stroke).toBe(glyph.color);
  const summary = chevron.locator("..");
  await summary.focus();
  await page.keyboard.press("Enter");
  await expect(chevron).toHaveCSS("transform", "matrix(0, 1, -1, 0, 0, 0)");
});
