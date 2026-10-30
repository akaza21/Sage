import { expect, test, type Page } from "@playwright/test";

import {
  fileResource,
  visualSnapshotNewText,
  visualSnapshotOldText,
} from "./fixtures/visualDocumentFixtures";
import { snapshotFileDiff } from "../../src/features/diff/snapshotDiff";
import { RESOURCE_VISUAL_SCENARIOS } from "./fixtures/visualScenarioTypes";
import { PREVIEW_PROJECTION_FIXTURE_SCENARIOS } from "./fixtures/visualScenarioTypes";
import { VISUAL_SCENARIOS } from "./fixtures/visualScenarioCatalog";
import {
  expectNoViewportOverflow,
  installFixedClock,
  installLayoutMode,
  installScenario,
  openScenario,
  settleTranscriptAtEnd,
  settleTranscriptAtStart,
  takeCanonicalScreenshot,
} from "./shared/visualBrowserPrimitives";
import {
  expandAgentSnapshotEdits,
  exerciseResourceStates,
  openAgentScenario,
  openAgentSnapshotDiff,
  setResourceState,
} from "./resourceVisualHelpers";
import { expectFindContained } from "./editorVisualHelpers";
import { assertAgentLandmarks } from "./agentVisualHelpers";
import {
  assertPreviewProjectionTransportBounds,
  emitPreviewProjection,
  installPreviewProjectionTransport,
} from "./previewProjectionFixture";

test.describe("Wave 2 canonical visual states", () => {
  test.use({ viewport: { width: 1380, height: 880 } });

  for (const { name, mode } of VISUAL_SCENARIOS.filter((scenario) =>
    RESOURCE_VISUAL_SCENARIOS.includes(scenario.name),
  )) {
    test(`${name} stays visually stable`, async ({ page }) => {
      const workbench = await openScenario(page, name, mode);
      if (name === "diff") await openAgentSnapshotDiff(page);
      await assertAgentLandmarks(page);
      await settleTranscriptAtEnd(page);
      await expectNoViewportOverflow(page);
      await takeCanonicalScreenshot(workbench, name);
    });
  }
});

test.describe("AV-01 projection-fixture browser routes", () => {
  test.use({ viewport: { width: 1_380, height: 880 } });

  for (const name of PREVIEW_PROJECTION_FIXTURE_SCENARIOS) {
    test(`${name} is projection-fixture proof only`, async ({ page }) => {
      await installFixedClock(page);
      await installLayoutMode(page, "agent");
      await page.goto("/");
      await installPreviewProjectionTransport(page);
      await assertPreviewProjectionTransportBounds(page);
      await installScenario(page, name);
      await page.evaluate(() => document.fonts.ready);
      const workbench = page.locator("main.workbench");
      const resources = page.getByLabel("Agent artifacts", { exact: true });
      await expect(
        resources.getByRole("tab", { name: "Preview" }),
      ).toHaveAttribute("aria-selected", "true");
      const start = resources.getByRole("button", { name: "Start" });
      await expect(start).toBeEnabled();
      await expect(start).toHaveCSS("height", "28px");
      const phase = name.replace("agent-preview-", "");
      await emitPreviewProjection(
        page,
        phase === "health-failed" ? "healthFailed" : phase,
      );
      await expect(
        resources.getByText(
          phase === "starting"
            ? "Starting process"
            : phase === "health-failed"
              ? "Preview failed"
              : "Preview exited",
          { exact: true },
        ),
      ).toBeVisible();
      if (phase !== "starting") {
        const detail =
          phase === "health-failed"
            ? "Preview did not become healthy before its deadline."
            : "Preview exited before it became ready.";
        const reason =
          phase === "health-failed" ? "Health timeout" : "Early exit";
        await expect(
          resources.getByText(reason, { exact: true }),
        ).toBeVisible();
        await expect(resources.getByText(detail, { exact: true })).toHaveCount(
          1,
        );
      }
      await expect(resources.getByTitle("Sage browser")).toHaveCount(0);
      // Projection-fixture proof only; native lifecycle proof is separate.
      await expectNoViewportOverflow(page);
      await settleTranscriptAtEnd(page);
      await takeCanonicalScreenshot(workbench, name);
    });
  }
});

