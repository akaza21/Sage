import { expect, test, type Locator, type Page } from "@playwright/test";

import { AGENT_VISUAL_SCENARIOS } from "./fixtures/visualScenarioTypes";
import { VISUAL_SCENARIOS } from "./fixtures/visualScenarioCatalog";
import {
  expectNoViewportOverflow,
  openScenario,
  settleTranscriptAtEnd,
  takeCanonicalScreenshot,
} from "./shared/visualBrowserPrimitives";
import {
  expandChildActivityHierarchy,
  expectAgentImageContained,
  expectChildActivityContained,
  runAgentCanonicalTest,
} from "./agentVisualHelpers";

async function expectOpenPickerContained(
  page: Page,
  input: Locator,
  listbox: Locator,
) {
  const popup = input.locator(
    "xpath=ancestor::div[contains(concat(' ', normalize-space(@class), ' '), ' menu-popover ')][1]",
  );
  await expect(input).toBeFocused();
  await expect(popup).toBeVisible();
  await expect(listbox).toBeVisible();
  const [inputBox, listboxBox, popupBox, viewport] = await Promise.all([
    input.boundingBox(),
    listbox.boundingBox(),
    popup.boundingBox(),
    page.evaluate(() => ({
      height: window.innerHeight,
      width: window.innerWidth,
    })),
  ]);
  expect(inputBox).not.toBeNull();
  expect(listboxBox).not.toBeNull();
  expect(popupBox).not.toBeNull();
  if (!inputBox || !listboxBox || !popupBox) return;
  const inside = (
    child: { x: number; y: number; width: number; height: number },
    parent: { x: number; y: number; width: number; height: number },
  ) =>
    child.x >= parent.x &&
    child.y >= parent.y &&
    child.x + child.width <= parent.x + parent.width &&
    child.y + child.height <= parent.y + parent.height;
  const viewportBox = {
    height: viewport.height,
    width: viewport.width,
    x: 0,
    y: 0,
  };
  expect(inputBox.width).toBeGreaterThan(0);
  expect(inputBox.height).toBeGreaterThan(0);
  expect(listboxBox.width).toBeGreaterThan(0);
  expect(listboxBox.height).toBeGreaterThan(0);
  expect(popupBox.width).toBeGreaterThan(0);
  expect(popupBox.height).toBeGreaterThan(0);
  expect(inside(inputBox, popupBox)).toBe(true);
  expect(inside(listboxBox, popupBox)).toBe(true);
  expect(inside(popupBox, viewportBox)).toBe(true);
}

type ComposerControlBox = {
  height: number;
  width: number;
  x: number;
  y: number;
};

async function expectComposerControlRow(page: Page) {
  // Scope to the composer and match exactly: an open picker puts its options
  // in a listbox that also answers to these names, so a page-wide lookup
  // resolves several elements and fails strict mode.
  const form = page.locator("form.composer").first();
  const controls = [
    form.getByRole("button", { name: "Harness", exact: true }),
    form.getByRole("button", { name: "Model", exact: true }),
    form.getByRole("button", { name: "Session mode", exact: true }),
    form.locator(".access-mode").first(),
    form.getByRole("button", { name: "Send message", exact: true }),
  ];
  const centers = await Promise.all(
    controls.map(async (control) => {
      await expect(control).toBeVisible();
      return control.evaluate((node) => {
        const box = node.getBoundingClientRect();
        return box.y + box.height / 2;
      });
    }),
  );
  expect(Math.max(...centers) - Math.min(...centers)).toBeLessThan(2);
}

async function expectComposerPickerValue(
  picker: Locator,
  name: "Harness" | "Model" | "Session mode",
  value: string,
) {
  await expect(picker).toHaveAccessibleName(name);
  await expect(picker).toHaveText(value);
  expect(
    await picker.evaluate((node) => node.scrollWidth === node.clientWidth),
  ).toBe(true);
}

async function expectComposerControlsLaidOut(page: Page) {
  const controls = [
    page.getByRole("button", { name: "Harness" }),
    page.getByRole("button", { name: "Model" }),
    page.getByRole("textbox", { name: "Message agent" }),
    page.getByRole("button", { name: "Send message" }),
  ];
  const boxes: ComposerControlBox[] = [];
  for (const control of controls) {
    await expect(control).toBeVisible();
    const box = await control.boundingBox();
    expect(box).not.toBeNull();
    if (!box) return;
    expect(box.width).toBeGreaterThan(0);
    expect(box.height).toBeGreaterThan(0);
    boxes.push({ height: box.height, width: box.width, x: box.x, y: box.y });
  }
  const viewport = await page.evaluate(() => ({
    height: window.innerHeight,
    width: window.innerWidth,
  }));
  for (const box of boxes) {
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.y).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(viewport.width);
    expect(box.y + box.height).toBeLessThanOrEqual(viewport.height);
  }
  for (let first = 0; first < boxes.length; first += 1) {
    for (let second = first + 1; second < boxes.length; second += 1) {
      const a = boxes[first];
      const b = boxes[second];
      if (!a || !b) continue;
      const overlap =
        a.x < b.x + b.width &&
        b.x < a.x + a.width &&
        a.y < b.y + b.height &&
        b.y < a.y + a.height;
      expect(overlap).toBe(false);
    }
  }
}

async function expectArtifactConstrainedComposer(
  page: Page,
  width: number,
  expectedLabels: readonly string[],
) {
  const state = await page
    .locator("form.composer")
    .first()
    .evaluate((form) => {
      const controls = form.querySelector<HTMLElement>(".composer__controls");
      const affordances = form.querySelector<HTMLElement>(
        ".composer__affordances",
      );
      const send = form.querySelector<HTMLElement>(".send-button");
      const toolbar = form.querySelector<HTMLElement>(".composer__toolbar");
      const chips = [
        ...form.querySelectorAll<HTMLElement>(
          ".composer__controls > .select-control",
        ),
      ];
      if (!controls || chips.length === 0) return null;
      return {
        chips: chips.map((chip) => {
          const chipBox = chip.getBoundingClientRect();
          const trigger = chip.querySelector<HTMLElement>(
            ".searchable-picker__trigger",
          );
          const triggerBox = trigger?.getBoundingClientRect();
          const label = chip.querySelector<HTMLElement>(
            ".searchable-picker__trigger-label",
          );
          return {
            containsTrigger:
              triggerBox !== undefined &&
              triggerBox.left >= chipBox.left &&
              triggerBox.right <= chipBox.right &&
              triggerBox.top >= chipBox.top &&
              triggerBox.bottom <= chipBox.bottom,
            overflow: chip.scrollWidth > chip.clientWidth,
            label: label?.textContent?.trim() ?? "",
            labelVisible:
              label !== null && label.scrollWidth <= label.clientWidth,
            top: chipBox.top,
            triggerWidth: triggerBox?.width ?? 0,
            width: chipBox.width,
          };
        }),
        affordancesBottom: affordances
          ? affordances.getBoundingClientRect().bottom
          : null,
        toolbarTop: toolbar ? toolbar.getBoundingClientRect().top : null,
        controlsCentre:
          controls.getBoundingClientRect().top +
          controls.getBoundingClientRect().height / 2,
        controlsWidth: controls.clientWidth,
        controlsOverflow: controls.scrollWidth > controls.clientWidth,
        items: [...controls.children]
          .filter(
            (child): child is HTMLElement =>
              child instanceof HTMLElement && child.checkVisibility(),
          )
          .map((child) => {
            const box = child.getBoundingClientRect();
            return { top: box.top, width: box.width };
          }),
        gap: Number.parseFloat(getComputedStyle(controls).columnGap),
        sendCenter: send
          ? send.getBoundingClientRect().top +
            send.getBoundingClientRect().height / 2
          : null,
        formOverflow: form.scrollWidth > form.clientWidth,
      };
    });
  expect(state).not.toBeNull();
  if (!state) return;
  expect(state.chips, `${width}px Chat column chips`).not.toHaveLength(0);
  expect(state.chips, `${width}px Chat column chips`).toEqual(
    expect.arrayContaining([
      expect.objectContaining({ containsTrigger: true, overflow: false }),
    ]),
  );
  expect(
    state.chips.every((chip) => chip.containsTrigger && !chip.overflow),
    `${width}px Chat column chips`,
  ).toBe(true);
  expect(
    state.chips.every((chip) => chip.triggerWidth >= 44),
    `${width}px Chat column triggers`,
  ).toBe(true);
  expect(
    state.chips.map((chip) => chip.label),
    `${width}px Chat column labels`,
  ).toEqual(expectedLabels);
  expect(
    state.chips.every((chip) => chip.labelVisible),
    `${width}px Chat column label visibility`,
  ).toBe(true);
  expect(state.controlsOverflow, `${width}px Chat column`).toBe(false);
  const rows: { top: number; items: (typeof state.items)[number][] }[] = [];
  for (const item of [...state.items].sort((a, b) => a.top - b.top)) {
    const row = rows.at(-1);
    if (row !== undefined && item.top - row.top <= 2) {
      row.items.push(item);
      continue;
    }
    rows.push({ top: item.top, items: [item] });
  }
  for (let index = 1; index < rows.length; index += 1) {
    const previousRow = rows[index - 1]?.items ?? [];
    const nextRow = rows[index]?.items ?? [];
    const previousWidth = previousRow.reduce(
      (total, item) => total + item.width,
      0,
    );
    expect(
      previousWidth + state.gap * previousRow.length + (nextRow[0]?.width ?? 0),
      `${width}px Chat column wrapping`,
    ).toBeGreaterThan(state.controlsWidth);
  }
  expect(
    state.affordancesBottom,
    `${width}px composer affordances`,
  ).not.toBeNull();
  expect(state.toolbarTop, `${width}px composer toolbar`).not.toBeNull();
  expect(state.sendCenter, `${width}px send action`).not.toBeNull();
  expect(
    state.affordancesBottom ?? 0,
    `${width}px composer affordances`,
  ).toBeLessThanOrEqual((state.toolbarTop ?? 0) + 1);
  expect(
    Math.abs((state.sendCenter ?? 0) - state.controlsCentre),
    `${width}px send action`,
  ).toBeLessThanOrEqual(1);
  expect(state.formOverflow, `${width}px Chat column`).toBe(false);
}

