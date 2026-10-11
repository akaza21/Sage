import { expect, test, type Locator, type Page } from "@playwright/test";

import { AGENT_VISUAL_SCENARIOS } from "./fixtures/visualScenarioTypes";
import { VISUAL_SCENARIOS } from "./fixtures/visualScenarioCatalog";
import {
  expectNoViewportOverflow,
  openScenario,
  settleChatProjections,
  settleTranscriptAtEnd,
  settleTranscriptAtStart,
  takeCanonicalScreenshot,
} from "./shared/visualBrowserPrimitives";
import {
  expandChildActivityHierarchy,
  expectAgentImageContained,
  expectChildActivityContained,
  expectFocusedChatIndicator,
  openComposerOptions,
  releasePickerHarnessReadiness,
  runAgentCanonicalTest,
  revealWorkedContent,
} from "./agentVisualHelpers";

async function expectOpenPickerContained(
  page: Page,
  input: Locator,
  choices: Locator,
) {
  const popup = input.locator(
    "xpath=ancestor::div[contains(concat(' ', normalize-space(@class), ' '), ' menu-popover ') or contains(concat(' ', normalize-space(@class), ' '), ' composer__popover ')][1]",
  );
  await expect(input).toBeFocused();
  await expect(popup).toBeVisible();
  await expect(choices).toBeVisible();
  const [inputBox, choicesBox, popupBox, viewport] = await Promise.all([
    input.boundingBox(),
    choices.boundingBox(),
    popup.boundingBox(),
    page.evaluate(() => ({
      height: window.innerHeight,
      width: window.innerWidth,
    })),
  ]);
  expect(inputBox).not.toBeNull();
  expect(choicesBox).not.toBeNull();
  expect(popupBox).not.toBeNull();
  if (!inputBox || !choicesBox || !popupBox) return;
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
  expect(choicesBox.width).toBeGreaterThan(0);
  expect(choicesBox.height).toBeGreaterThan(0);
  expect(popupBox.width).toBeGreaterThan(0);
  expect(popupBox.height).toBeGreaterThan(0);
  expect(inside(inputBox, popupBox)).toBe(true);
  expect(inside(choicesBox, popupBox)).toBe(true);
  expect(inside(popupBox, viewportBox)).toBe(true);
}