test.describe("Wave 2 accessibility and responsive states", () => {
  test("shows a File or Diff opened through production as new while pinned", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1_380, height: 880 });
    await openAgentScenario(page, "diff");
    await page.evaluate(async () => {
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/resourceVisualFixtures.ts");
      await fixtures.installVisualAgentArtifactPinnedFixture();
    });
    const resources = page.getByLabel("Agent artifacts", { exact: true });
    const file = resources.getByRole("tab", {
      name: "File src/workbench/AgentChatPanes.tsx",
    });
    const diff = resources.getByRole("tab", {
      name: "Diff src/workbench/AgentChatPanes.tsx",
    });
    await expect(file).toHaveAttribute("aria-selected", "true");
    await expect(file.locator(".tab__basename")).toHaveText(
      "AgentChatPanes.tsx",
    );
    await expect(file.locator("svg")).toHaveCount(1);
    await expect(file.getByTitle("Unsaved changes")).toBeVisible();
    await expect(diff).toHaveCount(0);
    await page.evaluate(async () => {
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/resourceVisualFixtures.ts");
      await fixtures.openVisualPinnedDiffThroughProduction();
    });
    await expect(diff).toHaveAttribute("aria-selected", "false");
    await expect(diff.locator(".tab__basename")).toHaveText(
      "AgentChatPanes.tsx",
    );
    await expect(diff.locator("svg")).toHaveCount(1);
    await expect(diff).toHaveAccessibleDescription("New resource");
    await expect(
      resources.getByRole("button", {
        name: "Pinned resource, new File or Diff available",
      }),
    ).toContainText("New");
  });

  test("keeps Follow latest and pinned resources scoped to each Chat", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1_888, height: 880 });
    await openScenario(page, "agent-dual", "agent");
    const secondPane = page.getByRole("region", {
      name: "Chat pane Editor consistency pass",
      exact: true,
    });
    await expect(
      secondPane.getByRole("textbox", { name: "Message agent" }),
    ).toBeDisabled();
    await page
      .getByRole("button", { name: /^Open chat Editor consistency pass$/ })
      .click();
    await expect(secondPane).toHaveClass(/agent-chat-pane--focused/);

    await page.evaluate(async () => {
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/resourceVisualFixtures.ts");
      await fixtures.installVisualAgentArtifactFollowFixture();
    });

    const resources = page.getByLabel("Agent artifacts", { exact: true });
    const resourceSplitter = page.getByRole("separator", {
      name: "Resize files and preview",
    });
    await resourceSplitter.focus();
    await page.keyboard.press("Home");
    await expect(resourceSplitter).toHaveAttribute("aria-valuenow", "280");
    await expect(page.locator(".agent-chat-pane")).toHaveCount(2);
    const tabs = resources.getByRole("tab");
    const previewTab = resources.getByRole("tab", {
      name: "Preview",
      exact: true,
    });
    await expect(tabs).toHaveCount(2);
    await expect(tabs.nth(0)).toHaveAttribute("aria-selected", "true");
    await expect(previewTab).toHaveAttribute("aria-selected", "false");
    await expect(
      page.getByRole("region", {
        name: "Chat pane Editor consistency pass",
        exact: true,
      }),
    ).toHaveClass(/agent-chat-pane--focused/);

    const pinned = resources.getByRole("button", {
      name: "Pinned resource, new Preview available",
    });
    await expect(pinned).toContainText("Pinned");
    await expect(pinned).toContainText("New");
    await expect(previewTab).toHaveAccessibleDescription("New resource");
    await pinned.click();
    const following = resources.getByRole("button", {
      name: "Following latest resource",
    });
    await expect(following).toHaveAttribute("aria-pressed", "true");
    await following.hover();
    expect(
      await following.evaluate(
        (node) => getComputedStyle(node).backgroundColor,
      ),
    ).toBe(
      await page.evaluate(() => {
        const probe = document.createElement("span");
        probe.style.color = "var(--color-surface-active)";
        document.body.append(probe);
        const resolved = getComputedStyle(probe).color;
        probe.remove();
        return resolved;
      }),
    );
    await expect(previewTab).toHaveAttribute("aria-selected", "true");
    await expect(
      page.getByRole("region", {
        name: "Chat pane Editor consistency pass",
        exact: true,
      }),
    ).toHaveClass(/agent-chat-pane--focused/);

    const rail = page.getByRole("complementary", {
      name: "Projects and chats",
    });
    await rail
      .getByRole("button", { name: /Open chat Agent navigation polish/ })
      .click();
    await expect(page.locator(".agent-chat-pane")).toHaveCount(2);
    await expect(
      page.getByRole("region", {
        name: "Chat pane Agent navigation polish",
        exact: true,
      }),
    ).toHaveClass(/agent-chat-pane--focused/);
    await expect(
      resources.getByRole("tab", {
        name: "Diff src/workbench/AgentChatPanes.tsx",
      }),
    ).toHaveAttribute("aria-selected", "true");

    await rail
      .getByRole("button", { name: /Open chat Editor consistency pass/ })
      .click();
    await expect(page.locator(".agent-chat-pane")).toHaveCount(2);
    await expect(
      page.getByRole("region", {
        name: "Chat pane Editor consistency pass",
        exact: true,
      }),
    ).toHaveClass(/agent-chat-pane--focused/);
    await expect(previewTab).toHaveAttribute("aria-selected", "true");
    await expectNoViewportOverflow(page);
  });

  test("keeps the same File and Diff document language in Editor and Agent", async ({
    page,
    context,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "editor", "editor");

    // Authority names for the document syntax palette declared once in
    // global.css. Values are read live below, never assumed from source.
    const docSyntaxTokens = [
      "--doc-syntax-meta",
      "--doc-syntax-keyword",
      "--doc-syntax-atom",
      "--doc-syntax-literal",
      "--doc-syntax-string",
      "--doc-syntax-regexp",
      "--doc-syntax-variable-definition",
      "--doc-syntax-variable-local",
      "--doc-syntax-type",
      "--doc-syntax-class",
      "--doc-syntax-variable-special",
      "--doc-syntax-property-definition",
      "--doc-syntax-comment",
      "--doc-syntax-invalid",
    ];

    const readFileAppearance = async (target: Page) => {
      await expect(target.locator(".file-surface .cm-gutters")).toBeVisible();
      // Highlight spans carry the runtime's generated classes, so wait for
      // any classed span inside a rendered line: actually-rendered syntax.
      await expect(
        target
          .locator(".file-surface .cm-content .cm-line span[class]")
          .first(),
      ).toBeVisible();
      return target.locator(".file-surface").evaluate((surface, tokens) => {
        const header = surface.querySelector<HTMLElement>(".file-header");
        const editor = surface.querySelector<HTMLElement>(".editor-input");
        const content = surface.querySelector<HTMLElement>(".cm-content");
        const gutters = surface.querySelector<HTMLElement>(".cm-gutters");
        const activeLine = surface.querySelector<HTMLElement>(".cm-activeLine");
        if (!header || !editor || !content || !gutters || !activeLine) {
          throw new Error("File appearance probe could not find the document");
        }
        const selectionStyle = getComputedStyle(content, "::selection");
        const codeColor = getComputedStyle(editor).color;
        const highlight = Array.from(
          content.querySelectorAll(".cm-line span[class]"),
        )
          .map((node) => ({
            text: (node as HTMLElement).textContent ?? "",
            color: getComputedStyle(node as HTMLElement).color,
          }))
          .filter((span) => span.color !== codeColor);
        highlight.sort((a, b) =>
          a.color < b.color
            ? -1
            : a.color > b.color
              ? 1
              : a.text < b.text
                ? -1
                : 1,
        );
        const probe = surface.ownerDocument.createElement("span");
        surface.appendChild(probe);
        const authority = tokens.map((token) => {
          probe.style.setProperty("color", `var(${token})`);
          return getComputedStyle(probe).color;
        });
        probe.remove();
        return {
          canvas: getComputedStyle(surface).backgroundColor,
          text: codeColor,
          header: getComputedStyle(header).backgroundColor,
          headerHeight: header.getBoundingClientRect().height,
          gutter: getComputedStyle(gutters).color,
          gutterDivider: getComputedStyle(gutters).borderRightColor,
          activeLine: getComputedStyle(activeLine).backgroundColor,
          caret: getComputedStyle(content).caretColor,
          selection: selectionStyle.backgroundColor,
          selectionText: selectionStyle.color,
          syntaxHighlight: highlight,
          syntaxAuthority: authority,
        };
      }, docSyntaxTokens);
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
            const { useEditorSettingsStore } =
              await import("/src/app/editorSettingsStore.ts");
            return useEditorSettingsStore.getState().persistenceReady;
          }),
        )
        .toBe(true);
      await target.evaluate(async () => {
        const { useEditorSettingsStore } =
          await import("/src/app/editorSettingsStore.ts");
        useEditorSettingsStore.getState().setSettings({
          "editor.fontSize": 16,
          "editor.tabSize": 4,
          "editor.wordWrap": true,
          "editor.lineNumbers": false,
          "editor.renderWhitespace": true,
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
    await agentEditor.press("ControlOrMeta+f");
    const agentFind = agentPage.getByRole("search", { name: "Find in file" });
    await agentFind
      .getByRole("textbox", { name: "Find text" })
      .fill("ChatSurface");
    await expectFindContained(agentPage);
    await expect(agentFind.getByRole("status")).toContainText("matches");
    // Matches are marked in the Agent window too, from the document palette.
    expect(
      await agentPage
        .locator(".cm-findMatch")
        .first()
        .evaluate((node) => getComputedStyle(node).backgroundColor),
    ).not.toBe("rgba(0, 0, 0, 0)");
    expect(
      await agentFind.evaluate((node) => {
        const query = node.querySelector<HTMLElement>(".editor-find__query");
        const status = node.querySelector<HTMLElement>(".editor-find__status");
        const buttons = Array.from(node.querySelectorAll("button"));
        // The Replace chevron leads the row, as in VS Code.
        const expand = node.querySelector(".editor-find__expand");
        if (!query || !status || buttons[0] !== expand) return null;
        const queryBounds = query.getBoundingClientRect();
        const controlBounds = [status, ...buttons.slice(1)].map((control) =>
          control.getBoundingClientRect(),
        );
        return {
          display: getComputedStyle(node).display,
          buttonCount: buttons.length,
          controlsShareQueryRow: controlBounds.every(
            (bounds) =>
              Math.abs(
                bounds.top +
                  bounds.height / 2 -
                  (queryBounds.top + queryBounds.height / 2),
              ) <= 1,
          ),
          queryLeadsControls: controlBounds.every(
            (bounds) => queryBounds.right <= bounds.left + 1,
          ),
          controlsVisible: controlBounds.every(
            (bounds) => bounds.width > 0 && bounds.height > 0,
          ),
          noOverflow: node.scrollWidth <= node.clientWidth,
        };
      }),
    ).toEqual({
      display: "grid",
      buttonCount: 7,
      controlsShareQueryRow: true,
      queryLeadsControls: true,
      controlsVisible: true,
      noOverflow: true,
    });
    await agentFind.getByRole("textbox", { name: "Find text" }).focus();
    await agentPage.keyboard.press("Tab");
    expect(
      await agentFind.evaluate(
        (node) =>
          node.contains(document.activeElement) &&
          document.activeElement?.tagName === "BUTTON",
      ),
    ).toBe(true);
    await agentFind.getByRole("button", { name: "Close find" }).click();

    expect(agentAppearance).toEqual(editorAppearance);
    // Actually-rendered highlight spans: non-empty, multi-role, identical
    // across modes, and every color resolves from the live token authority.
    expect(agentAppearance.syntaxHighlight.length).toBeGreaterThan(0);
    expect(agentAppearance.syntaxHighlight.length).toBeLessThan(500);
    expect(
      new Set(agentAppearance.syntaxHighlight.map((span) => span.color)).size,
    ).toBeGreaterThanOrEqual(2);
    expect(agentAppearance.syntaxAuthority).toEqual(
      editorAppearance.syntaxAuthority,
    );
    for (const span of agentAppearance.syntaxHighlight) {
      expect(agentAppearance.syntaxAuthority).toContain(span.color);
    }
    expect(customAgentAppearance).toEqual(customEditorAppearance);
    expect(customEditorAppearance).toMatchObject({
      fontSize: "16px",
      lineWrapping: true,
      lineNumbers: 0,
      tabSize: "4",
    });
    expect(customEditorAppearance.whitespace).toBeGreaterThan(0);
    await expect(agentPage.locator(".file-header__metadata")).toBeVisible();
    await expect(agentPage.locator(".file-header__metadata")).toContainText(
      "UTF-8 · LF",
    );
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
        const addCount = surface.querySelector<HTMLElement>(
          ".diff-header__counts span:first-child",
        );
        const deleteCount = surface.querySelector<HTMLElement>(
          ".diff-header__counts span:last-child",
        );
        const hunkHeader =
          surface.querySelector<HTMLElement>(".diff-hunk__header");
        if (
          !header ||
          !toolbar ||
          !code ||
          !gutter ||
          !added ||
          !removed ||
          !addCount ||
          !deleteCount ||
          !hunkHeader
        ) {
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
          gutterDivider: getComputedStyle(gutter).borderRightColor,
          added: getComputedStyle(added).backgroundColor,
          removed: getComputedStyle(removed).backgroundColor,
          addForeground: getComputedStyle(addCount).color,
          deleteForeground: getComputedStyle(deleteCount).color,
          hunkForeground: getComputedStyle(hunkHeader).color,
          hunkBackground: getComputedStyle(hunkHeader).backgroundColor,
          selection: getComputedStyle(code, "::selection").backgroundColor,
          selectionText: getComputedStyle(code, "::selection").color,
        };
      });

    const editorDiffPage = await context.newPage();
    await editorDiffPage.setViewportSize({ width: 1380, height: 880 });
    // The immutable snapshot fixture must open through the Agent FileChange
    // card. Editor's document-language comparison is intentionally separate:
    // it seeds the repository-style visual diff through the Editor's real
    // canvas, rather than retaining an Agent-only snapshot after its owner
    // unmounts on a mode switch.
    await openScenario(editorDiffPage, "editor", "editor");
    await editorDiffPage.evaluate(
      async ({ resource, diff }) => {
        const { useWorkbenchStore } = await import("/src/app/store.ts");
        const { diffCacheKey: cacheKey } =
          await import("/src/domain/diffIdentity.ts");
        const snapshot = useWorkbenchStore.getState();
        await snapshot.openDiff(resource);
        useWorkbenchStore.setState({
          diffs: {
            ...useWorkbenchStore.getState().diffs,
            [cacheKey(undefined, resource)]: { phase: "ready", value: diff },
          },
        });
      },
      {
        resource: fileResource,
        diff: snapshotFileDiff(
          fileResource,
          visualSnapshotOldText,
          visualSnapshotNewText,
        ),
      },
    );
    await expect(
      editorDiffPage.locator(".diff-surface .diff-editor"),
    ).toBeVisible();
    const editorDiffAppearance = await readDiffAppearance(editorDiffPage);

    const agentDiffPage = await context.newPage();
    await agentDiffPage.setViewportSize({ width: 1380, height: 880 });
    await openAgentScenario(agentDiffPage, "diff");
    await openAgentSnapshotDiff(agentDiffPage);
    const agentDiffAppearance = await readDiffAppearance(agentDiffPage);

    expect(agentDiffAppearance).toEqual(editorDiffAppearance);
    expect(agentDiffAppearance.addForeground).not.toBe(
      agentDiffAppearance.text,
    );
    expect(agentDiffAppearance.deleteForeground).not.toBe(
      agentDiffAppearance.text,
    );

    const readDocumentRows = async (
      target: Page,
      selector: string,
      rowSelector: string,
    ) => {
      await expect(
        target.locator(selector + " " + rowSelector).first(),
      ).toBeVisible();
      return target.locator(selector).evaluate(
        (node, probe) => {
          const region = node.getBoundingClientRect();
          const rows = Array.from(
            node.querySelectorAll<HTMLElement>(probe.rowSelector),
          ).slice(0, probe.limit);
          if (rows.length === 0) {
            throw new Error("Document row probe found no rendered rows");
          }
          return rows.map((row) => {
            const style = getComputedStyle(row);
            const bounds = row.getBoundingClientRect();
            return {
              text: row.textContent ?? "",
              height: Math.round(bounds.height),
              rendered: bounds.width > 0 && bounds.height > 0,
              insideRegion:
                bounds.left >= region.left - 1 && bounds.top >= region.top - 1,
              offsetFromRegionTop: Math.round(bounds.top - region.top),
              color: style.color,
              background: style.backgroundColor,
              borderLeft: style.borderLeftColor,
              fontFamily: style.fontFamily,
              fontSize: style.fontSize,
              fontStyle: style.fontStyle,
              fontWeight: style.fontWeight,
              letterSpacing: style.letterSpacing,
              lineHeight: style.lineHeight,
              tabSize: style.tabSize,
              whiteSpace: style.whiteSpace,
            };
          });
        },
        { rowSelector, limit: 24 },
      );
    };

    const compareDocumentRegion = async (
      first: Page,
      second: Page,
      selector: string,
      rowSelector: string,
      minimumRows: number,
    ) => {
      const [firstRows, secondRows] = await Promise.all([
        readDocumentRows(first, selector, rowSelector),
        readDocumentRows(second, selector, rowSelector),
      ]);
      expect(firstRows.length).toBeGreaterThanOrEqual(minimumRows);
      expect(
        firstRows
          .map((row) => row.text)
          .join("")
          .trim().length,
      ).toBeGreaterThan(0);
      expect(firstRows.every((row) => row.rendered && row.insideRegion)).toBe(
        true,
      );
      expect(secondRows).toEqual(firstRows);
    };

    await compareDocumentRegion(
      editorDiffPage,
      agentDiffPage,
      ".diff-surface .diff-editor",
      ".diff-line",
      2,
    );
    await expectNoViewportOverflow(editorDiffPage);
    await expectNoViewportOverflow(agentDiffPage);
    await editorDiffPage.close();
    await agentDiffPage.close();

    const editorFilePixelPage = await context.newPage();
    await editorFilePixelPage.setViewportSize({ width: 1380, height: 880 });
    await openScenario(editorFilePixelPage, "editor", "editor");
    const agentFilePixelPage = await context.newPage();
    await agentFilePixelPage.setViewportSize({ width: 1380, height: 880 });
    await openScenario(agentFilePixelPage, "file", "agent");
    await compareDocumentRegion(
      editorFilePixelPage,
      agentFilePixelPage,
      ".file-surface .editor-input",
      ".cm-line",
      3,
    );
    await expectNoViewportOverflow(editorFilePixelPage);
    await expectNoViewportOverflow(agentFilePixelPage);
    await editorFilePixelPage.close();
    await agentFilePixelPage.close();
  });
});