test.describe("Wave 2 canonical visual states", () => {
  test.use({ viewport: { width: 1380, height: 880 } });

  for (const { name, mode } of VISUAL_SCENARIOS.filter(
    (scenario) =>
      AGENT_VISUAL_SCENARIOS.includes(scenario.name) &&
      ![
        "agent-command-rollup",
        "agent-picker-harness-model",
        "agent-picker-handoff",
        "agent-composer-narrow",
        "agent-composer-context-unknown",
        "agent-composer-context-known",
        "agent-composer-context-over-capacity",
        "agent-session-mode-closed",
        "agent-session-mode-open",
        "agent-session-mode-narrow",
        "agent-session-mode-forced-colors",
      ].includes(scenario.name),
  )) {
    test(`${name} stays visually stable`, async ({ page }) => {
      await runAgentCanonicalTest(page, name, mode);
    });
  }
});

test.describe("Tool activity rollup disclosure", () => {
  test("opens real details content with pointer and keyboard activation", async ({
    page,
  }) => {
    await openScenario(page, "agent-declared-test", "agent");
    const rollup = page.locator(".acp-tool-rollup");
    const summary = rollup.locator("summary");
    const items = rollup.locator(".acp-tool-rollup__items");

    await expect(rollup).not.toHaveAttribute("open", "");
    await expect(items).toHaveCount(0);
    await summary.click();
    await expect(rollup).toHaveAttribute("open", "");
    await expect(items).toBeVisible();

    await summary.click();
    await expect(rollup).not.toHaveAttribute("open", "");
    await expect(items).toHaveCount(0);
    await summary.focus();
    await expect(summary).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(rollup).toHaveAttribute("open", "");
    await expect(items).toBeVisible();
  });
});

test.describe("Tool activity rollup summary containment", () => {
  test.use({ viewport: { width: 640, height: 720 } });

  test("keeps the rollup label on one line in a narrow Chat pane", async ({
    page,
  }) => {
    await openScenario(page, "agent-command-rollup", "agent");
    await settleTranscriptAtEnd(page);

    const rollup = page.locator(".acp-tool-rollup");
    await expect(rollup).toHaveCount(1);
    const label = rollup.locator(".acp-tool-rollup__label");
    await expect(label).toHaveText("Ran 2 commands");
    const lead = rollup.locator(".acp-tool-rollup__lead");
    await expect(lead).toBeVisible();
    await expect(lead).toHaveAttribute(
      "title",
      "powershell -NoProfile -Command 'Get-Content -Path alpha.txt -TotalCount 40'",
    );

    const geometry = await rollup.evaluate((node) => {
      const labelNode = node.querySelector(".acp-tool-rollup__label");
      const leadNode = node.querySelector(".acp-tool-rollup__lead");
      const summaryNode = node.querySelector("summary");
      if (!(labelNode instanceof HTMLElement)) return null;
      if (!(leadNode instanceof HTMLElement)) return null;
      if (!(summaryNode instanceof HTMLElement)) return null;
      const style = getComputedStyle(labelNode);
      const parsedLineHeight = Number.parseFloat(style.lineHeight);
      const lineHeight = Number.isFinite(parsedLineHeight)
        ? parsedLineHeight
        : Number.parseFloat(style.fontSize) * 1.5;
      return {
        labelLines: labelNode.getClientRects().length,
        labelHeight: labelNode.getBoundingClientRect().height,
        leadWidth: leadNode.getBoundingClientRect().width,
        leadLines: leadNode.getClientRects().length,
        leadClipped: leadNode.scrollWidth - leadNode.clientWidth,
        leadTextOverflow: getComputedStyle(leadNode).textOverflow,
        leadTitle: leadNode.getAttribute("title") ?? "",
        leadText: leadNode.textContent ?? "",
        lineHeight,
        summaryHeight: summaryNode.getBoundingClientRect().height,
        summaryWidth: summaryNode.getBoundingClientRect().width,
        cardOverflow: node.scrollWidth - node.clientWidth,
      };
    });

    expect(geometry, "the rollup summary is measurable").not.toBeNull();
    if (!geometry) return;
    expect(
      geometry.summaryWidth,
      "the Chat pane is narrow enough to squeeze the summary row",
    ).toBeLessThan(520);
    expect(
      geometry.labelLines,
      "the rollup label renders as a single line box",
    ).toBe(1);
    expect(
      geometry.labelHeight,
      "the rollup label never grows past one text line",
    ).toBeLessThanOrEqual(geometry.lineHeight * 1.5);
    expect(
      geometry.summaryHeight,
      "the summary row stays a compact single-line row",
    ).toBeLessThanOrEqual(geometry.lineHeight * 2);
    expect(
      geometry.cardOverflow,
      "the summary row does not overflow its card",
    ).toBeLessThanOrEqual(1);
    expect(
      geometry.leadWidth,
      "the lead detail keeps a readable share of the row",
    ).toBeGreaterThan(40);
    expect(
      geometry.leadLines,
      "the lead detail stays on the label's line",
    ).toBe(1);
    expect(
      geometry.leadClipped,
      "the lead detail is the part that truncates",
    ).toBeGreaterThan(0);
    expect(
      geometry.leadTextOverflow,
      "the truncated lead detail ends in an ellipsis",
    ).toBe("ellipsis");
    expect(
      geometry.leadTitle,
      "the full command stays available as the lead title",
    ).toBe(geometry.leadText);
  });
});

