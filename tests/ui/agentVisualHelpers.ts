import { expect, type Locator, type Page } from "@playwright/test";

import type {
  VisualScenarioMode,
  VisualScenarioName,
} from "./fixtures/visualScenarioTypes";
import {
  expectNoViewportOverflow,
  openScenario,
  settleTranscriptAtEnd,
  takeCanonicalScreenshot,
} from "./shared/visualBrowserPrimitives";

export async function assertAgentLandmarks(page: Page) {
  const projectSidebar = page.getByRole("complementary", {
    name: "Projects and chats",
  });
  await expect(
    projectSidebar.getByRole("button", {
      name: "New task",
      exact: true,
    }),
  ).toBeVisible();
  await expect(
    projectSidebar.getByRole("button", { name: "Filter tasks", exact: true }),
  ).toBeVisible();
}

export async function expectLiveThinkingFixtureLine(page: Page) {
  const thought = page.locator('[data-activity-kind="thought"]');
  await expect(thought).toHaveCount(1);
  await expect(thought).toHaveClass("acp-thought");
  await expect(thought).toHaveText(
    `Reviewing ${"updated workspace state ".repeat(20)}`.slice(0, 256),
  );
  await expect(thought.locator("summary, button, .acp-state")).toHaveCount(0);
  expect(
    await thought.evaluate((node) => {
      const element = node as HTMLElement;
      const style = getComputedStyle(element);
      const color = style.color;
      const originalColor = element.style.color;
      element.style.color = "var(--color-text-muted)";
      const mutedColor = getComputedStyle(element).color;
      element.style.color = originalColor;
      return {
        muted: color === mutedColor,
        clipped: element.scrollWidth > element.clientWidth,
        overflow: style.overflow,
        textOverflow: style.textOverflow,
        whiteSpace: style.whiteSpace,
        animation: style.animationName,
      };
    }),
  ).toEqual({
    muted: true,
    clipped: true,
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
    animation: "none",
  });
}

export async function releasePickerHarnessReadiness(page: Page) {
  await page.evaluate(() => {
    const visualWindow = window as typeof window & {
      __releaseVisualPickerHarnessReady?: () => void;
    };
    const release = visualWindow.__releaseVisualPickerHarnessReady;
    if (!release) throw new Error("Harness readiness fixture was not armed");
    release();
  });
}

export async function revealWorkedContent(content: Locator) {
  // Collapsed completed Turns do not mount their Worked groups; open each
  // closed Worked disclosure until the requested content mounts.
  const page = content.page();
  await expect(
    content.or(page.locator("details.chat-transcript-work")).first(),
  ).toBeAttached();
  if ((await content.count()) === 0) {
    const closed = page.locator("details.chat-transcript-work:not([open])");
    for (let opened = 0; opened < 20 && (await content.count()) === 0;) {
      if ((await closed.count()) === 0) break;
      await closed.last().locator(":scope > summary").click();
      opened += 1;
    }
  }
  await expect(content).toHaveCount(1);
  const worked = content.locator(
    "xpath=ancestor::details[contains(@class, 'chat-transcript-work')][1]",
  );
  // A collapsed completed Turn shows its tool rows beside (not inside) the
  // Worked disclosure; opening Worked swaps them for the expanded copy.
  const owner = (await worked.count())
    ? worked
    : content
        .locator(
          "xpath=ancestor::section[contains(concat(' ', normalize-space(@class), ' '), ' acp-turn ')][1]",
        )
        .locator("details.chat-transcript-work");
  if ((await owner.count()) === 1) {
    if ((await owner.getAttribute("open")) === null) {
      await owner.locator(":scope > summary").click();
    }
    await expect(owner).toHaveAttribute("open", "");
  }
  await expect(content).toBeVisible();
}

export async function openComposerOptions(page: Page) {
  const form = page.locator("form.composer").first();
  const trigger = form.getByRole("button", {
    name: "Composer options",
    exact: true,
  });
  await expect(trigger).toBeVisible();
  await expect(trigger).toHaveAttribute("aria-expanded", "false");
  await trigger.click();
  const panelId = await trigger.getAttribute("aria-controls");
  const menu = page.locator(`[id="${panelId}"]`);
  await expect(menu).toHaveAttribute("role", "menu");
  await expect(menu).toHaveAttribute("aria-label", "Composer options menu");
  await expect(menu).toBeVisible();
  await expect(trigger).toHaveAttribute("aria-expanded", "true");
  return menu;
}