test.describe("AV-03 Agent document ready baselines", () => {
  test.use({ viewport: { width: 1380, height: 880 } });

  test("agent-file-ready stays visually stable", async ({ page }) => {
    const workbench = await openAgentScenario(page, "file");
    await expect(
      page.getByRole("textbox", {
        name: "Edit src/workbench/AgentChatPanes.tsx",
      }),
    ).toBeVisible();
    await expect(
      page.locator(".file-surface .cm-content .cm-line span[class]").first(),
    ).toBeVisible();
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("agent-file-ready.png");
  });

  test("agent-diff-ready opens the immutable snapshot from its FileChange card", async ({
    page,
  }) => {
    await installFixedClock(page);
    await installLayoutMode(page, "agent");
    await page.goto("/");
    await page.evaluate(async () => {
      const { getWorkbenchPorts } =
        await import("/src/ports/workbenchRuntime.ts");
      const repository = getWorkbenchPorts().repository;
      const getFileDiff = repository.getFileDiff.bind(repository);
      repository.getFileDiff = async (...args) => {
        document.documentElement.dataset.visualGitDiffCalls = String(
          Number(document.documentElement.dataset.visualGitDiffCalls ?? "0") +
            1,
        );
        return getFileDiff(...args);
      };
      document.documentElement.dataset.visualGitDiffCalls = "0";
    });
    await installScenario(page, "diff");
    await page.evaluate(() => document.fonts.ready);
    const workbench = page.locator("main.workbench");
    await expandAgentSnapshotEdits(page);
    await expect(
      page.getByRole("button", {
        name: "Open diff Sage / src/workbench/AgentChatPanes.tsx",
      }),
    ).toBeVisible();
    await expect(
      page.getByRole("table", { name: "Unified file diff" }),
    ).toHaveCount(0);
    await openAgentSnapshotDiff(page);
    await expect(page.getByText("maximum: 1", { exact: false })).toBeVisible();
    await expect(page.getByText("maximum: 2", { exact: false })).toBeVisible();
    await expect(
      page.getByText("Saved agent change", { exact: true }),
    ).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.dataset.visualGitDiffCalls,
      ),
    ).toBe("0");
    await expect(page.locator(".diff-line--added").first()).toBeVisible();
    await expect(page.locator(".diff-line--removed").first()).toBeVisible();
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("agent-diff-ready.png");
  });

  test("agent-diff-ready shows a typed unavailable snapshot without Git fallback", async ({
    page,
  }) => {
    await installFixedClock(page);
    await installLayoutMode(page, "agent");
    await page.goto("/");
    await installScenario(page, "diff");
    await page.evaluate(async () => {
      const { AcpDesktopError, acpDesktop } =
        await import("/src/services/acpDesktop.ts");
      acpDesktop.getFileSnapshot = async () => {
        throw new AcpDesktopError({
          kind: "unavailable",
          reason: "sessionUnavailable",
          message: "The saved edit is unavailable for this Chat session.",
        });
      };
    });
    await expandAgentSnapshotEdits(page);
    await page
      .getByRole("button", {
        name: "Open diff Sage / src/workbench/AgentChatPanes.tsx",
      })
      .click();
    await expect(
      page.getByText("Saved agent change", { exact: true }),
    ).toBeVisible();
    await expect(page.getByRole("alert")).toContainText(
      "The saved edit is unavailable for this Chat session.",
    );
    await expect(
      page.getByText("Working tree changes", { exact: true }),
    ).toHaveCount(0);
  });
});