test.describe("ED02 picker visual states", () => {
  test("captures the ready harness and model picker state", async ({
    page,
  }) => {
    const workbench = await openScenario(
      page,
      "agent-picker-harness-model",
      "agent",
    );
    const trigger = page.getByRole("button", { name: "Harness" });
    await trigger.click();
    const input = page.getByRole("combobox", { name: "Harness" });
    const listbox = page.getByRole("listbox", { name: "Harness" });
    await expect(input).toBeFocused();
    await expect(
      listbox.getByRole("option", { name: "Codex — test only", exact: true }),
    ).toHaveAttribute("aria-selected", "true");
    const unavailable = listbox.getByRole("option", {
      name: /Unavailable Codex.*Sage did not admit this harness and will not start it\./,
    });
    await expect(unavailable).toHaveAttribute("aria-disabled", "true");
    await input.fill("Unavailable");
    await expect(listbox.getByRole("option")).toHaveCount(1);
    await expect(listbox.getByRole("option")).toBeDisabled();
    await input.fill("Codex");
    await expect(listbox.getByRole("option")).toHaveCount(2);
    await expect(
      listbox.getByRole("option", { name: "Codex — test only", exact: true }),
    ).toBeVisible();
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Enter");
    await expect(trigger).toHaveText("Codex");
    await expect(trigger).toHaveAttribute("data-selected-id", "visual-acp");
    await expect(page.locator("html")).toHaveAttribute(
      "data-visual-harness-selection-count",
      "1",
    );
    const modelTrigger = page.getByRole("button", { name: "Model" });
    await expect(modelTrigger).toBeDisabled();
    await expect(
      page.getByText("Wait while Sage checks the selected Harness."),
    ).toBeVisible();
    expect(
      await page.evaluate(() => {
        const visualWindow = window as typeof window & {
          __releaseVisualPickerHarnessReady?: () => void;
        };
        const release = visualWindow.__releaseVisualPickerHarnessReady;
        if (!release) return false;
        release();
        return true;
      }),
    ).toBe(true);
    await expect(modelTrigger).toBeEnabled();
    await modelTrigger.click();
    const modelInput = page.getByRole("combobox", { name: "Model" });
    const modelListbox = page.getByRole("listbox", { name: "Model" });
    await expect(modelListbox.getByRole("option")).toHaveCount(128);
    await expect(
      modelListbox.getByRole("option", {
        name: "Fresh advertised model 1",
        exact: true,
      }),
    ).toBeVisible();
    await expect(
      modelListbox.locator('[role="option"][aria-selected="true"]'),
      "choosing a harness clears the model, so the list opens unselected",
    ).toHaveCount(0);
    await expect(
      modelListbox.getByRole("option", { name: "Old advertised model" }),
    ).toHaveCount(0);
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Enter");
    await expect(modelTrigger).toHaveText("Fresh advertised model 2");
    await expect(page.locator("html")).toHaveAttribute(
      "data-visual-model-selection-count",
      "1",
    );
    await modelTrigger.click();
    await expectOpenPickerContained(page, modelInput, modelListbox);
    await settleTranscriptAtEnd(page);
    await expectNoViewportOverflow(page);
    await takeCanonicalScreenshot(workbench, "agent-picker-harness-model");
  });

  test("captures the modal-safe handoff target picker", async ({ page }) => {
    const workbench = await openScenario(page, "agent-picker-handoff", "agent");
    const sourceChat = page.getByRole("region", {
      name: "Chat pane Source chat",
      exact: true,
    });
    const targetChat = page.getByRole("region", {
      name: "Chat pane Target chat",
      exact: true,
    });
    const handoffTrigger = sourceChat.getByRole("button", {
      name: "Hand off chat",
      exact: true,
    });
    await expect(handoffTrigger).toHaveCount(1);
    await expect(handoffTrigger).toBeEnabled();
    await handoffTrigger.click();
    const dialog = page.getByRole("dialog", { name: "Hand off this Chat" });
    const trigger = dialog.getByRole("button", { name: "Target Chat" });
    await expect(trigger).toHaveText("Target chat");
    await trigger.click();
    const input = dialog.getByRole("combobox", { name: "Target Chat" });
    const listbox = dialog.getByRole("listbox", { name: "Target Chat" });
    await expect(input).toBeFocused();
    await expect(
      listbox.getByRole("option", { name: "Target chat", exact: true }),
    ).toHaveAttribute("aria-selected", "true");
    await expect(
      dialog.getByText("Current: Target chat", { exact: true }),
    ).toBeVisible();
    await expect(
      listbox.getByRole("option", { name: "Target chat" }),
    ).toBeVisible();
    await expect(listbox.getByRole("option")).toHaveCount(63);
    await page.keyboard.press("ArrowDown");
    await expect(input).toHaveAttribute(
      "aria-activedescendant",
      /option-visual-chat-handoff-target/,
    );
    await page.keyboard.press("Enter");
    await expect(trigger).toHaveText("Target chat");
    await expect(trigger).toBeFocused();
    await trigger.click();
    await expectOpenPickerContained(page, input, listbox);
    await expectNoViewportOverflow(page);
    await page.keyboard.press("Escape");
    await expect(dialog).toBeVisible();
    await expect(trigger).toBeFocused();
    const draft = dialog.getByRole("textbox", { name: "Handoff prompt" });
    await draft.fill("Edited but unsent visual handoff");
    await expect(draft).toHaveValue("Edited but unsent visual handoff");
    await expect(trigger).toHaveText("Target chat");
    await expect(
      sourceChat.getByRole("textbox", { name: "Message agent" }),
    ).toHaveValue("");
    await takeCanonicalScreenshot(workbench, "agent-picker-handoff");
    expect(
      await page.evaluate(() => {
        const visualWindow = window as typeof window & {
          __activateVisualHandoffTarget?: () => void;
        };
        const activateTarget = visualWindow.__activateVisualHandoffTarget;
        if (!activateTarget) return false;
        activateTarget();
        return true;
      }),
    ).toBe(true);
    await expect(dialog).toHaveCount(0);
    await expect(handoffTrigger).toBeFocused();
    await expect(page.locator("html")).toHaveAttribute(
      "data-visual-handoff-dispatch-count",
      "0",
    );
    await expect(
      sourceChat.getByRole("textbox", { name: "Message agent" }),
    ).toHaveValue("");
    await expect(
      targetChat.getByRole("textbox", { name: "Message agent" }),
    ).toHaveValue("");
  });

  test("captures the compact narrow composer picker", async ({ page }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    const workbench = await openScenario(
      page,
      "agent-composer-narrow",
      "agent",
    );
    await expectComposerControlsLaidOut(page);
    const harnessTrigger = page.getByRole("button", { name: "Harness" });
    await harnessTrigger.click();
    const input = page.getByRole("combobox", { name: "Harness" });
    const listbox = page.getByRole("listbox", { name: "Harness" });
    await expectOpenPickerContained(page, input, listbox);
    await expectNoViewportOverflow(page);
    await page.keyboard.press("Escape");
    await expect(harnessTrigger).toBeFocused();
    const modelTrigger = page.getByRole("button", { name: "Model" });
    await modelTrigger.click();
    await expectOpenPickerContained(
      page,
      page.getByRole("combobox", { name: "Model" }),
      page.getByRole("listbox", { name: "Model" }),
    );
    await expectNoViewportOverflow(page);
    await page.keyboard.press("Escape");
    await expect(modelTrigger).toBeFocused();
    await expectComposerControlsLaidOut(page);
    await harnessTrigger.click();
    await expectOpenPickerContained(page, input, listbox);
    await expectNoViewportOverflow(page);
    await takeCanonicalScreenshot(workbench, "agent-composer-narrow");
  });
});

test.describe("Artifact-constrained composer", () => {
  test("keeps picker chips legible across constrained Chat widths", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "agent-session-mode-closed", "agent");
    await page.getByRole("button", { name: /Show preview/ }).click();
    await page.evaluate(async () => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      const state = useWorkbenchStore.getState();
      useWorkbenchStore.setState({
        harnesses: state.harnesses.map((harness) =>
          harness.id === "visual-acp"
            ? { ...harness, name: "Claude Code" }
            : harness,
        ),
        models: state.models.map((model) => ({
          ...model,
          name: "Claude Opus 4.8 (1M context)",
        })),
      });
    });

    for (const width of [560, 589, 590, 630, 720, 721, 775] as const) {
      await page.locator(".workbench__body").evaluate((body, centerWidth) => {
        body.style.gridTemplateColumns = `224px 8px ${centerWidth}px 8px minmax(0, 1fr)`;
      }, width);
      await expect(page.locator(".agent-chat-pane")).toHaveCSS(
        "width",
        `${width}px`,
      );
      await expectArtifactConstrainedComposer(page, width, [
        "Claude Code",
        "Claude Opus 4.8 (1M context)",
        "Review changes",
        "Read-only",
      ]);
      await expectNoViewportOverflow(page);
    }
  });
});

