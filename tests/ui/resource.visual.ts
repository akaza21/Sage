import { expect, test, type Page } from "@playwright/test";

import { RESOURCE_VISUAL_SCENARIOS } from "./fixtures/visualScenarioTypes";
import { VISUAL_SCENARIOS } from "./fixtures/visualScenarioCatalog";
import {
  expectNoViewportOverflow,
  openScenario,
  settleTranscriptAtEnd,
  takeCanonicalScreenshot,
} from "./shared/visualBrowserPrimitives";
import {
  exerciseResourceStates,
  openAgentScenario,
  setResourceState,
} from "./resourceVisualHelpers";
import { expectFindContained } from "./editorVisualHelpers";
import { assertAgentLandmarks } from "./agentVisualHelpers";

test.describe("Wave 2 canonical visual states", () => {
  test.use({ viewport: { width: 1380, height: 880 } });

  for (const { name, mode } of VISUAL_SCENARIOS.filter((scenario) =>
    RESOURCE_VISUAL_SCENARIOS.includes(scenario.name),
  )) {
    test(`${name} stays visually stable`, async ({ page }) => {
      const workbench = await openScenario(page, name, mode);
      await assertAgentLandmarks(page);
      await settleTranscriptAtEnd(page);
      await expectNoViewportOverflow(page);
      await takeCanonicalScreenshot(workbench, name);
    });
  }
});

test.describe("Wave 2 accessibility and responsive states", () => {
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
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/resourceVisualFixtures.ts");
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
    const applyCustomAppearance = async (target: Page) => {
      await expect
        .poll(() =>
          target.evaluate(async () => {
            const { useEditorAppearanceStore } =
              await import("/src/app/editorAppearanceStore.ts");
            return useEditorAppearanceStore.getState().persistenceReady;
          }),
        )
        .toBe(true);
      await target.evaluate(async () => {
        const { useEditorAppearanceStore } =
          await import("/src/app/editorAppearanceStore.ts");
        useEditorAppearanceStore.getState().setAppearance({
          fontSize: 16,
          tabSize: 4,
          wordWrap: true,
          lineNumbers: false,
          showWhitespace: true,
        });
      });
      await expect(target.locator(".file-surface .cm-lineNumbers")).toHaveCount(
        0,
      );
      await expect(
        target.locator(".file-surface .cm-highlightSpace").first(),
      ).toBeVisible();
      return target.locator(".file-surface").evaluate((surface) => {
        const content = surface.querySelector<HTMLElement>(".cm-content");
        const scroller = surface.querySelector<HTMLElement>(".cm-scroller");
        if (!content || !scroller) {
          throw new Error("Custom appearance probe could not find the editor");
        }
        return {
          fontSize: getComputedStyle(scroller).fontSize,
          lineWrapping: content.classList.contains("cm-lineWrapping"),
          lineNumbers: surface.querySelectorAll(".cm-lineNumbers").length,
          tabSize: getComputedStyle(content).tabSize,
          whitespace: surface.querySelectorAll(".cm-highlightSpace").length,
        };
      });
    };
    const [customEditorAppearance, customAgentAppearance] = await Promise.all([
      applyCustomAppearance(page),
      applyCustomAppearance(agentPage),
    ]);
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
    expect(customAgentAppearance).toEqual(customEditorAppearance);
    expect(customEditorAppearance).toMatchObject({
      fontSize: "16px",
      lineWrapping: true,
      lineNumbers: 0,
      tabSize: "4",
    });
    expect(customEditorAppearance.whitespace).toBeGreaterThan(0);
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
});