async function expectContextUsageRing(progress: Locator, percentage: number) {
  await expect(progress).toBeVisible();
  const ring = await progress.evaluate((element) => {
    const style = getComputedStyle(element, "::before");
    const box = element.getBoundingClientRect();
    return {
      fill: getComputedStyle(element).getPropertyValue(
        "--composer-context-usage",
      ),
      gradient: style.backgroundImage,
      mask: style.maskImage,
      width: box.width,
      height: box.height,
      ringWidth: style.width,
      ringHeight: style.height,
    };
  });
  expect(ring.width).toBe(24);
  expect(ring.height).toBe(24);
  expect(ring.ringWidth).toBe("16px");
  expect(ring.ringHeight).toBe("16px");
  expect(ring.fill).toMatch(/%$/);
  expect(Number.parseFloat(ring.fill)).toBeCloseTo(percentage, 5);
  expect(ring.gradient).toContain("conic-gradient");
  expect(ring.mask).toContain("radial-gradient");
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
    form.getByRole("button", { name: "Composer options", exact: true }),
    form.getByRole("button", { name: "Add images", exact: true }),
    form.locator(".composer__access-label"),
    form.getByRole("button", { name: "Harness", exact: true }),
    form.getByRole("button", { name: "Model", exact: true }),
    form.getByRole("button", { name: "Start dictation", exact: true }),
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

async function expectComposerToolbarLaidOut(form: Locator) {
  const toolbar = form.locator(".composer__toolbar");
  await expect(toolbar).toBeVisible();
  const geometry = await toolbar.evaluate((node) => {
    const toolbarBox = node.getBoundingClientRect();
    const buttons = [
      ...node.querySelectorAll<HTMLButtonElement>("button"),
    ].filter((button) => button.checkVisibility());
    const harness = node.querySelector<HTMLElement>(".composer__harness")!;
    const trigger = harness.querySelector<HTMLButtonElement>("button")!;
    const wrapperBox = harness.getBoundingClientRect();
    const triggerBox = trigger.getBoundingClientRect();
    const composer = node.closest<HTMLElement>("form.composer")!;
    const composerStyle = getComputedStyle(composer);
    return {
      composerWidth:
        composer.clientWidth -
        Number.parseFloat(composerStyle.paddingLeft) -
        Number.parseFloat(composerStyle.paddingRight),
      overflow: node.scrollWidth - node.clientWidth,
      harnessContained:
        triggerBox.left >= wrapperBox.left - 1 &&
        triggerBox.right <= wrapperBox.right + 1 &&
        triggerBox.top >= wrapperBox.top - 1 &&
        triggerBox.bottom <= wrapperBox.bottom + 1,
      buttons: buttons.map((button) => {
        const box = button.getBoundingClientRect();
        return {
          name: button.getAttribute("aria-label"),
          left: box.left,
          right: box.right,
          top: box.top,
          bottom: box.bottom,
          width: box.width,
          height: box.height,
          center: box.top + box.height / 2,
          contained:
            box.left >= toolbarBox.left - 1 &&
            box.right <= toolbarBox.right + 1 &&
            box.top >= toolbarBox.top - 1 &&
            box.bottom <= toolbarBox.bottom + 1,
        };
      }),
    };
  });
  expect(geometry.buttons).toHaveLength(7);
  expect(geometry.overflow).toBeLessThanOrEqual(1);
  expect(
    geometry.harnessContained,
    "Harness trigger stays within its reserved wrapper",
  ).toBe(true);
  for (const button of geometry.buttons) {
    expect(
      button.width,
      button.name ?? "toolbar button",
    ).toBeGreaterThanOrEqual(20);
    expect(button.height).toBeGreaterThan(0);
    expect(button.contained, button.name ?? "toolbar button").toBe(true);
  }
  const trailing = ["Start dictation", "Send message"];
  const rows =
    geometry.composerWidth <= 280
      ? [
          geometry.buttons.filter(
            (button) => !trailing.includes(String(button.name)),
          ),
          geometry.buttons.filter((button) =>
            trailing.includes(String(button.name)),
          ),
        ]
      : [geometry.buttons];
  for (const row of rows) {
    expect(row.length).toBeGreaterThan(0);
    const centers = row.map((button) => button.center);
    expect(Math.max(...centers) - Math.min(...centers)).toBeLessThan(2);
  }
  if (rows.length === 2) {
    expect(
      Math.min(...rows[1].map((button) => button.top)),
    ).toBeGreaterThanOrEqual(
      Math.max(...rows[0].map((button) => button.bottom)),
    );
  }
  for (const [index, first] of geometry.buttons.entries()) {
    for (const second of geometry.buttons.slice(index + 1)) {
      const overlap =
        first.left < second.right &&
        second.left < first.right &&
        first.top < second.bottom &&
        second.top < first.bottom;
      expect(
        overlap,
        String(first.name) + " does not overlap " + String(second.name),
      ).toBe(false);
    }
  }
  expect(
    await form.evaluate((node) => node.scrollWidth - node.clientWidth),
  ).toBeLessThanOrEqual(1);
}

async function expectPickerLabelUnclipped(label: Locator) {
  await expect(label).toBeVisible();
  expect(
    await label.evaluate((node) => node.clientWidth >= node.scrollWidth),
  ).toBe(true);
}

async function expectComposerPickerValue(
  picker: Locator,
  name: "Harness" | "Model" | "Session mode",
  value: string,
) {
  await expect(picker).toHaveAccessibleName(name);
  await expect(picker).toHaveText(value);
  const label = picker.locator(".searchable-picker__trigger-label");
  if (await label.count()) {
    await expectPickerLabelUnclipped(label);
  }
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
  const form = page.locator("form.composer").first();
  await expect(form).toBeVisible();
  const formBox = await form.boundingBox();
  expect(formBox).not.toBeNull();
  expect(formBox!.width, width + "px Chat column inset").toBeLessThan(width);
  await expect(
    form.getByRole("button", { name: "Harness", exact: true }),
  ).toContainText(expectedLabels[0]!);
  const harnessLabel = form
    .getByRole("button", { name: "Harness", exact: true })
    .locator(".searchable-picker__trigger-label");
  await expectPickerLabelUnclipped(harnessLabel);
  await expect(
    form.getByRole("button", { name: "Model", exact: true }),
  ).toHaveText(expectedLabels[1]!);
  await expectComposerControlsLaidOut(page);
  await expectComposerToolbarLaidOut(form);
  const options = await openComposerOptions(page);
  await expect(
    options.getByRole("button", { name: "Session mode", exact: true }),
  ).toHaveText(expectedLabels[2]!);
  await expect(
    options.getByRole("button", { name: "Permission mode", exact: true }),
  ).toHaveText(expectedLabels[3]!);
  await page.keyboard.press("Escape");
  await expect(
    form.getByRole("button", { name: "Composer options", exact: true }),
  ).toBeFocused();
}

test.describe("Wave 2 canonical visual states", () => {
  test.use({ viewport: { width: 1380, height: 880 } });

  for (const { name, mode } of VISUAL_SCENARIOS.filter(
    (scenario) =>
      AGENT_VISUAL_SCENARIOS.includes(scenario.name) &&
      ![
        "rich",
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
    await revealWorkedContent(rollup);
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
    await revealWorkedContent(rollup);
    const label = rollup.locator(".acp-tool-rollup__label");
    await expect(label).toHaveText("Ran 2 commands");
    await expect(
      rollup.locator('[data-activity-icon="execute"]'),
    ).toBeVisible();
    const geometry = await label.evaluate((node) => {
      const summary = node.closest("summary")!;
      const card = summary.closest(".acp-tool-rollup")!;
      const style = getComputedStyle(node);
      const lineHeight = Number.parseFloat(style.lineHeight);
      return {
        labelHeight: node.getBoundingClientRect().height,
        lineHeight: Number.isFinite(lineHeight)
          ? lineHeight
          : Number.parseFloat(style.fontSize) * 1.5,
        summaryWidth: summary.getBoundingClientRect().width,
        summaryHeight: summary.getBoundingClientRect().height,
        cardOverflow: card.scrollWidth - card.clientWidth,
        labelOverflow: node.scrollWidth - node.clientWidth,
      };
    });
    expect(geometry.summaryWidth).toBeLessThan(520);
    expect(geometry.labelHeight).toBeLessThanOrEqual(geometry.lineHeight * 1.5);
    expect(geometry.summaryHeight).toBeLessThanOrEqual(geometry.lineHeight * 2);
    expect(geometry.cardOverflow).toBeLessThanOrEqual(1);
    expect(geometry.labelOverflow).toBeLessThanOrEqual(1);
    await rollup.locator("summary").click();
    await expect(rollup.locator(".acp-tool-rollup__items")).toBeVisible();
    await expect(rollup).toContainText(
      "powershell -NoProfile -Command 'Get-Content -Path alpha.txt -TotalCount 40'",
    );
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
    const listbox = page.getByRole("listbox", { name: "Harness" });
    await expect(listbox.getByRole("option").first()).toBeFocused();
    await expect(
      listbox.getByRole("option", { name: "Codex — test only", exact: true }),
    ).toHaveAttribute("aria-selected", "true");
    const unavailable = listbox.getByRole("option", {
      name: /Unavailable Codex.*Sage did not admit this harness and will not start it\./,
    });
    await expect(unavailable).toHaveAttribute("aria-disabled", "true");
    await expect(page.getByRole("combobox", { name: "Harness" })).toHaveCount(
      0,
    );
    await expect(unavailable).toBeDisabled();
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
    await releasePickerHarnessReadiness(page);
    await expect(modelTrigger).toBeEnabled();
    await modelTrigger.click();
    const modelListbox = page.getByRole("listbox", {
      name: "Select model",
      exact: true,
    });
    await expect(modelListbox.getByRole("option")).toHaveCount(128);
    await expect(
      modelListbox.getByRole("option", {
        name: "Fresh advertised model 1",
        exact: true,
      }),
    ).toBeVisible();
    await expect(modelListbox.locator('[aria-selected="true"]')).toHaveCount(0);
    await expect(
      modelListbox.getByRole("option", { name: /Old advertised model/ }),
    ).toHaveCount(0);
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Enter");
    await expect(modelTrigger).toHaveText("Model");
    await expect(page.locator("html")).toHaveAttribute(
      "data-visual-model-selection-count",
      "1",
    );
    await modelTrigger.click();
    const selectedModel = modelListbox.getByRole("option", {
      name: "Fresh advertised model 2",
      exact: true,
    });
    await expect(selectedModel).toHaveAttribute("aria-selected", "true");
    await expect(modelListbox.locator('[aria-selected="true"]')).toHaveCount(1);
    await expectOpenPickerContained(page, selectedModel, modelListbox);
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
    const taskActions = page
      .locator(".agent-task-header")
      .getByRole("button", { name: /^Chat actions for / });
    await taskActions.click();
    const handoffTrigger = page.getByRole("menuitem", {
      name: "Hand off chat",
      exact: true,
    });
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
    await expect(taskActions).toBeFocused();
    await expect(page.locator("html")).toHaveAttribute(
      "data-visual-handoff-dispatch-count",
      "0",
    );
    expect(
      await page.evaluate(async () => {
        const { useWorkbenchStore } = await import("/src/app/store.ts");
        return useWorkbenchStore.getState().chatUiById[
          "visual-chat-handoff-source"
        ]?.composerText;
      }),
    ).toBe("");
    await expect(page.locator(".agent-task-header__title")).toHaveText(
      "Target chat",
    );
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
    await expectComposerPickerValue(harnessTrigger, "Harness", "Codex");
    await expectComposerPickerValue(
      page.getByRole("button", { name: "Model", exact: true }),
      "Model",
      "Model",
    );
    await harnessTrigger.click();
    const listbox = page.getByRole("listbox", { name: "Harness" });
    const input = listbox.getByRole("option", {
      name: "Codex — test only",
      exact: true,
    });
    await expectOpenPickerContained(page, input, listbox);
    await expect(
      listbox.getByRole("option", { name: "Codex — test only", exact: true }),
    ).toHaveAttribute("aria-selected", "true");
    await expectNoViewportOverflow(page);
    await page.keyboard.press("Escape");
    await expect(harnessTrigger).toBeFocused();
    const modelTrigger = page.getByRole("button", { name: "Model" });
    await modelTrigger.click();
    await expectOpenPickerContained(
      page,
      page.getByRole("option", { name: "GPT-5", exact: true }),
      page.getByRole("listbox", { name: "Select model", exact: true }),
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
    const closedOptions = await openComposerOptions(page);
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
    const projectTrigger = closedOptions.getByRole("button", {
      name: "Project",
      exact: true,
    });
    await expect(projectTrigger).toBeEnabled();
    await expect(projectTrigger).toHaveAttribute("aria-haspopup", "listbox");
    await page
      .getByRole("button", { name: "Composer options", exact: true })
      .click();
    await expect(closedOptions).toHaveCount(0);
    await expectComposerControlRow(page);
    await takeCanonicalScreenshot(closedWorkbench, "agent-session-mode-closed");

    const openWorkbench = await openScenario(
      page,
      "agent-session-mode-open",
      "agent",
    );
    await openComposerOptions(page);
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
    await openComposerOptions(page);
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
    await openComposerOptions(page);
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
    await settleChatProjections(page);
    const unknownContext = page.getByLabel("Composer context", { exact: true });
    await expect(
      page.getByRole("textbox", { name: "Message agent" }),
    ).toBeVisible();
    await expect(unknownContext).toHaveCount(0);
    await expect(
      page.locator("form.composer").getByRole("progressbar"),
    ).toHaveCount(0);
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
    await settleChatProjections(page);
    const knownContext = page.getByLabel("Composer context", { exact: true });
    const knownProgress = knownContext.getByRole("progressbar", {
      name: "6,000 used / 272,000 total tokens",
    });
    await expect(knownContext).toContainText("6K context");
    await expect(
      knownContext.locator(".composer__context-usage-label"),
    ).toHaveText("6K context");
    await expect(knownProgress).toHaveCount(1);
    await expect(knownProgress).toHaveAttribute("aria-valuemin", "0");
    await expect(knownProgress).toHaveAttribute("aria-valuemax", "272000");
    await expect(knownProgress).toHaveAttribute("aria-valuenow", "6000");
    await expect(knownProgress).toHaveAttribute("tabindex", "0");
    await expect(knownProgress).toHaveAttribute(
      "title",
      "6k / 272k tokens · 2%",
    );
    await expect(knownProgress).toHaveAttribute(
      "aria-valuetext",
      "6,000 / 272,000 tokens used",
    );
    await expectContextUsageRing(knownProgress, 2.205882);
    expect(
      await knownContext.evaluate((node) =>
        Boolean(node.closest(".composer__toolbar")),
      ),
    ).toBe(true);
    const knownContextBox = await knownContext.boundingBox();
    const knownToolbarBox = await page
      .locator("form.composer .composer__toolbar")
      .first()
      .boundingBox();
    expect(knownContextBox).not.toBeNull();
    expect(knownToolbarBox).not.toBeNull();
    expect(knownContextBox?.y ?? -1).toBeGreaterThanOrEqual(
      knownToolbarBox?.y ?? 0,
    );
    expect(
      (knownContextBox?.y ?? 0) + (knownContextBox?.height ?? 0),
    ).toBeLessThanOrEqual(
      (knownToolbarBox?.y ?? 0) + (knownToolbarBox?.height ?? 0) + 1,
    );
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
    await settleChatProjections(page);
    const overCapacityContext = page.getByLabel("Composer context", {
      exact: true,
    });
    const overCapacityProgress = overCapacityContext.getByRole("progressbar", {
      name: "300,000 used / 272,000 total tokens",
    });
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
      "300k / 272k tokens · 110%",
    );
    await expectContextUsageRing(overCapacityProgress, 100);
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
      const ring = getComputedStyle(element, "::before");
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
        gradient: ring.backgroundImage,
        highlight,
        mask: ring.maskImage,
        trackBorder: ring.borderTopColor,
        trackBorderStyle: ring.borderTopStyle,
        trackBorderWidth: ring.borderTopWidth,
      };
    });
    expect(forcedColors.gradient).toContain("conic-gradient");
    expect(forcedColors.gradient).toContain(forcedColors.highlight);
    expect(forcedColors.gradient).toContain(forcedColors.canvas);
    expect(forcedColors.mask).toBe("none");
    expect(forcedColors.trackBorder).toBe(forcedColors.canvasText);
    expect(forcedColors.trackBorderStyle).toBe("solid");
    expect(forcedColors.trackBorderWidth).toBe("1px");
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
    const header = page.locator(".agent-task-header");
    await expect(panes).toHaveCount(2);
    await expectFocusedChatIndicator(page);
    await expect(header).toHaveCount(1);
    await expect(panes.locator(".surface-header")).toHaveCount(0);
    await expect(header.locator(".agent-task-header__title")).toHaveText(
      "Agent navigation polish",
    );
    const chatSplitter = page.getByRole("separator", {
      name: "Resize chat panes",
    });
    await expect(chatSplitter).toHaveCount(1);
    const sidebarSplitter = page.locator(".workbench-sidebar-splitter");
    await expect(sidebarSplitter).toHaveCount(1);
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
    const primaryDictation = panes.nth(0).getByRole("button", {
      name: "Start dictation",
      exact: true,
    });
    await expect(primaryDictation).toBeEnabled();
    await expect(
      panes.nth(0).getByRole("button", { name: "Send message" }),
    ).toBeDisabled();
    const waitingActivity = panes.nth(1).locator(".acp-tool-rollup > summary");
    await expect(waitingActivity).toBeVisible();
    await primaryDictation.focus();
    await page.keyboard.press("Tab");
    await expect(chatSplitter).toBeFocused();
    await page.keyboard.press("Shift+Tab");
    await expect(primaryDictation).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(chatSplitter).toBeFocused();
    expect(
      await chatSplitter.evaluate((node) => ({
        focusVisible: node.matches(":focus-visible"),
        lineWidth: getComputedStyle(node, "::after").width,
      })),
    ).toEqual({ focusVisible: true, lineWidth: "1px" });
    const waitingGoal = panes.nth(1).locator(".chat-goal-entry > summary");
    const waitingCancel = panes
      .nth(1)
      .getByRole("button", { name: "Cancel turn", exact: true });
    await expect(waitingGoal).toBeVisible();
    await expect(waitingCancel).toBeEnabled();
    await page.keyboard.press("Tab");
    await expect(panes.nth(1)).toHaveClass(/agent-chat-pane--focused/);
    expect(
      await panes
        .nth(1)
        .evaluate((node) => node.contains(document.activeElement)),
    ).toBe(true);
    await expect(waitingActivity).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(waitingGoal).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(waitingCancel).toBeFocused();
    await page.keyboard.press("Shift+Tab");
    await expect(waitingGoal).toBeFocused();
    await chatSplitter.focus();
    await expect(chatSplitter).toBeFocused();
    await chatSplitter.evaluate((node) => (node as HTMLElement).blur());
    await page
      .getByRole("button", { name: /^Open chat Agent navigation polish/ })
      .click();
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
      const primary = workbench.chatUiById["visual-chat-primary"];
      useWorkbenchStore.setState({
        chatUiById: {
          ...workbench.chatUiById,
          "visual-chat-primary": {
            ...primary,
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
    await page.keyboard.press("Home");
    await expect(chatSplitter).toHaveAttribute("aria-valuenow", "30");
    for (const owner of await panes.all()) {
      await expectComposerToolbarLaidOut(owner.locator("form.composer"));
    }
    const narrowPane = panes.nth(0);
    const optionsTrigger = narrowPane.getByRole("button", {
      name: "Composer options",
      exact: true,
    });
    await optionsTrigger.click();
    const panelId = await optionsTrigger.getAttribute("aria-controls");
    const options = page.locator(`[id="${panelId}"]`);
    await expect(options).toHaveAttribute("role", "dialog");
    await expect(options).toHaveAttribute(
      "aria-label",
      "Composer options menu",
    );
    await expect(
      options.getByRole("button", { name: "Session mode" }),
    ).toHaveText("Review changes");
    await expect(options.getByRole("alert")).toHaveText(
      "Mode update conflicted",
    );
    await options.getByRole("button", { name: "Session mode" }).click();
    await expect(
      page.getByRole("combobox", { name: "Session mode" }),
    ).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(
      options.getByRole("button", { name: "Session mode" }),
    ).toBeFocused();
    await expect(options).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(options).toHaveCount(0);
    await expect(optionsTrigger).toBeFocused();
    const measurePanes = () =>
      panes.evaluateAll((items) =>
        items.map((item) => {
          const box = item.getBoundingClientRect();
          return [box.x, box.y, box.width, box.height];
        }),
      );
    await page
      .getByRole("button", { name: /^Open chat Agent navigation polish/ })
      .click();
    const before = await measurePanes();
    const waitingPane = panes.nth(1);
    await expect(
      waitingPane.getByRole("textbox", { name: "Message agent" }),
    ).toBeDisabled();
    await waitingCancel.focus();
    await expect(waitingCancel).toBeFocused();
    await expect(waitingPane).toHaveClass(/agent-chat-pane--focused/);
    await expectFocusedChatIndicator(page);
    await expect(header.locator(".agent-task-header__title")).toHaveText(
      "Editor consistency pass",
    );
    await page.keyboard.press("Shift+Tab");
    await expect(waitingGoal).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(waitingCancel).toBeFocused();
    expect(await measurePanes()).toEqual(before);
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
    await settleTranscriptAtStart(page);
    await expect(workbench).toHaveScreenshot("agent-keyboard-focus.png");
  });

  test("honors forced colors", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await page.emulateMedia({ forcedColors: "active" });
    const workbench = await openScenario(page, "agent-single", "agent");

    await expect(page.locator(".agent-chat-pane--focused")).toHaveCount(1);
    await expect(page.locator(".agent-task-header")).toBeVisible();

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

    await settleTranscriptAtStart(page);
    await expect(workbench).toHaveScreenshot("agent-forced-colors.png");
    await openScenario(page, "agent-dual", "agent");
    await expectFocusedChatIndicator(page, true);
    await page
      .locator(".agent-chat-pane")
      .nth(1)
      .getByRole("button", { name: "Cancel turn", exact: true })
      .focus();
    await expect(page.locator(".agent-chat-pane").nth(1)).toHaveClass(
      /agent-chat-pane--focused/,
    );
    await expectFocusedChatIndicator(page, true);
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
    const harnessListbox = page.getByRole("listbox", { name: "Harness" });
    const harnessInput = harnessListbox.getByRole("option", {
      name: "Codex — test only",
      exact: true,
    });
    await expectOpenPickerContained(page, harnessInput, harnessListbox);
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: "Model" }).click();
    await expectOpenPickerContained(
      page,
      page.getByRole("option", { name: "GPT-5", exact: true }),
      page.getByRole("listbox", { name: "Select model" }),
    );
    await expectNoViewportOverflow(page);

    await openScenario(page, "agent-picker-handoff", "agent");
    await page
      .locator(".agent-task-header")
      .getByRole("button", { name: /^Chat actions for / })
      .click();
    const handoffTrigger = page.getByRole("menuitem", {
      name: "Hand off chat",
      exact: true,
    });
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
