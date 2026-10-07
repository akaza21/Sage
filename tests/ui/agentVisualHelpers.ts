import { expect, type Page } from "@playwright/test";

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
    projectSidebar.getByRole("button", { name: "Search all chats" }),
  ).toBeVisible();
}

export async function expectAgentImageContained(page: Page) {
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
    await expect(cards).toHaveCount(9);
    await expect(page.locator('[data-activity-kind="execute"]')).toBeVisible();
    await expect(page.locator('[data-activity-kind="search"]')).toHaveAttribute(
      "data-activity-state",
      "inProgress",
    );
    await expect(page.locator('[data-activity-kind="fetch"]')).toContainText(
      "Review preview",
    );
    await expect(page.locator('[data-activity-kind="usage"]')).toHaveCount(0);
    await expect(page.locator('[data-activity-kind="error"]')).toContainText(
      "Retry available",
    );
    const thought = page.locator('[data-activity-kind="thought"]');
    await thought.locator("summary").focus();
    await page.keyboard.press("Enter");
    await expect(thought).toHaveAttribute("open", "");
    await page.keyboard.press("Enter");
    await expect(thought).not.toHaveAttribute("open", "");
    await thought
      .locator("summary")
      .evaluate((node) => (node as HTMLElement).blur());
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