test.describe("V6a Agent right-resource state shell", () => {
  test.use({ viewport: { width: 1_380, height: 880 } });

  test("uses one honest collapsed toggle and returns focus after closing", async ({
    page,
  }) => {
    const workbench = await openAgentScenario(page, "agent-single");
    const toggle = page.getByRole("button", {
      name: "Show files and preview · Waiting",
    });
    await expect(toggle).toBeVisible();
    await expect(toggle.locator("svg")).toHaveCount(1);
    await expect(toggle.locator("small")).toHaveCount(0);

    await page
      .getByRole("button", { name: "Show chat resources", exact: true })
      .focus();
    await page.keyboard.press("Tab");
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
      page.getByRole("heading", { name: "Waiting for this Chat" }),
    ).toBeVisible();
    await expect(
      page.getByText(/Open a file or change from the conversation/),
    ).toBeVisible();
    await expect(workbench).toHaveScreenshot("agent-resource-empty.png");
    const collapse = page.getByRole("button", {
      name: "Collapse files, changes, and preview",
    });
    await collapse.focus();
    await page.keyboard.press("Space");
    await expect(
      page.getByRole("button", {
        name: "Show files and preview · Waiting",
      }),
    ).toBeFocused();
    await expectNoViewportOverflow(page);
  });

  test("fits the collapsed ready Preview receipt in the desktop rail", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1_380, height: 880 });
    const workbench = await openAgentScenario(page, "preview");
    await page
      .getByRole("button", { name: "Collapse files, changes, and preview" })
      .click();
    await expect(workbench).not.toHaveClass(/workbench--compact/);
    const toggle = page.getByRole("button", {
      name: "Show files and preview · Ready · 1 resource",
    });
    await expect(toggle).toHaveAttribute(
      "title",
      "Show files and preview · Ready · 1 resource",
    );
    await expect(toggle.locator(".agent-artifact-pane__edge-copy")).toHaveCount(
      0,
    );
    await expect(toggle.locator(".agent-artifact-pane__count")).toHaveText("1");
    const height = await toggle.evaluate(
      (node) => node.getBoundingClientRect().height,
    );
    expect(height).toBeLessThanOrEqual(44);
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
    await openAgentSnapshotDiff(page);
    await expect(
      page.getByRole("table", { name: "Unified file diff" }),
    ).toBeVisible();
    await expect(
      page.locator(".diff-toolbar").getByText("Read-only", { exact: true }),
    ).toBeVisible();
    await expect(page.getByText(/Explorer focused/i)).toHaveCount(0);

    await setResourceState(page, "diff-loading");
    await expect(
      page.getByRole("status").filter({ hasText: "Loading diff…" }),
    ).toBeVisible();
    await settleTranscriptAtStart(page);
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
      if (resource === "diff") await openAgentSnapshotDiff(page);
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
      name: "Show files and preview · Waiting",
    });
    // CP-27b: the collapsed toggle is a header control, not a 44px edge rail.
    const collapsed = await toggle.evaluate((node) => {
      const bounds = node.getBoundingClientRect();
      return {
        count: node.querySelector(".agent-artifact-pane__count")?.textContent,
        inHeader: node.closest(".agent-chat-center__header") !== null,
        right: bounds.right,
        height: bounds.height,
        width: bounds.width,
      };
    });
    expect(collapsed.count).toBeUndefined();
    expect(collapsed.inHeader).toBe(true);
    expect(collapsed.width).toBeGreaterThanOrEqual(24);
    expect(collapsed.height).toBeGreaterThanOrEqual(24);
    expect(collapsed.right).toBeLessThanOrEqual(720);
    await expect(page.locator(".agent-artifact-pane")).toHaveCount(0);

    await toggle.click();
    await expect(
      page.getByRole("heading", { name: "Waiting for this Chat" }),
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
      name: "Show files and preview · Waiting",
    });
    await expect(toggle.locator("small")).toHaveCount(0);
    await toggle.click();
    await expect(
      page.getByRole("heading", { name: "Waiting for this Chat" }),
    ).toBeVisible();
    await expectNoViewportOverflow(page);
  });

  for (const resource of ["file", "diff"] as const) {
    test(`keeps ${resource} states contained`, async ({ page }) => {
      const workbench = await openAgentScenario(page, resource);
      if (resource === "diff") await openAgentSnapshotDiff(page);
      await expect(workbench).toHaveClass(/workbench--compact/);
      await exerciseResourceStates(page, resource);
    });
  }
});