export async function expectFocusedChatIndicator(
  page: Page,
  forcedColors = false,
) {
  const panes = page.locator(".agent-chat-panes--split .agent-chat-pane");
  await expect(panes).toHaveCount(2);
  const focused = panes.and(page.locator(".agent-chat-pane--focused"));
  await expect(focused).toHaveCount(1);
  await expect(focused).toBeVisible();
  expect(
    await focused.evaluate((node, forced) => {
      const style = getComputedStyle(node);
      const probe = document.createElement("span");
      probe.style.outline = forced
        ? "2px solid Highlight"
        : "1px solid var(--color-border-strong)";
      node.append(probe);
      const expectedColor = getComputedStyle(probe).outlineColor;
      probe.remove();
      return {
        matchesExpectedColor: style.outlineColor === expectedColor,
        style: style.outlineStyle,
        width: style.outlineWidth,
        offset: style.outlineOffset,
      };
    }, forcedColors),
  ).toEqual({
    matchesExpectedColor: true,
    style: "solid",
    width: forcedColors ? "2px" : "1px",
    offset: forcedColors ? "-2px" : "-1px",
  });
  await expect(
    panes.and(page.locator(":not(.agent-chat-pane--focused)")),
  ).toHaveCSS("outline-style", "none");
}

export async function expectAgentImageContained(page: Page) {
  await revealWorkedContent(page.locator(".acp-image"));
  const image = page.getByRole("img", {
    name: "Image provided by the agent",
  });
  await expect(image).toBeVisible();
  await expect
    .poll(() =>
      image.evaluate(
        (node) =>
          (node as HTMLImageElement).complete &&
          (node as HTMLImageElement).naturalWidth === 1_200 &&
          (node as HTMLImageElement).naturalHeight === 675,
      ),
    )
    .toBe(true);
  await image.evaluate((node) => (node as HTMLImageElement).decode());
  expect(
    await image.evaluate((node) => {
      const element = node as HTMLImageElement;
      const figure = element.closest<HTMLElement>(".acp-image");
      const transcript = element.closest<HTMLElement>(".message-list");
      if (!figure || !transcript) return null;
      const imageBounds = element.getBoundingClientRect();
      const figureBounds = figure.getBoundingClientRect();
      const transcriptBounds = transcript.getBoundingClientRect();
      const naturalRatio = element.naturalWidth / element.naturalHeight;
      const renderedRatio = imageBounds.width / imageBounds.height;
      return {
        figureOwnsOverflow: figure.scrollWidth <= figure.clientWidth,
        imageInsideFigure:
          imageBounds.left >= figureBounds.left &&
          imageBounds.right <= figureBounds.right &&
          imageBounds.top >= figureBounds.top &&
          imageBounds.bottom <= figureBounds.bottom,
        imageInsideTranscript:
          imageBounds.left >= transcriptBounds.left &&
          imageBounds.right <= transcriptBounds.right,
        heightBounded:
          imageBounds.height <= Math.min(window.innerHeight * 0.6, 520) + 1,
        ratioPreserved: Math.abs(naturalRatio - renderedRatio) < 0.01,
      };
    }),
  ).toEqual({
    figureOwnsOverflow: true,
    imageInsideFigure: true,
    imageInsideTranscript: true,
    heightBounded: true,
    ratioPreserved: true,
  });
}

export async function expandChildActivityHierarchy(page: Page) {
  const activity = page.getByRole("group", {
    name: "Harness activity: Accessibility review activity",
  });
  await expect(activity).toBeVisible();
  await activity.locator("summary").click();
  const tree = activity.getByRole("tree", {
    name: "Accessibility review activity activity items",
  });

  for (let level = 1; level <= 4; level += 1) {
    const item = tree.locator(`[role="treeitem"][aria-level="${level}"]`);
    await expect(item).toBeVisible();
    await item.focus();
    await expect(item).toBeFocused();
    if (level < 4) {
      await expect(item).toHaveAttribute("aria-expanded", "false");
      await page.keyboard.press("ArrowRight");
      await expect(item).toHaveAttribute("aria-expanded", "true");
      await page.keyboard.press("ArrowRight");
      await expect(
        tree.locator(`[role="treeitem"][aria-level="${level + 1}"]`),
      ).toBeFocused();
    } else {
      await expect(item).not.toHaveAttribute("aria-expanded");
    }
  }

  await expect(activity.getByRole("button")).toHaveCount(0);
  return { activity, tree };
}

