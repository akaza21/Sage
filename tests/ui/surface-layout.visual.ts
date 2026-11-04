import { expect, test } from "@playwright/test";
import {
  waitForAnimations,
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

test("dock owns clipping and keeps a block-end inset at every viewport", async ({
  page,
}) => {
  for (const viewport of [
    { width: 800, height: 600 },
    { width: 1280, height: 800 },
    { width: 1872, height: 1080 },
  ]) {
    await page.setViewportSize(viewport);
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    const geometry = await page.locator("form.composer").evaluate((form) => {
      const chat = form.closest(".chat-surface")!;
      const body = form.querySelector(".composer__body")!;
      const css = getComputedStyle(form.closest(".chat-dock")!);
      return {
        clip: css.overflow,
        inset:
          chat.getBoundingClientRect().bottom -
          form.closest(".chat-dock")!.getBoundingClientRect().bottom,
        expectedInset: Number.parseFloat(
          getComputedStyle(chat).getPropertyValue(
            "--chat-column-inset-block-end",
          ),
        ),
        inner: css.getPropertyValue("--surface-radius-inner").trim(),
        bodyCorners: [
          getComputedStyle(body).borderTopLeftRadius,
          getComputedStyle(body).borderTopRightRadius,
        ],
      };
    });
    expect.soft(geometry.clip).toBe("clip");
    expect.soft(geometry.inset).toBeGreaterThan(0);
    expect.soft(geometry.inset).toBeCloseTo(geometry.expectedInset, 1);
    expect.soft(geometry.inner).not.toBe("");
    expect.soft(geometry.bodyCorners).toEqual(["0px", "0px"]);
  }
});

test("transcript owns one responsive reading column with nonnegative rhythm", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1872, height: 1080 });
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  for (const measure of ["40ch", "60ch"]) {
    await page
      .locator(".workbench")
      .evaluate(
        (element, value) =>
          (element as HTMLElement).style.setProperty(
            "--reading-measure",
            value,
          ),
        measure,
      );
    for (const actions of await page.locator(".chat-transcript-actions").all())
      await waitForAnimations(actions);
    const geometry = await page
      .locator(".message-list__content")
      .evaluate((column) => {
        const box = column.getBoundingClientRect();
        const nodes = [
          ...column.querySelectorAll(
            ".acp-turn__items, .acp-turn__header, .acp-message--assistant, .chat-transcript-actions, .chat-fork-divider",
          ),
        ];
        return {
          measure: getComputedStyle(column)
            .getPropertyValue("--reading-measure")
            .trim(),
          edges: nodes.map((node) => [
            node.getBoundingClientRect().left - box.left,
            box.right - node.getBoundingClientRect().right,
          ]),
          margins: nodes.map((node) =>
            Number.parseFloat(getComputedStyle(node).marginTop),
          ),
        };
      });
    expect.soft(geometry.measure).toBe(measure);
    expect(geometry.edges.length).toBeGreaterThan(0);
    for (const edges of geometry.edges)
      for (const edge of edges)
        expect.soft(Math.abs(edge)).toBeLessThanOrEqual(1);
    for (const margin of geometry.margins)
      expect.soft(margin).toBeGreaterThanOrEqual(0);
  }
});

test("settings loading and persistence errors stay in the content flow", async ({
  page,
}) => {
  await openScenario(page, "editor", "editor");
  await page.keyboard.press("Control+,");
  await expect(page.locator(".settings-surface")).toBeVisible();
  await page.evaluate(async () => {
    const { useEditorSettingsStore } =
      await import("/src/app/editorSettingsStore.ts");
    useEditorSettingsStore.setState({
      persistencePhase: "loading",
      persistenceMessage: "Settings could not be saved",
    });
  });
  for (const width of [1280, 480]) {
    await page.setViewportSize({ width, height: 800 });
    const headerGap = await page
      .locator(".settings-surface__nav")
      .evaluate((nav) => ({
        actual:
          nav.getBoundingClientRect().top -
          nav.previousElementSibling!.getBoundingClientRect().bottom,
        expected: Number.parseFloat(getComputedStyle(nav.parentElement!).gap),
      }));
    expect(headerGap.actual).toBeCloseTo(headerGap.expected, 1);
    for (const selector of [
      ".settings-surface__loading",
      ".settings-surface__error",
    ]) {
      const status = page.locator(selector);
      await expect(status).toBeVisible();
      const geometry = await status.evaluate((element) => ({
        margin: Number.parseFloat(getComputedStyle(element).marginTop),
        gap:
          element.getBoundingClientRect().top -
          element.previousElementSibling!.getBoundingClientRect().bottom,
        overflow: element.scrollWidth - element.clientWidth,
      }));
      expect.soft(geometry.margin).toBeGreaterThanOrEqual(0);
      expect.soft(geometry.gap).toBeGreaterThanOrEqual(0);
      expect.soft(geometry.overflow).toBeLessThanOrEqual(1);
    }
  }
});