test.describe("AV-01 right pane Preview chrome shell", () => {
  test.use({ viewport: { width: 1_380, height: 880 } });

  test("agent-preview-idle stays visually stable", async ({ page }) => {
    const workbench = await openAgentScenario(page, "preview");
    await page.evaluate(async () => {
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/resourceVisualFixtures.ts");
      await fixtures.installVisualAgentPreviewIdleFixture();
    });
    const resources = page.getByLabel("Agent artifacts", { exact: true });
    await expect(resources.getByRole("tab", { name: "Preview" })).toHaveCount(
      0,
    );
    await expect(
      resources.getByRole("heading", { name: "Waiting for this Chat" }),
    ).toBeVisible();
    await expect(
      resources.getByText(
        "Open a file or change from the conversation. Preview will appear when this Chat starts a loopback server.",
        { exact: true },
      ),
    ).toBeVisible();
    await expect(resources.locator(".harness-browser")).toHaveCount(0);
    await expect(
      resources.getByRole("textbox", { name: "Preview address" }),
    ).toHaveCount(0);
    await expect(page.getByTitle("Sage browser")).toHaveCount(0);
    const waiting = resources.getByRole("region", {
      name: "Waiting for this Chat",
    });
    expect(
      await waiting.evaluate((node) => {
        const canvas = node.closest(".agent-artifact-pane__canvas")!;
        const bounds = node.getBoundingClientRect();
        const parent = canvas.getBoundingClientRect();
        return (
          bounds.left >= parent.left &&
          bounds.right <= parent.right &&
          bounds.top >= parent.top &&
          bounds.bottom <= parent.bottom
        );
      }),
    ).toBe(true);
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("agent-preview-idle.png");
  });

  test("agent-preview-active stays visually stable", async ({ page }) => {
    const workbench = await openAgentScenario(page, "preview");
    const resources = page.getByLabel("Agent artifacts", { exact: true });
    await expect(
      resources.getByRole("tab", { name: "Preview" }),
    ).toHaveAttribute("aria-selected", "true");
    await expect(page.getByTitle("Sage browser")).toBeVisible();
    await expect(
      resources.getByRole("textbox", { name: "Preview address" }),
    ).toBeVisible();
    const activeGeometry = await resources.evaluate((root) => {
      const browser = root.querySelector<HTMLElement>(".harness-browser");
      const chrome = root.querySelector<HTMLElement>(
        ".harness-browser__chrome",
      );
      const button = root.querySelector<HTMLElement>(
        ".harness-browser__chrome button",
      );
      const input = root.querySelector<HTMLElement>(
        ".harness-browser__address input",
      );
      const body = root.querySelector<HTMLElement>(".harness-browser__frame");
      const denied = root.querySelector<HTMLElement>(
        ".harness-browser__denied",
      );
      if (!browser || !chrome || !button || !input || !body) {
        throw new Error("Preview chrome geometry probe missing");
      }
      return {
        chromeHeight: chrome.getBoundingClientRect().height,
        buttonHeight: getComputedStyle(button).height,
        inputHeight: getComputedStyle(input).height,
        browser: browser.getBoundingClientRect().toJSON(),
        chrome: chrome.getBoundingClientRect().toJSON(),
        body: body.getBoundingClientRect().toJSON(),
        denied: denied ? denied.getBoundingClientRect().toJSON() : null,
      };
    });
    expect(activeGeometry.chromeHeight).toBe(36);
    expect(activeGeometry.buttonHeight).toBe("28px");
    expect(activeGeometry.inputHeight).toBe("28px");
    expect(activeGeometry.denied).toBeNull();
    expect(activeGeometry.body.y).toBeGreaterThanOrEqual(
      activeGeometry.chrome.y + activeGeometry.chrome.height - 1,
    );
    expect(
      activeGeometry.body.y + activeGeometry.body.height,
    ).toBeLessThanOrEqual(
      activeGeometry.browser.y + activeGeometry.browser.height + 1,
    );
    const activeAddress = resources.getByRole("textbox", {
      name: "Preview address",
    });
    const activeHref = await activeAddress.inputValue();
    await activeAddress.fill("https://example.com/");
    await activeAddress.press("Enter");
    await expect(
      resources.getByRole("alert").filter({
        hasText: "Only loopback HTTP(S) with a port",
      }),
    ).toBeVisible();
    expect(await page.getByTitle("Sage browser").getAttribute("src")).toBe(
      activeHref,
    );
    const activeDenied = await resources.evaluate((root) => {
      const browser = root.querySelector<HTMLElement>(".harness-browser");
      const chrome = root.querySelector<HTMLElement>(
        ".harness-browser__chrome",
      );
      const denied = root.querySelector<HTMLElement>(
        ".harness-browser__denied",
      );
      const body = root.querySelector<HTMLElement>(".harness-browser__frame");
      if (!browser || !chrome || !denied || !body) {
        throw new Error("Preview denied-row probe missing");
      }
      return {
        browser: browser.getBoundingClientRect().toJSON(),
        chrome: chrome.getBoundingClientRect().toJSON(),
        denied: denied.getBoundingClientRect().toJSON(),
        body: body.getBoundingClientRect().toJSON(),
      };
    });
    expect(activeDenied.denied.y).toBeGreaterThanOrEqual(
      activeDenied.chrome.y + activeDenied.chrome.height - 1,
    );
    expect(activeDenied.body.y).toBeGreaterThanOrEqual(
      activeDenied.denied.y + activeDenied.denied.height - 1,
    );
    expect(activeDenied.body.y + activeDenied.body.height).toBeLessThanOrEqual(
      activeDenied.browser.y + activeDenied.browser.height + 1,
    );
    await activeAddress.fill(activeHref);
    await expect(resources.locator(".harness-browser__denied")).toHaveCount(0);
    expect(await page.getByTitle("Sage browser").getAttribute("src")).toBe(
      activeHref,
    );
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("agent-preview-active.png");
  });

  test("agent-artifacts-long-paths stays visually stable", async ({ page }) => {
    const workbench = await openAgentScenario(page, "file");
    await page.evaluate(async () => {
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/resourceVisualFixtures.ts");
      await fixtures.installVisualAgentLongPathFixture();
    });
    const resources = page.getByLabel("Agent artifacts", { exact: true });
    const longPath =
      "src/workbench/nested/panels/with/very/long/directory/names/for/ellipsis/AgentChatPanes.tsx";
    const assertLongTabLegible = async (tabName: string) => {
      const tab = resources.getByRole("tab", { name: tabName });
      await tab.click();
      await expect(tab).toHaveAttribute("aria-selected", "true");
      if (tabName.startsWith("File ")) {
        await expect(
          page
            .locator(
              ".agent-artifact-pane__canvas .file-surface .cm-content .cm-line",
            )
            .first(),
        ).toBeVisible();
      } else {
        await expect(
          resources.getByRole("table", { name: "Unified file diff" }),
        ).toBeVisible();
      }
      await tab.scrollIntoViewIfNeeded();
      await expect(tab).toBeVisible();
      await expect(tab).toHaveAccessibleName(tabName);
      await expect(tab).toHaveAttribute("title", tabName);
      const legibility = await tab.evaluate((element) => {
        const basename = element.querySelector<HTMLElement>(".tab__basename")!;
        const directory = element.querySelector<HTMLElement>(".tab__dir")!;
        const box = element.getBoundingClientRect();
        const baseBox = basename.getBoundingClientRect();
        const scroller = element.closest<HTMLElement>(".tab-strip__scroll")!;
        const scrollerBox = scroller.getBoundingClientRect();
        return {
          tabInsideScroller:
            box.left >= scrollerBox.left - 1 &&
            box.right <= scrollerBox.right + 1,
          basenameText: basename.textContent,
          directoryDisplay: getComputedStyle(directory).display,
          basenameEllipsis: getComputedStyle(basename).textOverflow,
          basenameWidth: baseBox.width,
          basenameInsideTab:
            baseBox.left >= box.left - 1 && baseBox.right <= box.right + 1,
        };
      });
      expect(legibility.basenameText).toBe("AgentChatPanes.tsx");
      expect(legibility.tabInsideScroller).toBe(true);
      expect(legibility.directoryDisplay).toBe("none");
      expect(legibility.basenameEllipsis).toBe("ellipsis");
      expect(legibility.basenameWidth).toBeGreaterThan(40);
      expect(legibility.basenameInsideTab).toBe(true);
    };
    await assertLongTabLegible(`File ${longPath}`);
    await assertLongTabLegible(`Diff ${longPath}`);
    await resources.getByRole("tab", { name: `File ${longPath}` }).click();
    await expect(
      resources.getByRole("tab", { name: `File ${longPath}` }),
    ).toHaveAttribute("aria-selected", "true");
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot("agent-artifacts-long-paths.png");
  });
});