test.describe("V2-4 session mode picker", () => {
  test("captures closed, open, narrow, and forced-color mode states", async ({
    page,
  }) => {
    const closedWorkbench = await openScenario(
      page,
      "agent-session-mode-closed",
      "agent",
    );
    const closedTrigger = page.getByRole("button", {
      name: "Session mode",
      exact: true,
    });
    await expectComposerPickerValue(
      page.getByRole("button", { name: "Harness", exact: true }),
      "Harness",
      "Codex",
    );
    await expectComposerPickerValue(
      page.getByRole("button", { name: "Model", exact: true }),
      "Model",
      "GPT-5",
    );
    await expectComposerPickerValue(
      closedTrigger,
      "Session mode",
      "Review changes",
    );
    await expect(page.locator(".project-scope")).toHaveText("Sage");
    await expect(page.locator(".project-scope")).toHaveAttribute(
      "title",
      "Project switching is unavailable",
    );
    await expectComposerControlRow(page);
    await takeCanonicalScreenshot(closedWorkbench, "agent-session-mode-closed");

    const openWorkbench = await openScenario(
      page,
      "agent-session-mode-open",
      "agent",
    );
    const openTrigger = page.getByRole("button", {
      name: "Session mode",
      exact: true,
    });
    await openTrigger.click();
    const openInput = page.getByRole("combobox", { name: "Session mode" });
    const openListbox = page.getByRole("listbox", { name: "Session mode" });
    await expect(openListbox.getByRole("option")).toHaveCount(2);
    await expect(openListbox.getByRole("option").nth(0)).toHaveText(
      "Review changes",
    );
    await expect(openListbox.getByRole("option").nth(1)).toHaveText(
      "Apply changes",
    );
    await expectOpenPickerContained(page, openInput, openListbox);
    await takeCanonicalScreenshot(openWorkbench, "agent-session-mode-open");

    await page.setViewportSize({ width: 720, height: 560 });
    const narrowWorkbench = await openScenario(
      page,
      "agent-session-mode-narrow",
      "agent",
    );
    const narrowTrigger = page.getByRole("button", {
      name: "Session mode",
      exact: true,
    });
    await expect(narrowTrigger).toBeVisible();
    await narrowTrigger.click();
    await expectOpenPickerContained(
      page,
      page.getByRole("combobox", { name: "Session mode" }),
      page.getByRole("listbox", { name: "Session mode" }),
    );
    await expectNoViewportOverflow(page);
    await takeCanonicalScreenshot(narrowWorkbench, "agent-session-mode-narrow");

    await page.emulateMedia({
      forcedColors: "active",
      reducedMotion: "reduce",
    });
    const forcedWorkbench = await openScenario(
      page,
      "agent-session-mode-forced-colors",
      "agent",
    );
    const forcedTrigger = page.getByRole("button", {
      name: "Session mode",
      exact: true,
    });
    await forcedTrigger.click();
    const forcedState = await page
      .getByRole("listbox", { name: "Session mode" })
      .getByRole("option", { name: "Review changes" })
      .evaluate((option) => ({
        background: getComputedStyle(option).backgroundColor,
        color: getComputedStyle(option).color,
      }));
    expect(forcedState.background).not.toBe("rgba(0, 0, 0, 0)");
    expect(forcedState.color).not.toBe("rgba(0, 0, 0, 0)");
    await takeCanonicalScreenshot(
      forcedWorkbench,
      "agent-session-mode-forced-colors",
    );
  });
});

test.describe("V6 Composer context meter", () => {
  test("renders runtime usage truthfully across unknown, known, and over-capacity states", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 720, height: 560 });

    const unknownWorkbench = await openScenario(
      page,
      "agent-composer-context-unknown",
      "agent",
    );
    const unknownContext = page.getByLabel("Composer context", { exact: true });
    await expect(unknownContext).toContainText("Reported usage · 6,000 tokens");
    await expect(unknownContext).not.toContainText("6,000 / ");
    await expect(unknownContext.getByRole("progressbar")).toHaveCount(0);
    await expectNoViewportOverflow(page);
    await takeCanonicalScreenshot(
      unknownWorkbench,
      "agent-composer-context-unknown",
    );

    const knownWorkbench = await openScenario(
      page,
      "agent-composer-context-known",
      "agent",
    );
    const knownContext = page.getByLabel("Composer context", { exact: true });
    const knownProgress = knownContext.getByRole("progressbar", {
      name: "Reported usage",
    });
    await expect(knownContext).toContainText("6K context");
    await expect(
      knownContext.locator(".composer__context-usage-label"),
    ).toHaveText("6K context");
    await expect(knownProgress).toHaveCount(1);
    await expect(knownProgress).toHaveAttribute("aria-valuemin", "0");
    await expect(knownProgress).toHaveAttribute("aria-valuemax", "272000");
    await expect(knownProgress).toHaveAttribute("aria-valuenow", "6000");
    await expect(knownProgress).toHaveAttribute(
      "title",
      "6,000 / 272,000 tokens",
    );
    await expect(knownProgress).toHaveAttribute(
      "aria-valuetext",
      "6,000 / 272,000 tokens used",
    );
    expect(
      await knownContext.evaluate((node) =>
        Boolean(node.closest(".composer__input-area")),
      ),
    ).toBe(true);
    const knownContextBox = await knownContext.boundingBox();
    const knownToolbarBox = await page
      .locator("form.composer .composer__toolbar")
      .first()
      .boundingBox();
    expect(knownContextBox).not.toBeNull();
    expect(knownToolbarBox).not.toBeNull();
    expect(
      (knownContextBox?.y ?? 0) + (knownContextBox?.height ?? 0),
    ).toBeLessThanOrEqual((knownToolbarBox?.y ?? 0) + 1);
    await expectNoViewportOverflow(page);
    await takeCanonicalScreenshot(
      knownWorkbench,
      "agent-composer-context-known",
    );

    const overCapacityWorkbench = await openScenario(
      page,
      "agent-composer-context-over-capacity",
      "agent",
    );
    const overCapacityContext = page.getByLabel("Composer context", {
      exact: true,
    });
    const overCapacityProgress = overCapacityContext.getByRole("progressbar", {
      name: "Reported usage",
    });
    const fill = overCapacityProgress.locator(".composer__context-usage-fill");
    await expect(overCapacityContext).toContainText("300K context");
    await expect(
      overCapacityContext.locator(".composer__context-usage-label"),
    ).toHaveText("300K context");
    await expect(overCapacityProgress).toHaveCount(1);
    await expect(overCapacityProgress).toHaveAttribute(
      "aria-valuenow",
      "272000",
    );
    await expect(overCapacityProgress).toHaveAttribute(
      "aria-valuetext",
      "300,000 / 272,000 tokens used",
    );
    await expect(overCapacityProgress).toHaveAttribute(
      "title",
      "300,000 / 272,000 tokens",
    );
    expect(
      await fill.evaluate((element) => {
        const fillWidth = element.getBoundingClientRect().width;
        const trackWidth =
          element.parentElement?.getBoundingClientRect().width ?? 0;
        return trackWidth > 0 && fillWidth / trackWidth <= 1;
      }),
    ).toBe(true);
    await expectNoViewportOverflow(page);
    await takeCanonicalScreenshot(
      overCapacityWorkbench,
      "agent-composer-context-over-capacity",
    );

    await page.emulateMedia({
      forcedColors: "active",
      reducedMotion: "reduce",
    });
    const forcedColors = await overCapacityProgress.evaluate((element) => {
      const fillElement = element.querySelector<HTMLElement>(
        ".composer__context-usage-fill",
      );
      const systemProbe = document.createElement("span");
      systemProbe.style.background = "Highlight";
      systemProbe.style.border = "1px solid CanvasText";
      document.body.append(systemProbe);
      const highlight = getComputedStyle(systemProbe).backgroundColor;
      const canvasText = getComputedStyle(systemProbe).borderTopColor;
      systemProbe.style.background = "Canvas";
      const canvas = getComputedStyle(systemProbe).backgroundColor;
      systemProbe.remove();
      return {
        canvas,
        canvasText,
        fill: fillElement
          ? getComputedStyle(fillElement).backgroundColor
          : null,
        highlight,
        track: getComputedStyle(element).backgroundColor,
        trackBorder: getComputedStyle(element).borderTopColor,
        trackBorderStyle: getComputedStyle(element).borderTopStyle,
      };
    });
    expect(forcedColors.fill).toBe(forcedColors.highlight);
    expect(forcedColors.track).toBe(forcedColors.canvas);
    expect(forcedColors.trackBorder).toBe(forcedColors.canvasText);
  });
});