export async function expectChildActivityContained(page: Page) {
  const tree = page.getByRole("tree", {
    name: "Accessibility review activity activity items",
  });
  const labels = tree.locator(".acp-collaboration__label");
  await expect(labels).toHaveCount(4);
  expect(
    await tree.evaluate((node) => {
      const transcript = node.closest<HTMLElement>(".message-list");
      const labelNodes = [
        ...node.querySelectorAll<HTMLElement>(".acp-collaboration__label"),
      ];
      if (!transcript || labelNodes.length !== 4) return null;
      const transcriptBounds = transcript.getBoundingClientRect();
      const labelLeftEdges = labelNodes.map(
        (label) => label.getBoundingClientRect().left,
      );
      return {
        treeOwnsOverflow: node.scrollWidth <= node.clientWidth,
        labelsInsideTranscript: labelNodes.every((label) => {
          const bounds = label.getBoundingClientRect();
          return (
            bounds.left >= transcriptBounds.left &&
            bounds.right <= transcriptBounds.right
          );
        }),
        levelsStepInward: labelLeftEdges.every(
          (left, index) => index === 0 || left > labelLeftEdges[index - 1],
        ),
      };
    }),
  ).toEqual({
    treeOwnsOverflow: true,
    labelsInsideTranscript: true,
    levelsStepInward: true,
  });
}

export async function runAgentCanonicalTest(
  page: Page,
  name: VisualScenarioName,
  mode: VisualScenarioMode,
) {
  if (name === "agent-activity-family") {
    await page.setViewportSize({ width: 1380, height: 1200 });
  }
  const workbench = await openScenario(page, name, mode);
  await assertAgentLandmarks(page);
  await settleTranscriptAtEnd(page);
  if (name === "agent-activity-family") {
    const cards = page.locator(".acp-activity-card");
    await expect(cards).toHaveCount(8);
    await expect(page.locator('[data-activity-kind="execute"]')).toBeVisible();
    const execution = page.locator(
      '.acp-tool-rollup[data-activity-kind="execute"]',
    );
    await execution.locator("summary").click();
    await expect(execution).toHaveAttribute("open", "");
    await expect(page.locator('[data-activity-kind="search"]')).toHaveAttribute(
      "data-activity-state",
      "inProgress",
    );
    const fetch = page.locator('.acp-tool-rollup[data-activity-kind="fetch"]');
    await fetch.locator("summary").click();
    await expect(fetch).toHaveAttribute("open", "");
    await expect(fetch).toContainText("Review preview");
    await expect(
      fetch.getByRole("group", { name: "Review preview" }),
    ).toBeVisible();
    await expect(page.locator('[data-activity-kind="usage"]')).toHaveCount(0);
    await expect(page.locator('[data-activity-kind="error"]')).toContainText(
      "Retry available",
    );
    await expect(page.locator('[data-activity-kind="thought"]')).toHaveCount(0);
    await settleTranscriptAtEnd(page);
    const command = page.locator(".acp-tool-command--line");
    await expect(command).toBeVisible();
    expect(
      await command.evaluate((node) => {
        const card = node.closest<HTMLElement>(".acp-activity-card");
        const range = document.createRange();
        range.selectNodeContents(node);
        const lines = range.getClientRects().length;
        range.detach();
        return {
          cardIsBounded: card !== null && card.scrollWidth <= card.clientWidth,
          commandStaysInside: node.scrollWidth <= node.clientWidth,
          commandWraps: lines > 1,
        };
      }),
    ).toEqual({
      cardIsBounded: true,
      commandStaysInside: true,
      commandWraps: true,
    });
    await expectNoViewportOverflow(page);
  }
  if (name === "agent-thinking-live") {
    await expectLiveThinkingFixtureLine(page);
  }
  if (name === "agent-image") {
    await expect(page.locator(".agent-chat-pane")).toHaveCount(2);
    await expectAgentImageContained(page);
  }
  if (name === "agent-child-activity") {
    await expandChildActivityHierarchy(page);
    await expectChildActivityContained(page);
    await settleTranscriptAtEnd(page);
  }
  await expectNoViewportOverflow(page);
  await takeCanonicalScreenshot(workbench, name);
  if (name === "agent-activity-family") {
    await page.setViewportSize({ width: 720, height: 560 });
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

    await page.emulateMedia({
      forcedColors: "active",
      reducedMotion: "reduce",
    });
    const activityStyle = await page
      .locator(".acp-activity-card")
      .first()
      .evaluate((node) => {
        const style = getComputedStyle(node);
        return {
          borderLeftStyle: style.borderLeftStyle,
          animationDuration: getComputedStyle(
            node.querySelector(".acp-state") ?? node,
          ).animationDuration,
        };
      });
    expect(activityStyle.borderLeftStyle).toBe("solid");
    expect(activityStyle.animationDuration).toBe("0s");
  }
}