test.describe("AV-02 Preview pane sizing, keyboard, narrow and zoom", () => {
  test("agent-preview-narrow-720x560", async ({ page }) => {
    await page.setViewportSize({ width: 720, height: 560 });
    const workbench = await openAgentScenario(page, "preview");
    const resources = page.getByLabel("Agent artifacts", { exact: true });
    const browser = resources.locator(".harness-browser");
    const address = resources.getByRole("textbox", {
      name: "Preview address",
    });
    const trigger = resources.getByRole("button", {
      name: "Preview controls",
    });

    await expect(
      resources.getByRole("tab", { name: "Preview" }),
    ).toHaveAttribute("aria-selected", "true");
    await expect(address).toBeVisible();
    await expect(trigger).toBeVisible();
    await expect(resources.getByRole("button", { name: "Back" })).toBeHidden();
    await trigger.click();
    const menu = page.getByRole("menu", { name: "Preview controls" });
    await expect(menu).toBeVisible();
    await expect(menu.getByRole("menuitem", { name: "Back" })).toBeDisabled();
    await expect(
      menu.getByRole("menuitem", { name: "Reload preview" }),
    ).toBeEnabled();
    expect(
      await menu.evaluate((node) => {
        const bounds = node.getBoundingClientRect();
        return (
          bounds.left >= 8 &&
          bounds.right <= innerWidth - 8 &&
          bounds.top >= 8 &&
          bounds.bottom <= innerHeight - 8 &&
          node.scrollWidth <= node.clientWidth
        );
      }),
    ).toBe(true);
    await page.emulateMedia({ reducedMotion: "reduce" });
    await expect(menu).toHaveCSS("animation-name", "none");
    const geometry = await browser.evaluate((node) => {
      const body = node.querySelector<HTMLElement>(
        ".harness-browser__frame, .harness-browser > .resource-state",
      );
      if (!body) throw new Error("Preview viewport is missing");
      const canvas = node.closest<HTMLElement>(".agent-artifact-pane__canvas");
      if (!canvas) throw new Error("Preview resource canvas is missing");
      const browserBounds = node.getBoundingClientRect();
      const bodyBounds = body.getBoundingClientRect();
      return {
        browser: browserBounds.toJSON(),
        body: bodyBounds.toJSON(),
        canvas: canvas.getBoundingClientRect().toJSON(),
      };
    });
    expect(geometry.body.height).toBeGreaterThanOrEqual(240);
    expect(geometry.body.bottom).toBeLessThanOrEqual(
      geometry.browser.bottom + 1,
    );
    expect(geometry.browser.y).toBeGreaterThanOrEqual(geometry.canvas.y - 1);
    expect(geometry.browser.y).toBeLessThanOrEqual(geometry.canvas.y + 1);
    expect(geometry.browser.bottom).toBeLessThanOrEqual(
      geometry.canvas.bottom + 1,
    );
    expect(geometry.browser.bottom).toBeGreaterThanOrEqual(
      geometry.canvas.bottom - 1,
    );
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot(
      "agent-preview-narrow-720x560.png",
    );
  });

  test.describe("agent-preview-zoom-200 DPR proxy", () => {
    test.use({
      viewport: { width: 690, height: 440 },
      deviceScaleFactor: 2,
    });

    test("agent-preview-zoom-200", async ({ page }) => {
      const workbench = await openAgentScenario(page, "preview");
      const resources = page.getByLabel("Agent artifacts", { exact: true });
      const trigger = resources.getByRole("button", {
        name: "Preview controls",
      });
      const address = resources.getByRole("textbox", {
        name: "Preview address",
      });

      await expect(workbench).toHaveClass(/workbench--compact/);
      await expect
        .poll(() =>
          page.evaluate(() => ({
            devicePixelRatio: window.devicePixelRatio,
            innerWidth: window.innerWidth,
          })),
        )
        .toEqual({ devicePixelRatio: 2, innerWidth: 690 });
      await expect(address).toBeVisible();
      await expect(trigger).toBeVisible();
      await trigger.click();
      await expect(
        page.getByRole("menu", { name: "Preview controls" }),
      ).toBeVisible();
      const geometry = await resources
        .locator(".harness-browser")
        .evaluate((node) => {
          const browser = node.getBoundingClientRect();
          const frame = node.querySelector<HTMLElement>(
            ".harness-browser__frame",
          );
          const addressInput = node.querySelector<HTMLElement>(
            ".harness-browser__address input",
          );
          const canvas = node.closest<HTMLElement>(
            ".agent-artifact-pane__canvas",
          );
          if (!frame || !addressInput || !canvas) {
            throw new Error("Zoom Preview geometry probe missing");
          }
          return {
            address: addressInput.getBoundingClientRect().toJSON(),
            browser: browser.toJSON(),
            canvas: canvas.getBoundingClientRect().toJSON(),
            content: frame.getBoundingClientRect().toJSON(),
          };
        });
      expect(geometry.address.width).toBeGreaterThan(0);
      expect(geometry.address.right).toBeLessThanOrEqual(
        geometry.browser.right + 1,
      );
      expect(geometry.content.height).toBeGreaterThanOrEqual(240);
      expect(geometry.content.bottom).toBeLessThanOrEqual(
        geometry.browser.bottom + 1,
      );
      expect(geometry.browser.bottom).toBeGreaterThanOrEqual(
        geometry.canvas.bottom - 1,
      );
      expect(geometry.browser.bottom).toBeLessThanOrEqual(
        geometry.canvas.bottom + 1,
      );
      await expectNoViewportOverflow(page);
      await expect(workbench).toHaveScreenshot("agent-preview-zoom-200.png");
    });
  });

  test("agent-preview-keyboard-focus", async ({ page }) => {
    const workbench = await openAgentScenario(page, "preview");
    const resources = page.getByLabel("Agent artifacts", { exact: true });
    const previewTab = resources.getByRole("tab", { name: "Preview" });
    const collapse = resources.getByRole("button", {
      name: "Collapse files, changes, and preview",
    });
    const controls = resources.locator(".harness-browser__controls");
    const reload = controls.getByRole("button", { name: "Reload preview" });
    const close = controls.getByRole("button", { name: "Close browser" });
    const trigger = resources.locator(".harness-browser__controls-trigger");
    const address = resources.getByRole("textbox", {
      name: "Preview address",
    });
    const frame = resources.getByTitle("Sage browser");

    await expect(reload).toBeVisible();
    await expect(reload).toBeEnabled();
    await expect(close).toBeVisible();
    await expect(trigger).toHaveCSS("display", "none");
    await previewTab.focus();
    await page.keyboard.press("Tab");
    const newBrowser = resources.getByRole("button", {
      name: "New browser tab",
    });
    await expect(newBrowser).toBeFocused();
    await expect(
      resources.getByRole("button", { name: "Expand resource pane" }),
    ).toBeEnabled();
    await page.keyboard.press("Tab");
    await expect(
      resources.getByRole("button", { name: "Expand resource pane" }),
    ).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(collapse).toBeFocused();
    await expect(collapse).toBeEnabled();
    await page.keyboard.press("Shift+Tab");
    await expect(
      resources.getByRole("button", { name: "Expand resource pane" }),
    ).toBeFocused();
    await page.keyboard.press("Shift+Tab");
    await expect(newBrowser).toBeFocused();
    await page.keyboard.press("Tab");
    await page.keyboard.press("Tab");
    await expect(collapse).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(reload).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(close).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(address).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(frame).toBeFocused();
    await page.keyboard.press("Shift+Tab");
    await expect(address).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(previewTab).toBeFocused();

    const splitter = page.getByRole("separator", {
      name: "Resize files and preview",
    });
    const ordinary = await splitter.evaluate((node) => ({
      hitTarget: node.getBoundingClientRect().width,
      seam: getComputedStyle(node, "::after").width,
    }));
    expect(ordinary.hitTarget).toBeGreaterThanOrEqual(8);
    expect(ordinary.seam).toBe("1px");
    await page.emulateMedia({ forcedColors: "active" });
    await splitter.focus();
    await expect
      .poll(() =>
        splitter.evaluate((node) => ({
          seam: getComputedStyle(node, "::after").width,
          outline: getComputedStyle(node).outlineStyle,
        })),
      )
      .toEqual({ seam: "2px", outline: "solid" });
    await page.emulateMedia({ forcedColors: "none" });
    await expectNoViewportOverflow(page);
    await expect(workbench).toHaveScreenshot(
      "agent-preview-keyboard-focus.png",
    );
    await collapse.focus();
    await page.keyboard.press("Space");
    await expect(
      page.getByRole("button", {
        name: "Show files and preview · Ready · 1 resource",
      }),
    ).toBeFocused();
  });
});