test("project picker target fits its chip without bleeding into adjacent controls", async ({
  page,
}) => {
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  const chip = page.locator("form.composer .project-scope.select-control");
  await expect(chip).toBeVisible();
  await chip.evaluate((element) =>
    (element as HTMLElement).style.setProperty(
      "--border-width",
      "var(--space-1)",
    ),
  );
  const geometry = await chip.evaluate((element) => {
    const outer = element.getBoundingClientRect();
    const picker = element
      .querySelector(".searchable-picker")!
      .getBoundingClientRect();
    const trigger = element
      .querySelector(".searchable-picker__trigger")!
      .getBoundingClientRect();
    return {
      border:
        Number.parseFloat(getComputedStyle(element).borderLeftWidth) +
        Number.parseFloat(
          getComputedStyle(
            element.querySelector(".searchable-picker__trigger")!,
          ).borderLeftWidth,
        ),
      borderToken: Number.parseFloat(
        getComputedStyle(element).getPropertyValue("--border-width"),
      ),
      edges: [
        picker.left - outer.left,
        outer.right - picker.right,
        trigger.left - outer.left,
        outer.right - trigger.right,
      ],
      target: trigger.height,
    };
  });
  expect(geometry.edges).toEqual([0, 0, 0, 0]);
  expect(geometry.border).toBe(geometry.borderToken);
  expect(geometry.target).toBeGreaterThanOrEqual(28);
  await page.evaluate(async () => {
    const { acpProjectionStoreFor } =
      await import("/src/app/acpProjectionRegistry.ts");
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    const projection = acpProjectionStoreFor(
      useWorkbenchStore.getState().activeTabId,
    );
    const { page: current, scope } = projection.getState();
    if (!current || !scope) throw new Error("Expected a loaded Chat");
    projection.getState().applyAuthoritativePage(scope, {
      ...current,
      turns: [],
      nextCursor: null,
    });
  });
  const caption = page.locator(".chat-home .searchable-picker__trigger");
  await expect(caption).toBeVisible();
  await expect(caption).toHaveCSS("box-shadow", "none");
  await expect(caption).toHaveCSS("backdrop-filter", "none");
  await expect(caption).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
});