test.describe("V5b Agent image presentation", () => {
  test("stays contained at narrow width, forced colors, and reduced motion", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    await openScenario(page, "agent-image", "agent");

    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expectAgentImageContained(page);
    await expectNoViewportOverflow(page);

    await page.emulateMedia({
      forcedColors: "active",
      reducedMotion: "reduce",
    });
    expect(
      await page.locator(".acp-image").evaluate((figure) => {
        const figureStyle = getComputedStyle(figure);
        const imageStyle = getComputedStyle(
          figure.querySelector(".acp-image__src") ?? figure,
        );
        return {
          borderStyle: figureStyle.borderTopStyle,
          borderColor: figureStyle.borderTopColor,
          animationDuration: imageStyle.animationDuration,
          transitionDuration: imageStyle.transitionDuration,
        };
      }),
    ).toEqual({
      borderStyle: "solid",
      borderColor: "rgb(255, 255, 255)",
      animationDuration: "0s",
      transitionDuration: "0s",
    });
  });
});

test.describe("V5c Agent child activity presentation", () => {
  test("keeps four read-only levels usable at narrow width and forced colors", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    await openScenario(page, "agent-child-activity", "agent");
    const { activity, tree } = await expandChildActivityHierarchy(page);

    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expectChildActivityContained(page);
    const longLabel =
      "Coordinate the independent accessibility review across every visible workbench surface";
    const collaboration = page.getByRole("group", {
      name: `Harness collaboration: ${longLabel}`,
    });
    await expect(collaboration).toBeVisible();
    await expect(
      collaboration.locator(".acp-collaboration__label"),
    ).toHaveAttribute("title", longLabel);
    expect(
      await collaboration
        .locator(".acp-collaboration__label")
        .evaluate((label) => label.scrollWidth > label.clientWidth),
    ).toBe(true);
    await expectNoViewportOverflow(page);

    await page.emulateMedia({
      forcedColors: "active",
      reducedMotion: "reduce",
    });
    expect(
      await activity.evaluate((node) => {
        const item = node.querySelector<HTMLElement>(
          ".acp-child-activity__item",
        );
        const itemStyle = item ? getComputedStyle(item, "::before") : null;
        const label = node.querySelector<HTMLElement>(
          ".acp-collaboration__label",
        );
        return {
          activityBorderStyle: getComputedStyle(node).borderLeftStyle,
          connectorBorderStyle: itemStyle?.borderTopStyle,
          animationDuration: label
            ? getComputedStyle(label).animationDuration
            : null,
          transitionDuration: label
            ? getComputedStyle(label).transitionDuration
            : null,
        };
      }),
    ).toEqual({
      activityBorderStyle: "solid",
      connectorBorderStyle: "solid",
      animationDuration: "0s",
      transitionDuration: "0s",
    });
    await expect(tree.locator('[aria-level="4"]')).toBeFocused();
  });
});

