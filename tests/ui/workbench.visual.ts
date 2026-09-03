import { expect, test, type Page } from "@playwright/test";

import type {
  VisualScenarioMode,
  VisualScenarioName,
} from "./fixtures/visualScenarios";

const FIXED_TIME = "2026-08-31T09:30:00.000Z";

const canonicalVisualScenarios = [
  ["agent-single", "agent"],
  ["agent-dual", "agent"],
  ["agent-long", "agent"],
  ["agent-activity-family", "agent"],
  ["agent-image", "agent"],
  ["agent-child-activity", "agent"],
  ["editor", "editor"],
  ["file", "agent"],
  ["diff", "agent"],
  ["preview", "agent"],
  ["error", "agent"],
  ["waiting", "agent"],
] as const satisfies ReadonlyArray<
  readonly [VisualScenarioName, VisualScenarioMode]
>;

async function openScenario(
  page: Page,
  name: VisualScenarioName,
  mode: VisualScenarioMode,
) {
  await page.clock.install({ time: new Date(FIXED_TIME) });
  await page.addInitScript((windowMode: VisualScenarioMode) => {
    localStorage.setItem(
      "sage.workbench.layout.v1",
      JSON.stringify({
        editorToolRailWidth: 264,
        agentChatSplitPercent: 50,
        windowMode,
      }),
    );
  }, mode);
  await page.goto("/");
  const workbench = page.locator("main.workbench");
  await expect(workbench).toBeVisible();
  await page.evaluate(async (scenarioName) => {
    const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
    await fixtures.installVisualScenario(scenarioName);
  }, name);
  await expect(page.locator("html")).toHaveAttribute(
    "data-visual-scenario",
    name,
  );
  await page.evaluate(() => document.fonts.ready);
  await page.locator(".message-list").evaluateAll((lists) => {
    for (const list of lists) list.scrollTop = list.scrollHeight;
  });
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      ),
  );
  await expect(workbench).toHaveClass(new RegExp(`workbench--${mode}`));
  return workbench;
}

async function expectNoViewportOverflow(page: Page) {
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          document.documentElement.scrollWidth <=
          document.documentElement.clientWidth,
      ),
    )
    .toBe(true);
}