test.describe("V6a Agent right-resource state shell", () => {
  test.use({ viewport: { width: 1_380, height: 880 } });

  test("uses one honest collapsed toggle and returns focus after closing", async ({
    page,
  }) => {
    const workbench = await openAgentScenario(page, "agent-single");
    const toggle = page.getByRole("button", {
      name: "Open files, changes, and preview",
    });
    await expect(toggle).toBeVisible();
    await expect(toggle.locator("svg")).toHaveCount(1);
    await expect(toggle.locator("small")).toHaveCount(0);

    for (let step = 0; step < 24; step += 1) {
      if (await toggle.evaluate((node) => node === document.activeElement)) {
        break;
      }
      await page.keyboard.press("Tab");
    }
    await expect(toggle).toBeFocused();
    const focusRing = await toggle.evaluate((node) => {
      const style = getComputedStyle(node);
      return {
        style: style.outlineStyle,
        width: Number.parseFloat(style.outlineWidth),
      };
    });
    expect(focusRing.style).toBe("solid");
    expect(focusRing.width).toBeGreaterThanOrEqual(1);
    await page.keyboard.press("Enter");
    await expect(
      page.getByRole("heading", { name: "No resources yet" }),
    ).toBeVisible();
    await expect(
      page.getByText(
        "Files, changes, and previews from this Chat will appear here.",
      ),
    ).toBeVisible();
    await expect(workbench).toHaveScreenshot("agent-resource-empty.png");
    const collapse = page.getByRole("button", {
      name: "Collapse files, changes, and preview",
    });
    await collapse.focus();
    await page.keyboard.press("Space");
    await expect(
      page.getByRole("button", {
        name: "Open files, changes, and preview",
      }),
    ).toBeFocused();
    await expectNoViewportOverflow(page);
  });

  test("announces File loading and failure without changing its surface", async ({
    page,
  }) => {
    const workbench = await openAgentScenario(page, "file");
    await expect(
      page.getByRole("textbox", {
        name: "Edit src/workbench/AgentChatPanes.tsx",
      }),
    ).toBeVisible();
    await setResourceState(page, "file-loading");
    const loading = page
      .getByRole("status")
      .filter({ hasText: "Loading src/workbench/AgentChatPanes.tsx" });
    await expect(loading).toContainText(
      "Loading src/workbench/AgentChatPanes.tsx",
    );
    await expect(loading.locator("svg")).toHaveCount(1);

    await page.emulateMedia({
      forcedColors: "active",
      reducedMotion: "reduce",
    });
    await expect
      .poll(() =>
        loading
          .locator("svg")
          .evaluate((icon) => getComputedStyle(icon).animationDuration),
      )
      .toBe("0s");

    await page.emulateMedia({
      forcedColors: "none",
      reducedMotion: "no-preference",
    });
    await setResourceState(page, "file-error");
    await expect(loading).toHaveCount(0);
    await expect(
      page.getByRole("alert").filter({ hasText: "Could not open" }),
    ).toContainText("The file is temporarily unavailable.");
    await expect(workbench).toHaveScreenshot("agent-file-error.png");
    await expectNoViewportOverflow(page);
  });

  test("announces Diff loading and failure with mode-neutral wording", async ({
    page,
  }) => {
    const workbench = await openAgentScenario(page, "diff");
    await expect(
      page.getByRole("table", { name: "Unified file diff" }),
    ).toBeVisible();
    await expect(page.getByText("Read-only", { exact: true })).toBeVisible();
    await expect(page.getByText(/Explorer focused/i)).toHaveCount(0);

    await setResourceState(page, "diff-loading");
    await expect(
      page.getByRole("status").filter({ hasText: "Loading diff…" }),
    ).toBeVisible();
    await expect(workbench).toHaveScreenshot("agent-diff-loading.png");
    await setResourceState(page, "diff-error");
    await expect(
      page.getByRole("alert").filter({ hasText: "Could not load this diff" }),
    ).toContainText("The comparison is temporarily unavailable.");
    await expectNoViewportOverflow(page);
  });

  for (const resource of ["file", "diff"] as const) {
    test(`keeps ${resource} states contained at 720px`, async ({ page }) => {
      await page.setViewportSize({ width: 720, height: 560 });
      const workbench = await openAgentScenario(page, resource);
      await expect(workbench).toHaveClass(/workbench--compact/);
      await exerciseResourceStates(page, resource);
    });
  }

  test("keeps the collapsed and expanded shell contained at 720px", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    const workbench = await openAgentScenario(page, "agent-single");
    await expect(workbench).toHaveClass(/workbench--compact/);
    const toggle = page.getByRole("button", {
      name: "Open files, changes, and preview",
    });
    expect(
      await toggle.evaluate((node) => {
        const bounds = node.getBoundingClientRect();
        return { width: bounds.width, height: bounds.height };
      }),
    ).toEqual({ width: 44, height: 44 });

    await toggle.click();
    await expect(
      page.getByRole("heading", { name: "No resources yet" }),
    ).toBeVisible();
    await expect(page.locator(".center-region")).toHaveAttribute("inert");
    await expectNoViewportOverflow(page);
  });
});

test.describe("V6a Agent resource shell at 200%", () => {
  test.use({
    viewport: { width: 690, height: 440 },
    deviceScaleFactor: 2,
  });

  test("keeps the single edge toggle and empty state usable", async ({
    page,
  }) => {
    const workbench = await openAgentScenario(page, "agent-single");
    await expect(workbench).toHaveClass(/workbench--compact/);
    const toggle = page.getByRole("button", {
      name: "Open files, changes, and preview",
    });
    await expect(toggle.locator("small")).toHaveCount(0);
    await toggle.click();
    await expect(
      page.getByRole("heading", { name: "No resources yet" }),
    ).toBeVisible();
    await expectNoViewportOverflow(page);
  });

  for (const resource of ["file", "diff"] as const) {
    test(`keeps ${resource} states contained`, async ({ page }) => {
      const workbench = await openAgentScenario(page, resource);
      await expect(workbench).toHaveClass(/workbench--compact/);
      await exerciseResourceStates(page, resource);
    });
  }
});