test.describe("Wave 2 accessibility and responsive states", () => {
  test("keeps dual Chat header geometry stable when focus changes", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "agent-dual", "agent");

    const panes = page.locator(".agent-chat-pane");
    const paneIdentities = page.locator(".agent-chat-pane__identity");
    const actionRows = page.locator(".agent-chat-pane .surface-header");
    await expect(panes).toHaveCount(2);
    await expect(paneIdentities).toHaveCount(2);
    await expect(actionRows).toHaveCount(2);
    const sidebarSplitter = page.locator(".workbench-sidebar-splitter");
    const chatSplitter = page.getByRole("separator", {
      name: "Resize chat panes",
    });
    await expect(sidebarSplitter).toHaveCount(1);
    await expect(chatSplitter).toHaveCount(1);
    expect(
      await page.evaluate(() => {
        const chatRow =
          document.querySelector<HTMLElement>(".project-chat-row");
        const chat = document.querySelector<HTMLElement>(".project-chat");
        const working = document.querySelector<HTMLElement>(
          ".project-chat__working",
        );
        const splitter = document.querySelector<HTMLElement>(
          ".agent-chat-splitter",
        );
        if (!chatRow || !chat || !working || !splitter) return null;
        return {
          chat: getComputedStyle(chat).borderRadius,
          chatRow: getComputedStyle(chatRow).borderRadius,
          splitter: getComputedStyle(splitter, "::after").borderRadius,
          working: getComputedStyle(working).borderRadius,
        };
      }),
    ).toEqual({
      chat: "6px",
      chatRow: "6px",
      splitter: "999px",
      working: "50%",
    });
    expect(
      await sidebarSplitter.evaluate(
        (node) => getComputedStyle(node, "::after").width,
      ),
    ).toBe("2px");
    expect(
      await chatSplitter.evaluate(
        (node) => getComputedStyle(node, "::after").width,
      ),
    ).toBe("2px");
    await chatSplitter.focus();
    await page.keyboard.press("Tab");
    await page.keyboard.press("Shift+Tab");
    await expect(chatSplitter).toBeFocused();
    expect(
      await chatSplitter.evaluate((node) => ({
        focusVisible: node.matches(":focus-visible"),
        lineWidth: getComputedStyle(node, "::after").width,
      })),
    ).toEqual({ focusVisible: true, lineWidth: "1px" });
    await chatSplitter.evaluate((node) => (node as HTMLElement).blur());
    expect(
      await actionRows.evaluateAll((rows) =>
        rows.every((row) => row.getBoundingClientRect().height <= 36),
      ),
    ).toBe(true);
    const shortcuts = page.locator(
      ".agent-chat-panes--split .composer__shortcut",
    );
    // The focus chord remains in the enabled composer's accessible description,
    // but no visual shortcut takes space from a split-pane control row.
    await expect(shortcuts).toHaveCount(1);
    await expect(shortcuts.first()).toBeHidden();
    // Both panes, not just the first: split hides the policy chip in each so
    // neither control group wraps and pushes its send out of the shared row.
    const splitPolicies = page.locator(".agent-chat-panes--split .access-mode");
    await expect(splitPolicies).toHaveCount(2);
    for (let index = 0; index < 2; index += 1) {
      await expect(splitPolicies.nth(index)).toBeHidden();
    }
    await page.evaluate(async () => {
      const { acpProjectionStoreFor } =
        await import("/src/app/acpProjectionRegistry.ts");
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      for (const chatId of ["visual-chat-primary", "visual-chat-secondary"]) {
        const store = acpProjectionStoreFor(chatId as never);
        const current = store.getState();
        if (!current.page || !current.scope) throw new Error("Missing Chat");
        current.applyAuthoritativePage(current.scope, {
          ...current.page,
          sessionConfig: {
            sageAgentSessionId: "visual-session",
            runtimeEpoch: "7",
            configRevision: "1",
            modes: {
              currentModeId: "review",
              availableModes: [
                {
                  id: "review",
                  name: "Review changes",
                  description: "Analyze changes without applying them.",
                },
              ],
            },
            configOptions: [],
          },
        });
      }
      const workbench = useWorkbenchStore.getState();
      const secondary = workbench.chatUiById["visual-chat-secondary"];
      useWorkbenchStore.setState({
        chatUiById: {
          ...workbench.chatUiById,
          "visual-chat-secondary": {
            ...secondary,
            modeError: "Mode update conflicted",
            modeErrorOwner: {
              sessionId: "visual-session",
              runtimeEpoch: "7",
            },
          },
        },
      });
    });
    await chatSplitter.focus();
    await page.keyboard.press("End");
    await expect(chatSplitter).toHaveAttribute("aria-valuenow", "70");
    const controlRails = panes.locator(".composer__controls");
    await expect(controlRails).toHaveCount(2);
    for (let index = 0; index < 2; index += 1) {
      const pane = panes.nth(index);
      const railDimensions = await controlRails.nth(index).evaluate((node) => ({
        scrollHeight: node.scrollHeight,
        clientHeight: node.clientHeight,
      }));
      expect(railDimensions.scrollHeight).toBeLessThanOrEqual(
        railDimensions.clientHeight + 1,
      );
      const measure = (control: Locator) =>
        control.evaluate((node) => {
          const box = node.getBoundingClientRect();
          return {
            bottom: box.bottom,
            centre: box.y + box.height / 2,
            top: box.y,
          };
        });
      const pickerBoxes = await Promise.all(
        [
          pane.getByRole("button", { name: "Harness" }),
          pane.getByRole("button", { name: "Model" }),
          pane.getByRole("button", { name: "Session mode" }),
        ].map(measure),
      );
      const sendBox = await measure(pane.locator(".send-button"));
      const pickerRows: { bottom: number; centres: number[] }[] = [];
      for (const box of [...pickerBoxes].sort((a, b) => a.top - b.top)) {
        const row = pickerRows.at(-1);
        if (row !== undefined && box.top < row.bottom - 1) {
          row.centres.push(box.centre);
          row.bottom = Math.max(row.bottom, box.bottom);
          continue;
        }
        pickerRows.push({ bottom: box.bottom, centres: [box.centre] });
      }
      const identity = `pane ${index}, ${pickerRows.length} picker rows`;
      pickerRows.forEach((row, rowIndex) => {
        expect(
          Math.max(...row.centres) - Math.min(...row.centres),
          `${identity}, row ${rowIndex}`,
        ).toBeLessThan(2);
      });
      const lastPickerRow = pickerRows.at(-1);
      if (lastPickerRow === undefined) {
        throw new Error(`${identity}: no picker rows were measured`);
      }
      // Centre lines, not top edges: send is deliberately the taller
      // emphasized square, so equal tops would mean it was NOT centred
      // against the shorter pickers.
      if (pickerRows.length === 1) {
        expect(
          Math.abs(sendBox.centre - lastPickerRow.centres[0]),
          `${identity}, send centre`,
        ).toBeLessThan(2);
      } else {
        expect(
          Math.abs(sendBox.bottom - lastPickerRow.bottom),
          `${identity}, send baseline`,
        ).toBeLessThanOrEqual(1);
      }
    }
    const narrowPane = panes.nth(1);
    const composerError = narrowPane.getByRole("alert");
    await expect(composerError).toHaveText("Mode update conflicted");
    const narrowRailBounds = await controlRails.nth(1).evaluate((node) => {
      const rail = node.getBoundingClientRect();
      const contains = (child: DOMRect) =>
        child.left >= rail.left - 1 && child.right <= rail.right + 1;
      const chips = [
        ...node.querySelectorAll<HTMLElement>(".select-control"),
      ].filter((chip) => chip.checkVisibility());
      const alert = node.querySelector<HTMLElement>('[role="alert"]');
      return {
        alertContained:
          alert === null || contains(alert.getBoundingClientRect()),
        chipsContained: chips.map((chip) =>
          contains(chip.getBoundingClientRect()),
        ),
        clientWidth: node.clientWidth,
        scrollWidth: node.scrollWidth,
      };
    });
    expect(narrowRailBounds.scrollWidth).toBeLessThanOrEqual(
      narrowRailBounds.clientWidth + 1,
    );
    expect(narrowRailBounds.chipsContained).not.toHaveLength(0);
    expect(narrowRailBounds.chipsContained).not.toContain(false);
    expect(narrowRailBounds.alertContained).toBe(true);
    expect(
      await composerError.evaluate((node) => {
        const error = node.getBoundingClientRect();
        const composer = node.closest<HTMLElement>(".composer");
        const bounds = composer?.getBoundingClientRect();
        return bounds !== undefined && error.bottom <= bounds.bottom;
      }),
    ).toBe(true);
    const sessionMode = narrowPane.getByRole("button", {
      name: "Session mode",
    });
    await sessionMode.click();
    await expect(
      page.getByRole("combobox", { name: "Session mode" }),
    ).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(sessionMode).toBeFocused();
    expect(
      await sessionMode.evaluate((node) => node.matches(":focus-visible")),
    ).toBe(true);

    const before = await panes.evaluateAll((items) =>
      items.map((item) => {
        const bounds = item.getBoundingClientRect();
        return [bounds.x, bounds.y, bounds.width, bounds.height];
      }),
    );
    const secondFocus = panes.nth(1).locator(".agent-chat-pane__focus");
    await secondFocus.click();
    await expect(secondFocus).toHaveAttribute("aria-pressed", "true");
    const after = await panes.evaluateAll((items) =>
      items.map((item) => {
        const bounds = item.getBoundingClientRect();
        return [bounds.x, bounds.y, bounds.width, bounds.height];
      }),
    );
    expect(after).toEqual(before);
    await expectNoViewportOverflow(page);
  });

  test("shows only the focused Chat when the measured center becomes tight", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1_000, height: 720 });
    const workbench = await openScenario(page, "agent-dual", "agent");

    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expect(
      page.getByRole("separator", { name: "Resize chat panes" }),
    ).toHaveCount(0);
    await expect(
      page.getByRole("region", {
        name: "Chat pane Agent navigation polish",
      }),
    ).toBeVisible();

    await page
      .getByRole("complementary", { name: "Projects and chats" })
      .getByRole("button", { name: /Open chat Editor consistency pass/ })
      .click();
    await expect(
      page.getByRole("region", {
        name: "Chat pane Editor consistency pass",
      }),
    ).toBeVisible();
    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expectNoViewportOverflow(page);
    await settleTranscriptAtEnd(page);
    await expect(workbench).toHaveScreenshot("agent-dual-medium.png");
  });

  test("supports a 720px narrow Agent window", async ({ page }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    const workbench = await openScenario(page, "agent-dual", "agent");

    await expect(
      page.getByRole("region", { name: "Agent chat", exact: true }),
    ).toBeVisible();
    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expect(
      page.getByRole("separator", { name: "Resize chat panes" }),
    ).toHaveCount(0);
    await expectNoViewportOverflow(page);
    await settleTranscriptAtEnd(page);
    await expect(workbench).toHaveScreenshot("agent-narrow-720x560.png");
  });

  test("exposes visible keyboard focus", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "agent-single", "agent");

    const composerSelector =
      'textarea[aria-label="Message agent"]:not(:disabled)';
    const composerTextarea = page.locator(composerSelector);
    const currentProjection = page.locator(
      ".agent-chat-pane--focused .acp-chat-projection",
    );
    await expect(currentProjection).toHaveAttribute(
      "data-project-id",
      "visual-project",
    );
    await expect(currentProjection).toHaveAttribute(
      "data-chat-id",
      "visual-chat-agent-single",
    );
    await expect(currentProjection).toHaveAttribute(
      "data-current-session-id",
      "visual-session",
    );
    await expect(composerTextarea).toBeVisible();
    await expect(composerTextarea).toBeEnabled();
    await page.keyboard.press("ControlOrMeta+L");
    await expect(composerTextarea).toBeFocused();
    expect(
      await composerTextarea.evaluate(
        (textarea) => document.activeElement === textarea,
      ),
    ).toBe(true);
    expect(
      await composerTextarea.evaluate((textarea) =>
        textarea.matches(":focus-visible"),
      ),
    ).toBe(true);
    const focusVisible = page.locator(":focus-visible");
    await expect(focusVisible).toHaveCount(1);
    expect(
      await focusVisible.evaluate(
        (focused, selector) => focused === document.querySelector(selector),
        composerSelector,
      ),
    ).toBe(true);
    await expect
      .poll(
        () =>
          focusVisible.evaluate((trigger) => {
            const owner = trigger.closest<HTMLElement>(".composer");
            if (!owner) return false;
            const edgeProbe = document.createElement("span");
            edgeProbe.style.border = "1px solid var(--highlight-focus-edge)";
            owner.append(edgeProbe);
            const target = getComputedStyle(edgeProbe).borderTopColor;
            edgeProbe.remove();
            return getComputedStyle(owner).borderTopColor === target;
          }),
        { timeout: 1_000 },
      )
      .toBe(true);
    const sharedFocus = await composerTextarea.evaluate((trigger) => {
      const identity = (element: Element | null) => {
        if (!element) return "none";
        const id = element.id ? `#${element.id}` : "";
        const classes = [...element.classList]
          .map((className) => `.${className}`)
          .join("");
        return `${element.tagName.toLowerCase()}${id}${classes}`;
      };
      const transparentProbe = document.createElement("span");
      transparentProbe.style.border = "1px solid transparent";
      document.body.append(transparentProbe);
      const transparentColor =
        getComputedStyle(transparentProbe).borderTopColor;
      transparentProbe.remove();

      const owner = trigger.closest<HTMLElement>(".composer");
      const ownerStyle = owner ? getComputedStyle(owner) : null;
      const workbench = trigger.closest<HTMLElement>(".workbench");
      const workbenchStyle = workbench ? getComputedStyle(workbench) : null;
      const halo = ownerStyle?.boxShadow ?? "none";
      const sharedStyleProbe = document.createElement("span");
      sharedStyleProbe.style.border = "1px solid var(--highlight-focus-edge)";
      sharedStyleProbe.style.boxShadow =
        "var(--highlight-focus-halo-shadow), var(--elevation-popover)";
      (owner ?? document.body).append(sharedStyleProbe);
      const sharedProbeStyle = getComputedStyle(sharedStyleProbe);
      const resolvedEdge = sharedProbeStyle.borderTopColor;
      const resolvedHalo = sharedProbeStyle.boxShadow;
      sharedStyleProbe.remove();
      return {
        edgeToken: workbenchStyle
          ?.getPropertyValue("--highlight-focus-edge")
          .trim(),
        haloToken: workbenchStyle
          ?.getPropertyValue("--highlight-focus-halo")
          .trim(),
        edgeColor: ownerStyle?.borderTopColor,
        edgeStyle: ownerStyle?.borderTopStyle,
        edgeWidth: ownerStyle?.borderTopWidth,
        focusRingOffset: ownerStyle
          ?.getPropertyValue("--focus-ring-offset")
          .trim(),
        focusWithinMatches: owner?.matches(":focus-within") ?? false,
        halo,
        ownerContainsTrigger: owner?.contains(trigger) ?? false,
        ownerIdentity: identity(owner),
        resolvedEdge,
        resolvedHalo,
        transparentColor,
        triggerIdentity: identity(trigger),
      };
    });
    const focusIdentity = `trigger=${sharedFocus.triggerIdentity}; owner=${sharedFocus.ownerIdentity}`;
    expect(sharedFocus, focusIdentity).toMatchObject({
      edgeStyle: "solid",
      edgeWidth: "1px",
      focusRingOffset: "1px",
      focusWithinMatches: true,
      ownerContainsTrigger: true,
    });
    expect(sharedFocus.edgeToken, focusIdentity).toContain("color-mix");
    expect(sharedFocus.edgeToken, focusIdentity).toContain("transparent");
    expect(sharedFocus.haloToken, focusIdentity).toContain("color-mix");
    expect(sharedFocus.haloToken, focusIdentity).toContain("transparent");
    expect(sharedFocus.edgeColor, focusIdentity).not.toBe(
      sharedFocus.transparentColor,
    );
    expect(sharedFocus.edgeColor, focusIdentity).toBe(sharedFocus.resolvedEdge);
    expect(sharedFocus.halo, focusIdentity).not.toBe("none");
    expect(sharedFocus.halo, focusIdentity).toBe(sharedFocus.resolvedHalo);
    await settleTranscriptAtEnd(page);
    await expect(workbench).toHaveScreenshot("agent-keyboard-focus.png");
  });

  test("honors forced colors", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await page.emulateMedia({ forcedColors: "active" });
    const workbench = await openScenario(page, "agent-single", "agent");

    const focusedPaneHeader = page.locator(
      ".agent-chat-pane--focused .surface-header",
    );
    await expect(focusedPaneHeader).toHaveCount(1);
    const forcedFocus = await focusedPaneHeader.evaluate((node) => {
      const transparentProbe = document.createElement("span");
      transparentProbe.style.color = "transparent";
      document.body.append(transparentProbe);
      const transparentColor = getComputedStyle(transparentProbe).color;
      transparentProbe.remove();
      const style = getComputedStyle(node);
      return {
        outlineColor: style.outlineColor,
        outlineStyle: style.outlineStyle,
        outlineWidth: style.outlineWidth,
        shadow: style.boxShadow,
        transparentColor,
      };
    });
    expect(forcedFocus).toMatchObject({
      outlineStyle: "solid",
      outlineWidth: "2px",
      shadow: "none",
    });
    expect(forcedFocus.outlineColor).not.toBe(forcedFocus.transparentColor);

    const composerSelector =
      'textarea[aria-label="Message agent"]:not(:disabled)';
    const composerTextarea = page.locator(composerSelector);
    await expect(composerTextarea).toBeEnabled();
    await page.keyboard.press("ControlOrMeta+L");
    await expect(composerTextarea).toBeFocused();
    await expect(composerTextarea).toBeVisible();
    expect(
      await composerTextarea.evaluate((textarea) =>
        textarea.matches(":focus-visible"),
      ),
    ).toBe(true);
    const keyboardFocus = page.locator(":focus-visible");
    await expect(keyboardFocus).toHaveCount(1);
    expect(
      await keyboardFocus.evaluate(
        (focused, selector) => focused === document.querySelector(selector),
        composerSelector,
      ),
    ).toBe(true);
    await expect
      .poll(
        () =>
          keyboardFocus.evaluate((trigger) => {
            const owner = trigger.closest<HTMLElement>(".composer");
            if (!owner) return false;
            const borderProbe = document.createElement("span");
            borderProbe.style.border = "1px solid Highlight";
            owner.append(borderProbe);
            const target = getComputedStyle(borderProbe).borderTopColor;
            borderProbe.remove();
            return getComputedStyle(owner).borderTopColor === target;
          }),
        { timeout: 1_000 },
      )
      .toBe(true);
    const forcedKeyboardFocus = await composerTextarea.evaluate((trigger) => {
      const identity = (element: Element | null) => {
        if (!element) return "none";
        const id = element.id ? `#${element.id}` : "";
        const classes = [...element.classList]
          .map((className) => `.${className}`)
          .join("");
        return `${element.tagName.toLowerCase()}${id}${classes}`;
      };
      const owner = trigger.closest<HTMLElement>(".composer");
      const ownerStyle = owner ? getComputedStyle(owner) : null;
      const systemProbe = document.createElement("span");
      systemProbe.style.border = "1px solid Highlight";
      systemProbe.style.outline = "2px solid Highlight";
      (owner ?? document.body).append(systemProbe);
      const systemProbeStyle = getComputedStyle(systemProbe);
      const resolvedBorderHighlight = systemProbeStyle.borderTopColor;
      const resolvedOutlineHighlight = systemProbeStyle.outlineColor;
      systemProbe.style.borderColor = "transparent";
      systemProbe.style.outlineColor = "transparent";
      const transparentProbeStyle = getComputedStyle(systemProbe);
      const transparentBorder = transparentProbeStyle.borderTopColor;
      const transparentOutline = transparentProbeStyle.outlineColor;
      systemProbe.remove();
      return {
        borderColor: ownerStyle?.borderTopColor,
        borderStyle: ownerStyle?.borderTopStyle,
        borderWidth: ownerStyle?.borderTopWidth,
        focusWithinMatches: owner?.matches(":focus-within") ?? false,
        outlineColor: ownerStyle?.outlineColor,
        outlineOffset: ownerStyle?.outlineOffset,
        outlineStyle: ownerStyle?.outlineStyle,
        outlineWidth: ownerStyle?.outlineWidth,
        ownerContainsTrigger: owner?.contains(trigger) ?? false,
        ownerIdentity: identity(owner),
        resolvedBorderHighlight,
        resolvedOutlineHighlight,
        shadow: ownerStyle?.boxShadow,
        transparentBorder,
        transparentOutline,
        triggerIdentity: identity(trigger),
      };
    });
    const forcedIdentity = `trigger=${forcedKeyboardFocus.triggerIdentity}; owner=${forcedKeyboardFocus.ownerIdentity}`;
    expect(forcedKeyboardFocus, forcedIdentity).toMatchObject({
      borderStyle: "solid",
      borderWidth: "1px",
      focusWithinMatches: true,
      outlineOffset: "-2px",
      outlineStyle: "solid",
      outlineWidth: "2px",
      ownerContainsTrigger: true,
      shadow: "none",
    });
    expect(forcedKeyboardFocus.borderColor, forcedIdentity).toBe(
      forcedKeyboardFocus.resolvedBorderHighlight,
    );
    expect(forcedKeyboardFocus.outlineColor, forcedIdentity).toBe(
      forcedKeyboardFocus.resolvedOutlineHighlight,
    );
    expect(forcedKeyboardFocus.borderColor, forcedIdentity).not.toBe(
      forcedKeyboardFocus.transparentBorder,
    );
    expect(forcedKeyboardFocus.outlineColor, forcedIdentity).not.toBe(
      forcedKeyboardFocus.transparentOutline,
    );
    await keyboardFocus.evaluate((trigger) => (trigger as HTMLElement).blur());
    await expect(page.locator(":focus-visible")).toHaveCount(0);

    await settleTranscriptAtEnd(page);
    await expect(workbench).toHaveScreenshot("agent-forced-colors.png");
  });
});