async function expectAgentImageContained(page: Page) {
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

async function expandChildActivityHierarchy(page: Page) {
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

async function expectChildActivityContained(page: Page) {
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

async function expectFindContained(page: Page) {
  expect(
    await page.locator(".editor-find--overlay").evaluate((find) => {
      const stage = find.closest<HTMLElement>(".file-editor-stage");
      if (!stage) return false;
      const findBounds = find.getBoundingClientRect();
      const stageBounds = stage.getBoundingClientRect();
      return (
        findBounds.left >= stageBounds.left &&
        findBounds.right <= stageBounds.right &&
        findBounds.top >= stageBounds.top &&
        findBounds.bottom <= stageBounds.bottom
      );
    }),
  ).toBe(true);
}

async function settleTranscriptAtEnd(page: Page) {
  await page.locator(".message-list").evaluateAll(async (lists) => {
    let priorGeometry = "";
    let stableFrames = 0;
    for (let frame = 0; frame < 30 && stableFrames < 4; frame += 1) {
      for (const list of lists) list.scrollTop = list.scrollHeight;
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => resolve()),
      );
      const geometry = lists
        .map((list) => `${list.scrollHeight}:${list.clientHeight}`)
        .join("|");
      stableFrames = geometry === priorGeometry ? stableFrames + 1 : 0;
      priorGeometry = geometry;
    }
    for (const list of lists) list.scrollTop = list.scrollHeight;
  });
  await expect
    .poll(() =>
      page
        .locator(".message-list")
        .evaluateAll((lists) =>
          lists.every(
            (list) =>
              list.scrollHeight - list.scrollTop - list.clientHeight <= 1,
          ),
        ),
    )
    .toBe(true);
}

test.describe("Wave 2 canonical visual states", () => {
  test.use({ viewport: { width: 1380, height: 880 } });

  for (const [name, mode] of canonicalVisualScenarios) {
    test(`${name} stays visually stable`, async ({ page }) => {
      if (name === "agent-activity-family") {
        await page.setViewportSize({ width: 1380, height: 1200 });
      }
      const workbench = await openScenario(page, name, mode);

      if (mode === "agent") {
        const projectSidebar = page.getByRole("complementary", {
          name: "Projects and chats",
        });
        await expect(
          projectSidebar.getByRole("button", {
            name: "New chat",
            exact: true,
          }),
        ).toBeVisible();
        await expect(
          projectSidebar.getByRole("button", { name: "Search chats" }),
        ).toBeVisible();
      } else {
        await expect(page.getByRole("tabpanel")).toBeVisible();
      }
      if (name === "editor") {
        const tree = page.getByRole("tree", { name: "Project files" });
        const activeFile = tree.getByRole("treeitem", {
          name: "Open file src/workbench/AgentChatPanes.tsx",
        });
        await expect(activeFile).toBeVisible();
        await expect(activeFile).toHaveAttribute("aria-current", "page");
        expect(
          await activeFile.evaluate((node) => {
            const row = node.getBoundingClientRect();
            const viewport = node.closest(".sidebar")?.getBoundingClientRect();
            return Boolean(
              viewport &&
              row.top >= Math.max(viewport.top, 0) &&
              row.bottom <= Math.min(viewport.bottom, window.innerHeight),
            );
          }),
        ).toBe(true);
      }
      if (mode === "agent") await settleTranscriptAtEnd(page);
      if (name === "agent-activity-family") {
        const cards = page.locator(".acp-activity-card");
        await expect(cards).toHaveCount(10);
        await expect(
          page.locator('[data-activity-kind="command"]'),
        ).toBeVisible();
        await expect(
          page.locator('[data-activity-kind="search"]'),
        ).toHaveAttribute("data-activity-state", "inProgress");
        await expect(
          page.locator('[data-activity-kind="usage"]'),
        ).toContainText("1,626 tokens");
        await expect(
          page.locator('[data-activity-kind="error"]'),
        ).toContainText("Retry available");
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
        const command = page.locator(".acp-tool-command");
        await expect(command).toBeVisible();
        expect(
          await command.evaluate((node) => {
            const card = node.closest<HTMLElement>(".acp-activity-card");
            return {
              cardIsBounded:
                card !== null && card.scrollWidth <= card.clientWidth,
              commandOwnsOverflow: node.scrollWidth > node.clientWidth,
            };
          }),
        ).toEqual({ cardIsBounded: true, commandOwnsOverflow: true });
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
      await expect(workbench).toHaveScreenshot(`${name}.png`);
      if (name === "agent-activity-family") {
        await page.setViewportSize({ width: 720, height: 560 });
        await expect
          .poll(() =>
            page
              .locator(".acp-activity-card")
              .evaluateAll((cards) =>
                cards.every(
                  (card) =>
                    card.clientWidth > 0 &&
                    card.scrollWidth <= card.clientWidth,
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
    });
  }
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
  test("keeps Editor Settings visible, resettable, and responsive", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");
    await page.getByRole("button", { name: "Open settings" }).click();

    await expect(page.getByRole("tab", { name: "Settings" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    const settings = page.getByRole("region", { name: "Settings" });
    await expect(settings).toBeVisible();
    const reset = settings.getByRole("button", {
      name: "Reset Editor Appearance",
    });
    await expect(reset).toBeDisabled();
    await settings
      .getByRole("combobox", { name: "Font size" })
      .selectOption("16");
    await expect(reset).toBeEnabled();
    await reset.click();
    await expect(
      settings.getByRole("combobox", { name: "Font size" }),
    ).toHaveValue("13");
    await expect(workbench).toHaveScreenshot("editor-settings.png");

    await page.setViewportSize({ width: 720, height: 560 });
    await page.getByRole("button", { name: "Hide left sidebar" }).click();
    await expect(settings).toBeVisible();
    await expectNoViewportOverflow(page);
    const fontCopy = settings.getByText(
      "Controls the size of text in the code editor.",
    );
    const fontSelect = settings.getByRole("combobox", { name: "Font size" });
    const [copyBounds, selectBounds] = await Promise.all([
      fontCopy.boundingBox(),
      fontSelect.boundingBox(),
    ]);
    expect(copyBounds).not.toBeNull();
    expect(selectBounds).not.toBeNull();
    expect(selectBounds!.y).toBeGreaterThan(copyBounds!.y);
    await page.emulateMedia({ forcedColors: "active" });
    await expect(settings.getByRole("searchbox")).toBeVisible();
    await settings.getByRole("checkbox", { name: "Word wrap" }).check();
    await expect(
      settings.getByRole("checkbox", { name: "Word wrap" }),
    ).toBeChecked();
    await expect(reset).toBeEnabled();
    await expectNoViewportOverflow(page);
  });

  test("scrolls a restored Explorer file into the actual sidebar viewport", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1_000, height: 560 });
    await openScenario(page, "editor", "editor");
    await page.evaluate(async () => {
      const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
      await fixtures.installVisualExplorerScrollFixture();
    });

    const activeFile = page.getByRole("treeitem", {
      name: "Open file src/workbench/AgentChatPanes.tsx",
    });
    await expect(activeFile).toBeVisible();
    const visibility = await activeFile.evaluate((node) => {
      const row = node.getBoundingClientRect();
      const viewport = node.closest(".sidebar")?.getBoundingClientRect();
      const sidebar = node.closest<HTMLElement>(".sidebar");
      return {
        inside:
          viewport !== undefined &&
          row.top >= Math.max(viewport.top, 0) &&
          row.bottom <= Math.min(viewport.bottom, window.innerHeight),
        scrolled: Boolean(sidebar && sidebar.scrollTop > 0),
      };
    });
    expect(visibility).toEqual({ inside: true, scrolled: true });
    await expectNoViewportOverflow(page);
  });

  test("keeps compact Explorer actions usable and contained", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    const workbench = await openScenario(page, "editor", "editor");
    await page.evaluate(async () => {
      const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
      fixtures.installVisualExplorerActionFixture();
    });

    const explorer = page.getByLabel("Explorer project tools", { exact: true });
    const more = explorer.getByRole("button", {
      name: "More Explorer actions",
    });
    await expect(
      explorer.getByRole("button", { name: "New File" }),
    ).toBeVisible();
    await expect(
      explorer.getByRole("button", { name: "New Folder" }),
    ).toBeVisible();
    await expect(
      explorer.getByRole("button", { name: "Add folder root" }),
    ).toHaveCount(0);

    await more.focus();
    await more.press("Enter");
    const menu = explorer.getByRole("menu", { name: "Explorer actions" });
    await expect(menu).toBeVisible();
    await expect(menu.getByRole("menuitem")).toHaveText([
      "Add folder root",
      "Rename File",
      "Move to Recycle Bin",
    ]);
    await expect(
      menu.getByRole("menuitem", { name: "Add folder root" }),
    ).toBeFocused();
    expect(
      await menu
        .getByRole("menuitem")
        .evaluateAll((items) =>
          items.map((item) => (item as HTMLElement).tabIndex),
        ),
    ).toEqual([0, -1, -1]);
    await page.keyboard.press("End");
    await expect(
      menu.getByRole("menuitem", { name: "Move to Recycle Bin" }),
    ).toBeFocused();
    expect(
      await menu
        .getByRole("menuitem")
        .evaluateAll((items) =>
          items.map((item) => (item as HTMLElement).tabIndex),
        ),
    ).toEqual([-1, -1, 0]);
    await page.keyboard.press("Tab");
    await expect(menu).toBeHidden();
    await expect(more).toHaveAttribute("aria-expanded", "false");

    await more.focus();
    await more.press("ArrowUp");
    await expect(menu).toBeVisible();
    await expect(
      menu.getByRole("menuitem", { name: "Move to Recycle Bin" }),
    ).toBeFocused();
    await page.keyboard.press("Shift+Tab");
    await expect(menu).toBeHidden();
    await expect(more).toHaveAttribute("aria-expanded", "false");

    await more.focus();
    await more.press("ArrowDown");
    await expect(
      menu.getByRole("menuitem", { name: "Add folder root" }),
    ).toBeFocused();

    expect(
      await menu.evaluate((node) => {
        const menuBounds = node.getBoundingClientRect();
        const sidebarBounds = node.closest(".sidebar")?.getBoundingClientRect();
        return Boolean(
          sidebarBounds &&
          menuBounds.left >= sidebarBounds.left &&
          menuBounds.right <= sidebarBounds.right &&
          menuBounds.bottom <= Math.min(sidebarBounds.bottom, innerHeight),
        );
      }),
    ).toBe(true);
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-explorer-actions.png");

    await menu.getByRole("menuitem", { name: "Rename File" }).click();
    await expect(menu).toBeHidden();
    const dialog = page.getByRole("dialog", { name: "Rename File" });
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(more).toBeFocused();

    await more.click();
    await page.emulateMedia({ forcedColors: "active" });
    await expect(menu).toBeVisible();
    expect(
      await menu.evaluate((node) => {
        const style = getComputedStyle(node);
        return {
          border: style.borderTopStyle,
          distinct: style.color !== style.backgroundColor,
        };
      }),
    ).toEqual({ border: "solid", distinct: true });
    await explorer.locator(".sidebar__heading > span").click();
    await expect(menu).toBeHidden();
    await expect(more).toBeFocused();

    await page.evaluate(async () => {
      const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
      fixtures.installVisualExplorerUnavailableActionFixture();
    });
    await more.press("ArrowDown");
    const reason = menu.getByRole("menuitem", {
      name: "Select a visible closed file or empty folder.",
    });
    await expect(reason).toHaveAttribute("aria-disabled", "true");
    await expect(reason).toHaveAttribute("tabindex", "-1");
    await page.keyboard.press("ArrowDown");
    await expect(reason).toBeFocused();
    expect(
      await menu
        .getByRole("menuitem")
        .evaluateAll((items) =>
          items.map((item) => (item as HTMLElement).tabIndex),
        ),
    ).toEqual([-1, 0]);
    await page.keyboard.press("ArrowDown");
    await expect(
      menu.getByRole("menuitem", { name: "Add folder root" }),
    ).toBeFocused();
    await page.keyboard.press("ArrowUp");
    await expect(reason).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(menu).toBeHidden();
    await more.press("ArrowUp");
    await expect(reason).toBeFocused();
    await expect(reason).toHaveAttribute("tabindex", "0");
  });

  test("keeps in-file Find compact without moving the document", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");
    const editor = page.getByRole("textbox", {
      name: "Edit src/workbench/AgentChatPanes.tsx",
    });
    const editorPlane = page.locator(".editor-input");
    const before = await editorPlane.boundingBox();

    await editor.press("Control+f");
    const find = page.getByRole("search", { name: "Find in file" });
    await expect(find).toBeVisible();
    await find.getByRole("textbox", { name: "Find text" }).fill("ChatSurface");
    await expect(find.getByRole("status")).toContainText("matches");
    expect(await editorPlane.boundingBox()).toEqual(before);
    await expectFindContained(page);
    await expect(workbench).toHaveScreenshot("editor-find.png");

    await page.setViewportSize({ width: 720, height: 560 });
    const hideSidebar = page.getByRole("button", { name: "Hide left sidebar" });
    if (await hideSidebar.isVisible()) await hideSidebar.click();
    await expect(find).toBeVisible();
    await expectFindContained(page);
    expect(
      await find.evaluate(
        (node) => node.scrollWidth <= node.clientWidth && node.clientWidth > 0,
      ),
    ).toBe(true);
    await page.emulateMedia({ forcedColors: "active" });
    await find.getByRole("button", { name: "Match case" }).click();
    await expect(
      find.getByRole("button", { name: "Match case" }),
    ).toHaveAttribute("aria-pressed", "true");
    await expectNoViewportOverflow(page);
  });

  test("reveals Chat actions in the trailing state cell without moving the title", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "waiting", "agent");

    const chatButton = page
      .getByRole("complementary", { name: "Projects and chats" })
      .getByRole("button", { name: /Open chat .*pinned/ })
      .first();
    const row = chatButton.locator("..");
    const title = chatButton.locator(".project-chat__title-label");
    const state = row.locator(".project-chat__state");
    const actions = row.getByRole("button", { name: /Chat actions for/ });
    const before = await title.evaluate((node) => {
      const bounds = node.getBoundingClientRect();
      return [bounds.x, bounds.y, bounds.width, bounds.height];
    });

    await expect(state).toHaveCSS("opacity", "1");
    await expect(actions).toHaveCSS("opacity", "0");
    await row.hover();
    await expect(state).toHaveCSS("opacity", "0");
    await expect(actions).toHaveCSS("opacity", "1");
    expect(
      await title.evaluate((node) => {
        const bounds = node.getBoundingClientRect();
        return [bounds.x, bounds.y, bounds.width, bounds.height];
      }),
    ).toEqual(before);
    await expectNoViewportOverflow(page);
  });

  test("keeps dual Chat header geometry stable when focus changes", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "agent-dual", "agent");

    const panes = page.locator(".agent-chat-pane");
    const paneHeaders = page.locator(".agent-chat-pane__header");
    const actionRows = page.locator(".agent-chat-pane .surface-header");
    await expect(panes).toHaveCount(2);
    await expect(paneHeaders).toHaveCount(2);
    await expect(actionRows).toHaveCount(2);
    expect(
      await paneHeaders.evaluateAll((headers) =>
        headers.map((header) => header.getBoundingClientRect().height),
      ),
    ).toEqual([32, 32]);
    expect(
      await actionRows.evaluateAll((rows) =>
        rows.every((row) => row.getBoundingClientRect().height <= 36),
      ),
    ).toBe(true);
    const shortcuts = page.locator(
      ".agent-chat-panes--split .composer__shortcut",
    );
    await expect(shortcuts).toHaveCount(2);
    await expect(shortcuts.first()).toBeHidden();
    for (let index = 0; index < 2; index += 1) {
      const pane = panes.nth(index);
      const controls = await Promise.all(
        [
          pane.getByRole("combobox", { name: "Harness" }),
          pane.getByRole("combobox", { name: "Model" }),
          pane.locator(".send-button"),
        ].map((control) =>
          control.evaluate((node) => node.getBoundingClientRect().y),
        ),
      );
      expect(Math.max(...controls) - Math.min(...controls)).toBeLessThan(4);
    }

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

  test("keeps Follow latest and pinned resources scoped to each Chat", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1_380, height: 880 });
    await openScenario(page, "agent-dual", "agent");
    const secondFocus = page
      .locator(".agent-chat-pane__focus")
      .filter({ hasText: "Editor consistency pass" });
    await secondFocus.click();
    await expect(secondFocus).toHaveAttribute("aria-pressed", "true");

    await page.evaluate(async () => {
      const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
      await fixtures.installVisualAgentArtifactFollowFixture();
    });

    const resources = page.getByRole("complementary", {
      name: "Agent artifacts",
    });
    await expect(resources).not.toHaveClass(/agent-artifact-pane--collapsed/);
    const tabs = resources.getByRole("tab");
    await expect(tabs).toHaveCount(2);
    await expect(tabs.nth(0)).toHaveAttribute("aria-selected", "true");
    await expect(tabs.nth(1)).toHaveAttribute("aria-selected", "false");
    await expect(
      page
        .locator(".agent-chat-pane__focus")
        .filter({ hasText: "Editor consistency pass" }),
    ).toHaveAttribute("aria-pressed", "true");

    const follow = resources.getByRole("button", {
      name: "Follow latest resource",
    });
    await follow.click();
    await expect(follow).toHaveAttribute("aria-pressed", "true");
    await expect(tabs.nth(1)).toHaveAttribute("aria-selected", "true");
    await expect(
      page
        .locator(".agent-chat-pane__focus")
        .filter({ hasText: "Editor consistency pass" }),
    ).toHaveAttribute("aria-pressed", "true");

    const rail = page.getByRole("complementary", {
      name: "Projects and chats",
    });
    await rail
      .getByRole("button", { name: /Open chat Agent navigation polish/ })
      .click();
    await expect(
      page
        .locator(".agent-chat-pane__focus")
        .filter({ hasText: "Agent navigation polish" }),
    ).toHaveAttribute("aria-pressed", "true");
    await expect(
      resources.getByRole("tab", { name: /AgentChatPanes\.tsx changes/ }),
    ).toHaveAttribute("aria-selected", "true");

    await rail
      .getByRole("button", { name: /Open chat Editor consistency pass/ })
      .click();
    await expect(
      page
        .locator(".agent-chat-pane__focus")
        .filter({ hasText: "Editor consistency pass" }),
    ).toHaveAttribute("aria-pressed", "true");
    await expect(tabs.nth(1)).toHaveAttribute("aria-selected", "true");
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

  test("keeps the same File and Diff document language in Editor and Agent", async ({
    page,
    context,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "editor", "editor");

    const readFileAppearance = async (target: Page) => {
      await expect(target.locator(".file-surface .cm-gutters")).toBeVisible();
      return target.locator(".file-surface").evaluate((surface) => {
        const header = surface.querySelector<HTMLElement>(".file-header");
        const editor = surface.querySelector<HTMLElement>(".editor-input");
        const content = surface.querySelector<HTMLElement>(".cm-content");
        const gutters = surface.querySelector<HTMLElement>(".cm-gutters");
        if (!header || !editor || !content || !gutters) {
          throw new Error("File appearance probe could not find the document");
        }
        const selectionStyle = getComputedStyle(content, "::selection");
        return {
          canvas: getComputedStyle(surface).backgroundColor,
          text: getComputedStyle(editor).color,
          header: getComputedStyle(header).backgroundColor,
          headerHeight: header.getBoundingClientRect().height,
          gutter: getComputedStyle(gutters).color,
          selection: selectionStyle.backgroundColor,
          selectionText: selectionStyle.color,
        };
      });
    };

    const editorAppearance = await readFileAppearance(page);
    const agentPage = await context.newPage();
    await agentPage.setViewportSize({ width: 1380, height: 880 });
    await openScenario(agentPage, "file", "agent");
    const agentAppearance = await readFileAppearance(agentPage);
    const agentEditor = agentPage.getByRole("textbox", {
      name: "Edit src/workbench/AgentChatPanes.tsx",
    });
    await agentEditor.press("Control+f");
    const agentFind = agentPage.getByRole("search", { name: "Find in file" });
    await agentFind
      .getByRole("textbox", { name: "Find text" })
      .fill("ChatSurface");
    await expectFindContained(agentPage);
    await expect(agentFind.getByRole("status")).toContainText("matches");
    expect(
      await agentFind.evaluate((node) => {
        const query = node.querySelector<HTMLElement>(".editor-find__query");
        const status = node.querySelector<HTMLElement>(".editor-find__status");
        const buttons = Array.from(node.querySelectorAll("button"));
        if (!query || !status) return null;
        const queryBounds = query.getBoundingClientRect();
        const controlBounds = [status, ...buttons].map((control) =>
          control.getBoundingClientRect(),
        );
        return {
          display: getComputedStyle(node).display,
          buttonCount: buttons.length,
          queryAboveControls: controlBounds.every(
            (bounds) => queryBounds.bottom <= bounds.top,
          ),
          controlsVisible: controlBounds.every(
            (bounds) => bounds.width > 0 && bounds.height > 0,
          ),
          noOverflow: node.scrollWidth <= node.clientWidth,
        };
      }),
    ).toEqual({
      display: "grid",
      buttonCount: 4,
      queryAboveControls: true,
      controlsVisible: true,
      noOverflow: true,
    });
    await agentFind.getByRole("button", { name: "Close find" }).click();

    expect(agentAppearance).toEqual(editorAppearance);
    await expect(agentPage.locator(".file-header__metadata")).toBeHidden();
    await expect(
      agentPage.locator(".file-breadcrumb__item").last(),
    ).toBeVisible();
    expect(
      await agentPage
        .locator(".file-header")
        .evaluate((header) => header.scrollWidth <= header.clientWidth),
    ).toBe(true);
    await expectNoViewportOverflow(agentPage);
    await agentPage.close();

    const readDiffAppearance = (target: Page) =>
      target.locator(".diff-surface").evaluate((surface) => {
        const header = surface.querySelector<HTMLElement>(".diff-header");
        const toolbar = surface.querySelector<HTMLElement>(".diff-toolbar");
        const code = surface.querySelector<HTMLElement>(".diff-line code");
        const gutter = surface.querySelector<HTMLElement>(".diff-line > span");
        const added = surface.querySelector<HTMLElement>(".diff-line--added");
        const removed = surface.querySelector<HTMLElement>(
          ".diff-line--removed",
        );
        if (!header || !toolbar || !code || !gutter || !added || !removed) {
          throw new Error("Diff appearance probe could not find the document");
        }
        return {
          canvas: getComputedStyle(surface).backgroundColor,
          header: getComputedStyle(header).backgroundColor,
          headerHeight: header.getBoundingClientRect().height,
          toolbar: getComputedStyle(toolbar).backgroundColor,
          toolbarHeight: toolbar.getBoundingClientRect().height,
          text: getComputedStyle(code).color,
          gutter: getComputedStyle(gutter).color,
          added: getComputedStyle(added).backgroundColor,
          removed: getComputedStyle(removed).backgroundColor,
          selection: getComputedStyle(code, "::selection").backgroundColor,
          selectionText: getComputedStyle(code, "::selection").color,
        };
      });

    const editorDiffPage = await context.newPage();
    await editorDiffPage.setViewportSize({ width: 1380, height: 880 });
    await openScenario(editorDiffPage, "diff", "editor");
    const editorDiffAppearance = await readDiffAppearance(editorDiffPage);

    const agentDiffPage = await context.newPage();
    await agentDiffPage.setViewportSize({ width: 1380, height: 880 });
    await openScenario(agentDiffPage, "diff", "agent");
    const agentDiffAppearance = await readDiffAppearance(agentDiffPage);

    expect(agentDiffAppearance).toEqual(editorDiffAppearance);
    await expectNoViewportOverflow(editorDiffPage);
    await expectNoViewportOverflow(agentDiffPage);
    await editorDiffPage.close();
    await agentDiffPage.close();
  });

  test("keeps Editor Source Control compact and visually stable", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");

    await page
      .getByRole("button", { name: "Source Control", exact: true })
      .click();
    await expect(
      page.getByLabel("Source Control project tools", { exact: true }),
    ).toBeVisible();
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-source-control.png");
  });

  test("keeps the Editor welcome surface useful and bounded", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");
    await page.evaluate(async () => {
      const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
      fixtures.installVisualEmptyEditorCanvas();
    });

    const welcome = page.getByRole("region", { name: "Editor welcome" });
    await expect(welcome).toBeVisible();
    await expect(
      welcome.getByRole("button", { name: "Browse files" }),
    ).toBeVisible();
    await expect(
      welcome.getByRole("button", { name: "Search files" }),
    ).toBeVisible();
    await expect(
      welcome.getByRole("button", { name: "Open Agent" }),
    ).toBeVisible();
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-welcome.png");
  });

  test("shows selected, pressed, disabled, dirty, and confirmation states", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");
    await page.evaluate(async () => {
      const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
      fixtures.installVisualDirtyEditor();
    });

    await expect(
      page.getByRole("status", {
        name: "Unsaved changes. Press Control or Command plus S to save.",
      }),
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "Save" })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Discard" })).toHaveCount(0);

    const wrap = page.getByRole("button", {
      name: "Toggle editor word wrap",
    });
    await wrap.click();
    await expect(wrap).toHaveAttribute("aria-pressed", "true");
    const font = page.getByRole("button", {
      name: "Cycle editor font size",
    });
    const tabs = page.getByRole("button", { name: "Cycle editor tab size" });
    await font.click();
    await tabs.click();
    await expect(font).toHaveText("Font: 14px");
    await expect(tabs).toHaveText("Tabs: 4 spaces");

    const sourceControl = page.getByRole("button", {
      name: "Source Control",
      exact: true,
    });
    await sourceControl.click();
    await expect(sourceControl).toHaveAttribute("aria-expanded", "true");

    const sourceTools = page.getByLabel("Source Control project tools", {
      exact: true,
    });
    await expect(
      sourceTools.getByRole("button", { name: "Publish Branch", exact: true }),
    ).toBeVisible();
    await expect(
      sourceTools.getByRole("form", {
        name: "Commit staged changes in Sage",
      }),
    ).toHaveCount(0);

    const discard = sourceTools.getByRole("button", {
      name: /Discard unstaged changes to src\/workbench\/AgentChatPanes\.tsx/,
    });
    await discard.hover();
    await expect(discard).toHaveCSS("background-color", "rgb(42, 42, 42)");
    await discard.click();
    const confirmation = sourceTools.getByRole("group", {
      name: /Confirm discard of src\/workbench\/AgentChatPanes\.tsx/,
    });
    await expect(confirmation).toBeVisible();
    const cancel = confirmation.getByRole("button", {
      name: /Cancel discard/,
    });
    await expect(cancel).toBeVisible();
    await cancel.focus();
    await expect(cancel).toBeFocused();
    await page.keyboard.press("Shift+Tab");
    const confirmDiscard = confirmation.getByRole("button", {
      name: /Confirm discard/,
    });
    await expect(confirmDiscard).toBeFocused();
    await expect(page.locator(":focus-visible")).toHaveCount(1);

    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-control-states.png");

    for (const state of ["ahead", "behind", "diverged"] as const) {
      await page.evaluate(async (primaryState) => {
        const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
        fixtures.installVisualRepositoryPrimaryState(primaryState);
      }, state);
      const label =
        state === "ahead"
          ? "Push"
          : state === "behind"
            ? "Pull"
            : "Sync Changes";
      const action = sourceTools.getByRole("button", {
        name: label,
        exact: true,
      });
      await expect(action).toBeVisible();
      if (state === "diverged") await expect(action).toBeDisabled();
    }
    await expect(sourceTools).toContainText(
      "Sage cannot yet choose a merge or rebase safely.",
    );
  });

  test("keeps Editor Source Control errors distinct", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");
    await page.evaluate(async () => {
      const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
      fixtures.installVisualRepositoryLoading();
    });

    await page
      .getByRole("button", { name: "Source Control", exact: true })
      .click();
    const sourceTools = page.getByLabel("Source Control project tools", {
      exact: true,
    });
    await expect(sourceTools.getByRole("status")).toContainText(
      "Loading repository status",
    );
    await expect(workbench).toHaveScreenshot(
      "editor-source-control-loading.png",
    );
    await page.evaluate(async () => {
      const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
      fixtures.installVisualRepositoryError();
    });
    await expect(sourceTools.getByRole("alert")).toContainText(
      "Could not refresh repository inventory.",
    );
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-source-control-error.png");
  });

  test("keeps Editor Search compact and visually stable", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");

    await page.getByRole("button", { name: "Search", exact: true }).click();
    const searchTools = page.getByLabel("Search project tools", {
      exact: true,
    });
    await searchTools.getByRole("searchbox").fill("AgentChatPanes");
    await searchTools.getByRole("searchbox").press("Enter");
    await expect(
      searchTools.getByRole("button", {
        name: "Open src/workbench/AgentChatPanes.tsx at line 3, column 17",
      }),
    ).toBeVisible();
    await expect(searchTools).toContainText(
      "export function AgentChatPanes() {",
    );
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-search-populated.png");
  });

  test("keeps Editor Search empty state bounded", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");

    await page.getByRole("button", { name: "Search", exact: true }).click();
    const searchTools = page.getByLabel("Search project tools", {
      exact: true,
    });
    await expect(searchTools).toContainText(
      "Search all roots in the active Project.",
    );
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-search.png");
  });

  test("keeps Editor Problems cards bounded", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");

    await page.getByRole("button", { name: "Problems", exact: true }).click();
    const problemsTools = page.getByLabel("Problems project tools", {
      exact: true,
    });
    await expect(
      problemsTools.getByRole("heading", { name: "Problems" }),
    ).toBeVisible();
    await expect(problemsTools.getByText("3 Problems")).toBeVisible();
    const problem = problemsTools.getByRole("button", {
      name: "Flag: maximum should be named maxVisibleChats. src/workbench/AgentChatPanes.tsx, line 4, column 43",
    });
    await expect(problem).toBeVisible();
    await expect(problem).toHaveAttribute(
      "title",
      "maximum should be named maxVisibleChats — src/workbench/AgentChatPanes.tsx:4:43",
    );
    await expect(problemsTools.locator(".problem-card")).toHaveCount(3);
    await expect(problem).toContainText("AgentChatPanes.tsx:4:43");
    const problemPosition = problem.locator(".problem-card__location-position");
    await expect(problemPosition).toHaveText(":4:43");
    await expect(problemPosition).toBeInViewport();
    await problem.click();
    await expect(page.getByTitle("Caret position")).toHaveText("Ln 4, Col 43");
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-problems-populated.png");
  });

  test("keeps Editor Problems empty state bounded", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");
    await page.evaluate(async () => {
      const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
      fixtures.installVisualEmptyEditorTools();
    });

    await page.getByRole("button", { name: "Problems", exact: true }).click();
    const problemsTools = page.getByLabel("Problems project tools", {
      exact: true,
    });
    await expect(problemsTools).toContainText(
      "No problems for the active File.",
    );
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-problems.png");
  });

  test("keeps Editor Outline cards bounded", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");

    await page.getByRole("button", { name: "Outline", exact: true }).click();
    const outlineTools = page.getByLabel("Outline project tools", {
      exact: true,
    });
    await expect(
      outlineTools.getByRole("heading", { name: "Outline" }),
    ).toBeVisible();
    await expect(outlineTools.getByText("4 Symbols")).toBeVisible();
    const symbol = outlineTools.getByRole("button", {
      name: "Function: AgentChatPanes. src/workbench/AgentChatPanes.tsx, line 3, column 1",
    });
    await expect(symbol).toBeVisible();
    await expect(symbol).toHaveAttribute(
      "title",
      "AgentChatPanes — src/workbench/AgentChatPanes.tsx:3:1",
    );
    await expect(outlineTools.locator(".outline-card")).toHaveCount(4);
    await expect(
      outlineTools.getByRole("button", {
        name: "Type: AgentChatPanesWithProjectScopedResourceFollowing. src/workbench/AgentChatPanes.tsx, line 3, column 1",
      }),
    ).toHaveAttribute(
      "title",
      "AgentChatPanesWithProjectScopedResourceFollowing — src/workbench/AgentChatPanes.tsx:3:1",
    );
    const symbolPosition = symbol.locator(".outline-card__location-position");
    await expect(symbolPosition).toHaveText(":3:1");
    await expect(symbolPosition).toBeInViewport();
    await symbol.focus();
    await page.keyboard.press("Enter");
    await expect(page.getByTitle("Caret position")).toHaveText("Ln 3, Col 1");
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-outline-populated.png");
  });

  test("keeps Editor Outline empty state bounded", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");
    await page.evaluate(async () => {
      const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
      fixtures.installVisualEmptyEditorTools();
    });

    await page.getByRole("button", { name: "Outline", exact: true }).click();
    const outlineTools = page.getByLabel("Outline project tools", {
      exact: true,
    });
    await expect(outlineTools).toContainText("No symbols for the active File.");
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-outline.png");
  });

  test("keeps Editor focus, selection, and resize highlights subtle", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "editor", "editor");

    const selectedFile = page
      .getByRole("tree", { name: "Project files" })
      .getByRole("treeitem", {
        name: "Open file src/workbench/AgentChatPanes.tsx",
      });
    const activeTab = page.getByRole("tab", {
      name: /AgentChatPanes\.tsx/,
    });
    const explorerActivity = page.getByRole("button", {
      name: "Explorer",
      exact: true,
    });
    const persistentStates = await page.evaluate(() => {
      const workbenchNode =
        document.querySelector<HTMLElement>(".workbench--editor");
      const selected = document.querySelector<HTMLElement>(
        ".workbench--editor .tree__row--selected",
      );
      const tab = document.querySelector<HTMLElement>(
        ".workbench--editor .tab--active",
      );
      const activity = document.querySelector<HTMLElement>(
        ".workbench--editor .activity-rail__button--active",
      );
      if (!workbenchNode || !selected || !tab || !activity) return null;
      return {
        focusWidth: getComputedStyle(workbenchNode)
          .getPropertyValue("--focus-ring-width")
          .trim(),
        selectedShadow: getComputedStyle(selected).boxShadow,
        tabShadow: getComputedStyle(tab).boxShadow,
        activityIndicatorWidth: getComputedStyle(activity, "::after").width,
      };
    });
    expect(persistentStates).not.toBeNull();
    expect(persistentStates!.focusWidth).toBe("1px");
    expect(persistentStates!.selectedShadow).toContain("1px");
    expect(persistentStates!.selectedShadow).not.toContain("2px");
    expect(persistentStates!.tabShadow).toContain("1px");
    expect(persistentStates!.activityIndicatorWidth).toBe("1px");
    await expect(selectedFile).toBeVisible();
    await expect(activeTab).toHaveAttribute("aria-selected", "true");
    await expect(explorerActivity).toHaveAttribute("aria-expanded", "true");

    const editorContent = page.getByRole("textbox", {
      name: "Edit src/workbench/AgentChatPanes.tsx",
    });
    await editorContent.focus();
    await expect(page.locator(".file-surface .cm-editor")).toHaveClass(
      /cm-focused/,
    );
    const editorFocus = await page.locator(".file-surface").evaluate((file) => {
      const frame = file.querySelector<HTMLElement>(".cm-editor");
      const content = file.querySelector<HTMLElement>(".cm-content");
      if (!frame || !content) return null;
      return {
        frameOutline: getComputedStyle(frame).outlineWidth,
        frameShadow: getComputedStyle(frame).boxShadow,
        contentOutline: getComputedStyle(content).outlineWidth,
      };
    });
    expect(editorFocus).not.toBeNull();
    expect(editorFocus!.frameOutline).toBe("0px");
    expect(editorFocus!.frameShadow).not.toBe("none");
    expect(editorFocus!.contentOutline).toBe("0px");

    await page.getByRole("button", { name: "Search", exact: true }).click();
    const searchTools = page.getByLabel("Search project tools", {
      exact: true,
    });
    const searchbox = searchTools.getByRole("searchbox");
    await searchbox.focus();
    const queryFocus = await searchTools
      .locator(".workspace-search__input")
      .evaluate((input) => {
        const style = getComputedStyle(input);
        return {
          borderWidth: style.borderTopWidth,
          borderColor: style.borderTopColor,
          shadow: style.boxShadow,
        };
      });
    expect(queryFocus.borderWidth).toBe("1px");
    expect(queryFocus.shadow).not.toBe("none");
    expect(queryFocus.borderColor).not.toContain("255, 255, 255");

    await searchTools
      .getByRole("button", { name: "Show Search filters" })
      .click();
    const include = searchTools.getByRole("textbox", {
      name: "Files to include",
    });
    await include.focus();
    const filterFocus = await include.evaluate((input) => {
      const style = getComputedStyle(input);
      return {
        borderWidth: style.borderTopWidth,
        borderColor: style.borderTopColor,
        outlineWidth: style.outlineWidth,
        shadow: style.boxShadow,
      };
    });
    expect(filterFocus).toMatchObject({
      borderWidth: "1px",
      outlineWidth: "0px",
    });
    expect(filterFocus.shadow).not.toBe("none");
    expect(filterFocus.borderColor).not.toContain("255, 255, 255");

    const matchCase = searchTools.getByRole("button", { name: "Match case" });
    await matchCase.click();
    await expect(matchCase).toHaveAttribute("aria-pressed", "true");
    const pressed = await matchCase.evaluate((button) => {
      const style = getComputedStyle(button);
      return { background: style.backgroundColor, shadow: style.boxShadow };
    });
    expect(pressed.background).not.toBe("rgb(42, 75, 109)");
    await expect
      .poll(() =>
        matchCase.evaluate((button) => getComputedStyle(button).boxShadow),
      )
      .toContain("1px");

    const splitter = page.getByRole("separator", {
      name: "Resize Editor tool rail",
    });
    const bounds = await splitter.boundingBox();
    expect(bounds).not.toBeNull();
    await page.mouse.move(bounds!.x + bounds!.width / 2, bounds!.y + 80);
    await searchTools
      .getByRole("textbox", { name: "Files to exclude" })
      .focus();
    await page.keyboard.press("Tab");
    await expect(splitter).toBeFocused();
    const focusedSplitter = await splitter.evaluate((node) => {
      const host = getComputedStyle(node);
      const line = getComputedStyle(node, "::after");
      return {
        focusVisible: node.matches(":focus-visible"),
        outlineStyle: host.outlineStyle,
        lineWidth: line.width,
        lineColor: line.backgroundColor,
        lineShadow: line.boxShadow,
      };
    });
    expect(focusedSplitter).toMatchObject({
      focusVisible: true,
      outlineStyle: "none",
      lineWidth: "1px",
    });
    expect(focusedSplitter.lineColor).not.toBe("rgba(0, 0, 0, 0)");
    expect(focusedSplitter.lineColor).not.toContain("255, 255, 255");
    expect(focusedSplitter.lineShadow).not.toBe("none");

    await page.mouse.down();
    await expect(splitter).toHaveClass(/workbench-sidebar-splitter--active/);
    await expect(workbench).toHaveScreenshot("editor-resizer-states.png");
    await page.mouse.up();

    await page.emulateMedia({ reducedMotion: "reduce" });
    await expect
      .poll(() =>
        matchCase.evaluate(
          (button) => getComputedStyle(button).transitionDuration,
        ),
      )
      .toBe("0s");
  });

  test("keeps Editor planes and resize boundaries intentional", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "editor", "editor");

    await expect
      .poll(() =>
        page.evaluate(() => {
          const activity = document.querySelector<HTMLElement>(
            ".workbench--editor .activity-rail",
          );
          const sidebar = document.querySelector<HTMLElement>(
            '.workbench--editor .sidebar[aria-label="Explorer project tools"]',
          );
          const splitter = document.querySelector<HTMLElement>(
            ".workbench--editor .workbench-sidebar-splitter",
          );
          const center = document.querySelector<HTMLElement>(
            ".workbench--editor .center-region",
          );
          if (!activity || !sidebar || !splitter || !center) return null;
          const activityStyle = getComputedStyle(activity);
          const sidebarStyle = getComputedStyle(sidebar);
          const splitterStyle = getComputedStyle(splitter);
          const splitterLine = getComputedStyle(splitter, "::after");
          const centerStyle = getComputedStyle(center);
          return {
            activityBackground: activityStyle.backgroundColor,
            activityWidth: activity.getBoundingClientRect().width,
            centerBackground: centerStyle.backgroundColor,
            centerBorder: centerStyle.borderColor,
            centerRadius: centerStyle.borderRadius,
            sidebarBackground: sidebarStyle.backgroundColor,
            sidebarBorder: sidebarStyle.borderColor,
            sidebarRadius: sidebarStyle.borderRadius,
            splitterLine: splitterLine.backgroundColor,
            splitterWidth: splitterStyle.width,
          };
        }),
      )
      .toEqual({
        activityBackground: "rgb(20, 20, 20)",
        activityWidth: 48,
        centerBackground: "rgb(20, 20, 20)",
        centerBorder: "rgb(36, 36, 36)",
        centerRadius: "6px",
        sidebarBackground: "rgb(20, 20, 20)",
        sidebarBorder: "rgb(36, 36, 36)",
        sidebarRadius: "6px",
        splitterLine: "rgba(0, 0, 0, 0)",
        splitterWidth: "8px",
      });
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

  test("supports a 720px narrow Editor window", async ({ page }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    const workbench = await openScenario(page, "editor", "editor");

    await expect(workbench).toHaveClass(/workbench--compact/);
    await expect(page.locator(".center-region")).toBeVisible();
    await expect
      .poll(() =>
        page.evaluate(() => {
          const drawer = document.querySelector<HTMLElement>(
            '.workbench--editor .sidebar[role="dialog"]',
          );
          const center = document.querySelector<HTMLElement>(
            ".workbench--editor .center-region",
          );
          if (!drawer || !center) return null;
          const drawerBounds = drawer.getBoundingClientRect();
          const centerBounds = center.getBoundingClientRect();
          return centerBounds.left >= drawerBounds.right;
        }),
      )
      .toBe(true);
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-narrow-720x560.png");
  });

  test("exposes visible keyboard focus", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    const workbench = await openScenario(page, "agent-single", "agent");

    await page.keyboard.press("Tab");
    const focusVisible = page.locator(":focus-visible");
    await expect(focusVisible).toHaveCount(1);
    await expect(focusVisible).toBeVisible();
    await settleTranscriptAtEnd(page);
    await expect(workbench).toHaveScreenshot("agent-keyboard-focus.png");
  });

  test("honors forced colors", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await page.emulateMedia({ forcedColors: "active" });
    const workbench = await openScenario(page, "agent-single", "agent");

    await settleTranscriptAtEnd(page);
    await expect(workbench).toHaveScreenshot("agent-forced-colors.png");
  });

  test("honors forced colors in Editor mode", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await page.emulateMedia({ forcedColors: "active" });
    const workbench = await openScenario(page, "editor", "editor");

    await expect(page.getByRole("tabpanel")).toBeVisible();
    await expect(workbench).toHaveScreenshot("editor-forced-colors.png");
  });

  test("honors reduced motion", async ({ page }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    const workbench = await openScenario(page, "waiting", "agent");

    await expect(
      page.getByRole("status", { name: "1 working turn" }),
    ).toBeVisible();
    await expect(workbench).toHaveScreenshot("agent-reduced-motion.png");
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

  test("keeps Editor Settings usable at 200% zoom", async ({ page }) => {
    await openScenario(page, "editor", "editor");
    await page.getByRole("button", { name: "Open settings" }).click();
    await page.getByRole("button", { name: "Hide left sidebar" }).click();

    const settings = page.getByRole("region", { name: "Settings" });
    await expect(settings).toBeVisible();
    await expect(settings.getByRole("searchbox")).toBeVisible();
    await expect(
      settings.getByRole("combobox", { name: "Font size" }),
    ).toBeVisible();
    await expectNoViewportOverflow(page);
  });

  test("keeps Problems and Outline rows contained at 200% zoom", async ({
    page,
  }) => {
    await openScenario(page, "editor", "editor");

    await page.getByRole("button", { name: "Problems", exact: true }).click();
    const problems = page
      .getByLabel("Problems project tools", { exact: true })
      .locator(".problem-card");
    await expect(problems).toHaveCount(3);
    expect(
      await problems.evaluateAll((rows) =>
        rows.every(
          (row) => row.clientWidth > 0 && row.scrollWidth <= row.clientWidth,
        ),
      ),
    ).toBe(true);

    await page.getByRole("button", { name: "Outline", exact: true }).click();
    const symbols = page
      .getByLabel("Outline project tools", { exact: true })
      .locator(".outline-card");
    await expect(symbols).toHaveCount(4);
    expect(
      await symbols.evaluateAll((rows) =>
        rows.every(
          (row) => row.clientWidth > 0 && row.scrollWidth <= row.clientWidth,
        ),
      ),
    ).toBe(true);
    await expectNoViewportOverflow(page);
  });

  test("supports the Editor workbench at 200% zoom", async ({ page }) => {
    const workbench = await openScenario(page, "editor", "editor");

    await expect(workbench).toHaveClass(/workbench--compact/);
    await expect
      .poll(() =>
        page.evaluate(() => ({
          devicePixelRatio: window.devicePixelRatio,
          innerWidth: window.innerWidth,
        })),
      )
      .toEqual({ devicePixelRatio: 2, innerWidth: 690 });
    await page.evaluate(async () => {
      const fixtures = await import("/tests/ui/fixtures/visualScenarios.ts");
      fixtures.installVisualExplorerActionFixture();
    });
    await page.getByRole("button", { name: "More Explorer actions" }).click();
    const explorerMenu = page.getByRole("menu", { name: "Explorer actions" });
    await expect(explorerMenu).toBeVisible();
    expect(
      await explorerMenu.evaluate((node) => {
        const menuBounds = node.getBoundingClientRect();
        const sidebarBounds = node.closest(".sidebar")?.getBoundingClientRect();
        return Boolean(
          sidebarBounds &&
          menuBounds.left >= sidebarBounds.left &&
          menuBounds.right <= sidebarBounds.right &&
          node.scrollWidth <= node.clientWidth,
        );
      }),
    ).toBe(true);
    await page.keyboard.press("Escape");
    await expect(page.locator(".center-region")).toBeVisible();
    await page.getByRole("button", { name: "Hide left sidebar" }).click();
    const editorContent = page.locator(".file-surface .cm-content");
    await editorContent.click();
    await editorContent.press("Control+f");
    const find = page.getByRole("search", { name: "Find in file" });
    await expect(find).toBeVisible();
    await expectFindContained(page);
    expect(
      await find.evaluate(
        (node) => node.scrollWidth <= node.clientWidth && node.clientWidth > 0,
      ),
    ).toBe(true);
    await find.getByRole("button", { name: "Close find" }).click();
    await page.getByRole("button", { name: "Show left sidebar" }).click();
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("editor-zoom-200.png");
  });
});