test("queued messages and the edit disclosure share the dock and never overlap input", async ({
  page,
}) => {
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await page.evaluate(async () => {
    const { useWorkbenchStore } = await import("/src/app/store.ts");
    useWorkbenchStore.setState((state) => ({
      chatUiById: {
        ...state.chatUiById,
        [state.activeTabId!]: {
          ...state.chatUiById[state.activeTabId!],
          queuedPrompts: Array.from({ length: 8 }, (_, index) => ({
            id: `queue-${index}`,
            text: `Follow-up ${index}`,
            attachedContext: [],
            bindingId: "fixture",
            modelId: null,
            status: "blocked" as const,
          })),
        },
      },
    }));
  });
  await expect(
    page.getByLabel("Queued messages", { exact: true }),
  ).toBeVisible();
  const disclosure = page
    .getByRole("region", { name: "Files edited in this chat" })
    .getByRole("button", { name: /files? changed/ });
  await disclosure.click();
  await expect(disclosure).toHaveAttribute("aria-expanded", "true");
  await page
    .getByRole("textbox", { name: "Message agent" })
    .fill("Keep this draft");
  for (const viewport of [
    { width: 1280, height: 800 },
    { width: 800, height: 600 },
  ]) {
    await page.setViewportSize(viewport);
    const geometry = await page.locator("form.composer").evaluate((form) => {
      const queue = form.querySelector(".composer__queued")!;
      const input = form.querySelector("textarea")!;
      const summary = form
        .closest(".chat-surface")!
        .querySelector(".chat-edit-summary-zone")!;
      const outer = form.getBoundingClientRect();
      const inputBox = input.getBoundingClientRect();
      const toolbar = form
        .querySelector(".composer__toolbar")!
        .getBoundingClientRect();
      const body = input.closest(".composer__body")!.getBoundingClientRect();
      return {
        inputBounds: [inputBox.top, inputBox.bottom],
        bodyBounds: [body.top, body.bottom],
        summaryScroll: summary.scrollHeight - summary.clientHeight,
        summaryOverflow: getComputedStyle(summary).overflowY,
        queueScroll: queue.scrollHeight - queue.clientHeight,
        inputVisible:
          inputBox.top >= body.top &&
          inputBox.bottom <= body.bottom &&
          inputBox.top >= outer.top &&
          inputBox.bottom <= outer.bottom,
        toolbarVisible:
          toolbar.top >= outer.top && toolbar.bottom <= outer.bottom,
        ownsSummary: form.closest(".chat-dock")!.contains(summary),
        queuePosition: getComputedStyle(queue).position,
        queueFill: getComputedStyle(queue.firstElementChild!).backgroundColor,
        inputGap:
          input.getBoundingClientRect().top -
          queue.getBoundingClientRect().bottom,
        bounded:
          form.closest(".chat-dock")!.getBoundingClientRect().height <=
          form.closest(".chat-surface")!.getBoundingClientRect().height * 0.6,
      };
    });
    expect.soft(geometry.ownsSummary).toBe(true);
    expect.soft(geometry.summaryOverflow).toBe("auto");
    expect.soft(geometry.inputVisible, JSON.stringify(geometry)).toBe(true);
    expect.soft(geometry.toolbarVisible).toBe(true);
    if (viewport.height === 600) {
      expect.soft(geometry.summaryScroll).toBeGreaterThan(0);
      expect.soft(geometry.queueScroll).toBeGreaterThan(0);
    }
    await expect(
      page.getByRole("textbox", { name: "Message agent" }),
    ).toHaveValue("Keep this draft");
    expect.soft(geometry.queuePosition).toBe("static");
    expect.soft(geometry.queueFill).toBe("rgba(0, 0, 0, 0)");
    expect.soft(geometry.inputGap).toBeGreaterThanOrEqual(0);
    expect.soft(geometry.bounded).toBe(true);
  }
});

test("Agent and Editor cards honor the shared surface paint and radius contract", async ({
  page,
}) => {
  for (const mode of ["agent", "editor"] as const) {
    await openScenario(
      page,
      mode === "agent" ? "agent-single" : "editor",
      mode,
    );
    if (
      mode === "agent" &&
      !(await page.locator(".agent-artifact-pane").isVisible())
    )
      await page
        .getByRole("button", { name: /Show files and preview/ })
        .click();
    const cards = page.locator(
      mode === "agent"
        ? ".agent-chat-center, .agent-artifact-pane"
        : ".center-region",
    );
    for (const card of await cards.all()) {
      const style = await card.evaluate((element) => {
        const node = element as HTMLElement;
        node.style.setProperty("--surface-fill", "rgb(12 34 56)");
        node.style.setProperty("--surface-radius", "var(--space-8)");
        node.style.setProperty("--border-width", "var(--space-1)");
        const inner = document.createElement("span");
        inner.style.paddingInlineStart = "var(--surface-radius-inner)";
        node.append(inner);
        const result = {
          fill: getComputedStyle(node).backgroundColor,
          radius: getComputedStyle(node).borderTopLeftRadius,
          inner: getComputedStyle(inner).paddingInlineStart,
        };
        inner.remove();
        return result;
      });
      expect(style).toEqual({
        fill: "rgb(12, 34, 56)",
        radius: "32px",
        inner: "28px",
      });
    }
  }
});