test.describe("Wave 2 200% zoom state", () => {
  test.use({
    viewport: { width: 690, height: 440 },
    deviceScaleFactor: 2,
  });

  test("supports the Agent workbench at 200% zoom", async ({ page }) => {
    const workbench = await openScenario(page, "agent-dual", "agent");

    await expect(workbench).toHaveClass(/workbench--compact/);
    await expect
      .poll(() =>
        page.evaluate(() => ({
          devicePixelRatio: window.devicePixelRatio,
          innerWidth: window.innerWidth,
        })),
      )
      .toEqual({ devicePixelRatio: 2, innerWidth: 690 });
    await expect(
      page.getByRole("region", { name: "Agent chat", exact: true }),
    ).toBeVisible();
    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expect(
      page.getByRole("separator", { name: "Resize chat panes" }),
    ).toHaveCount(0);
    await expectNoViewportOverflow(page);
    await settleTranscriptAtEnd(page);
    await expect(workbench).toHaveScreenshot("agent-zoom-200.png");

    await openScenario(page, "agent-composer-narrow", "agent");
    await page.getByRole("button", { name: "Harness" }).click();
    const harnessInput = page.getByRole("combobox", { name: "Harness" });
    const harnessListbox = page.getByRole("listbox", { name: "Harness" });
    await expectOpenPickerContained(page, harnessInput, harnessListbox);
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: "Model" }).click();
    await expectOpenPickerContained(
      page,
      page.getByRole("combobox", { name: "Model" }),
      page.getByRole("listbox", { name: "Model" }),
    );
    await expectNoViewportOverflow(page);

    await openScenario(page, "agent-picker-handoff", "agent");
    const handoffTrigger = page
      .getByRole("region", { name: "Chat pane Source chat", exact: true })
      .getByRole("button", { name: "Hand off chat", exact: true });
    await expect(handoffTrigger).toHaveCount(1);
    await expect(handoffTrigger).toBeEnabled();
    await handoffTrigger.click();
    await page
      .getByRole("button", { name: "Target Chat", exact: true })
      .click();
    await expectOpenPickerContained(
      page,
      page.getByRole("combobox", { name: "Target Chat" }),
      page.getByRole("listbox", { name: "Target Chat" }),
    );
    await expectNoViewportOverflow(page);

    await openScenario(page, "agent-composer-narrow", "agent");
    const titleSearch = page
      .locator("header.title-bar")
      .getByRole("button", { name: "Search chats" });
    await titleSearch.click();
    const searchDialog = page.getByRole("dialog", { name: "Search Chats" });
    await expect(searchDialog).toBeVisible();
    const folderTrigger = searchDialog.getByRole("button", {
      name: "Search in",
    });
    await expect(folderTrigger).toHaveText("Sage");
    await folderTrigger.click();
    const folderInput = searchDialog.getByRole("combobox", {
      name: "Search in",
    });
    const folderListbox = searchDialog.getByRole("listbox", {
      name: "Search in",
    });
    await expect(folderInput).toBeFocused();
    await expect(folderListbox.getByRole("option")).toHaveCount(2);
    await expect(
      folderListbox.getByRole("option", { name: "Sage", exact: true }),
    ).toHaveAttribute("aria-selected", "true");
    await expect(
      searchDialog.getByText("Current: Sage", { exact: true }),
    ).toBeVisible();
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Enter");
    await expect(folderTrigger).toHaveText("All folders");
    await expect(folderTrigger).toBeFocused();
    await folderTrigger.click();
    await expectOpenPickerContained(page, folderInput, folderListbox);
    await expectNoViewportOverflow(page);
    await page.keyboard.press("Escape");
    await expect(searchDialog).toBeVisible();
    await expect(folderTrigger).toBeFocused();
    await searchDialog.getByRole("button", { name: "Cancel" }).click();
    await expect(searchDialog).toHaveCount(0);
    await expect(titleSearch).toBeFocused();
    await expectNoViewportOverflow(page);
  });

  test("keeps Agent activity cards bounded at 200% zoom", async ({ page }) => {
    await openScenario(page, "agent-activity-family", "agent");

    await expect
      .poll(() =>
        page
          .locator(".acp-activity-card")
          .evaluateAll((cards) =>
            cards.every(
              (card) =>
                card.clientWidth > 0 && card.scrollWidth <= card.clientWidth,
            ),
          ),
      )
      .toBe(true);
    await expectNoViewportOverflow(page);
  });

  test("keeps an Agent image contained at 200% zoom", async ({ page }) => {
    await openScenario(page, "agent-image", "agent");

    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expect
      .poll(() =>
        page.evaluate(() => ({
          devicePixelRatio: window.devicePixelRatio,
          innerWidth: window.innerWidth,
        })),
      )
      .toEqual({ devicePixelRatio: 2, innerWidth: 690 });
    await expectAgentImageContained(page);
    await expectNoViewportOverflow(page);
  });

  test("keeps four child-activity levels contained at 200% zoom", async ({
    page,
  }) => {
    await openScenario(page, "agent-child-activity", "agent");
    await expandChildActivityHierarchy(page);

    await expect(page.locator(".agent-chat-pane")).toHaveCount(1);
    await expect
      .poll(() =>
        page.evaluate(() => ({
          devicePixelRatio: window.devicePixelRatio,
          innerWidth: window.innerWidth,
        })),
      )
      .toEqual({ devicePixelRatio: 2, innerWidth: 690 });
    await expectChildActivityContained(page);
    await expectNoViewportOverflow(page);
  });
});
