import { expect, test, type Locator, type Page } from "@playwright/test";

import {
  openAgentScenario,
  openAgentSnapshotDiff,
} from "./resourceVisualHelpers";
import {
  openScenario,
  settleChatProjections,
  settleTranscriptAtEnd,
} from "./shared/visualBrowserPrimitives";

test.use({ viewport: { width: 1380, height: 880 } });

async function openCompactHeader(
  page: Page,
  turnState: "completed" | "running" | "failed",
) {
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await page.evaluate(async (state) => {
    const { installChatChromeFixture } =
      await import("/tests/ui/fixtures/chatChromeFixture.ts");
    installChatChromeFixture("codex", false, state, true);
  }, turnState);
  // Remount the header so it reads the fixture's chat and harness state.
  for (const mode of ["Editor", "Agent"]) {
    const radio = page.getByRole("radio", { name: mode, exact: true });
    await radio.locator("..").click();
    await expect(radio).toBeChecked();
  }
  await settleChatProjections(page);
  const centre = await page.locator(".agent-chat-center").boundingBox();
  expect(centre!.width).toBeLessThanOrEqual(400);
  return page.locator(".agent-task-header");
}

// Resolves a colour role through a probe so tests follow the theme.
function roleColour(
  locator: Locator,
  property: "color" | "background-color",
  role: string,
) {
  return locator.evaluate(
    (node, [property, role]) => {
      const probe = document.createElement("span");
      probe.style.setProperty(property, `var(${role})`);
      node.append(probe);
      const colour = getComputedStyle(probe).getPropertyValue(property);
      probe.remove();
      return colour;
    },
    [property, role] as const,
  );
}

async function expectReadableTitle(header: Locator) {
  const title = header.locator(".agent-task-header__title");
  await expect(title).toBeVisible();
  const { width, em } = await title.evaluate((node) => ({
    width: node.getBoundingClientRect().width,
    em: Number.parseFloat(getComputedStyle(node).fontSize),
  }));
  expect(width).toBeGreaterThanOrEqual(3 * em);
}

test.describe("compact Agent chat header", () => {
  // At 640 px the rail stays open and the chat centre is ~315 px wide, inside
  // the compact (<= 400 px) header layout.
  test.use({ viewport: { width: 640, height: 800 } });

  for (const turnState of ["completed", "running", "failed"] as const) {
    test(`keeps the title readable while the turn is ${turnState}`, async ({
      page,
    }) => {
      await expectReadableTitle(await openCompactHeader(page, turnState));
    });
  }

  test("collapses the harness status to an icon and opens it in full", async ({
    page,
  }) => {
    const header = await openCompactHeader(page, "completed");
    const summary = header.getByLabel("Harness status: Sign in required", {
      exact: true,
    });
    await expect(
      summary.locator(".agent-task-header__harness-icon"),
    ).toBeVisible();
    await expect(summary.getByText("Sign in required")).toBeHidden();
    await summary.click();
    const detail = header.locator(".agent-task-header__harness-status > p");
    await expect(detail).toHaveText("Sign in required");
    await expect(detail).toBeVisible();
    const fit = await detail.evaluate((node) => {
      const box = node.getBoundingClientRect();
      const bar = node.closest(".agent-task-header")!.getBoundingClientRect();
      return {
        clipped: node.scrollWidth > node.clientWidth,
        inside: box.left >= bar.left && box.right <= bar.right,
      };
    });
    expect(fit).toEqual({ clipped: false, inside: true });
  });

  test("shows a failed rename's alert without adding a header row", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    const header = page.locator(".agent-task-header");
    const before = await header.boundingBox();
    await header
      .getByRole("button", { name: /^Chat actions for .* task$/ })
      .click();
    await page
      .getByRole("menuitem", { name: /^More chat actions in / })
      .click();
    await page.getByRole("menuitem", { name: "Rename chat" }).click();
    await header.getByRole("textbox", { name: "New chat name" }).fill("   ");
    await page.keyboard.press("Enter");
    const alert = header.getByRole("alert");
    await expect(alert).toHaveText(
      "Chat title must not be empty after trimming",
    );
    await expect(alert).toBeVisible();
    const after = await header.boundingBox();
    expect(after!.height).toBe(before!.height);
    const alertBox = await alert.boundingBox();
    expect(alertBox!.y).toBeGreaterThanOrEqual(after!.y + after!.height - 1);

    // A longer error wraps inside its box instead of spilling past it.
    await header
      .getByRole("textbox", { name: "New chat name" })
      .fill("Renamed\u0007 chat");
    await page.keyboard.press("Enter");
    await expect(alert).toHaveText(
      "Chat title must not contain control characters",
    );
    expect(
      await alert.evaluate((node) => node.scrollWidth <= node.clientWidth),
    ).toBe(true);
  });
});

test.describe("artifact pane", () => {
  async function expectStacked(pane: Locator, selectors: string[]) {
    const gaps = await pane.evaluate((node, parts) => {
      const boxes = parts.map((part) =>
        node.querySelector(`:scope > ${part}`)!.getBoundingClientRect(),
      );
      const paneBottom =
        node.getBoundingClientRect().bottom -
        Number.parseFloat(getComputedStyle(node).borderBottomWidth);
      return [
        ...boxes.slice(1).map((box, index) => box.top - boxes[index].bottom),
        paneBottom - boxes.at(-1)!.bottom,
      ];
    }, selectors);
    for (const gap of gaps) expect(Math.abs(gap)).toBeLessThanOrEqual(0.5);
  }

  test("starts a diff's header and content exactly below the tab header", async ({
    page,
  }) => {
    await openAgentScenario(page, "diff");
    await openAgentSnapshotDiff(page);
    const pane = page.getByLabel("Agent artifacts", { exact: true });
    await expect(pane.locator(".agent-artifact-header")).toBeVisible();
    await expectStacked(pane, [
      ".agent-artifact-pane__header",
      ".agent-artifact-header",
      ".agent-artifact-pane__canvas",
    ]);
  });

  test('puts the new-tab "+" right after the last tab (Codex 67)', async ({
    page,
  }) => {
    await openAgentScenario(page, "diff");
    await openAgentSnapshotDiff(page);
    const header = page
      .getByLabel("Agent artifacts", { exact: true })
      .locator(".agent-artifact-pane__header");
    const edges = await header.evaluate((node) => {
      const box = (selector: string) =>
        node.querySelector(selector)!.getBoundingClientRect();
      const tabs = [...node.querySelectorAll(".tab")];
      return {
        lastTab: tabs.at(-1)!.getBoundingClientRect().right,
        newTab: box(":scope > .agent-artifact-pane__new-tab"),
        collapse: node.lastElementChild!.getBoundingClientRect(),
        header: node.getBoundingClientRect().right,
      };
    });
    expect(Math.abs(edges.newTab.left - edges.lastTab)).toBeLessThanOrEqual(1);
    // The pane controls stay at the far end.
    expect(edges.collapse.left).toBeGreaterThan(edges.newTab.right);
    expect(edges.header - edges.collapse.right).toBeLessThanOrEqual(16);
  });

  test("starts the empty pane's content exactly below the tab header", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    await page.getByRole("button", { name: /^Show files and preview/ }).click();
    const pane = page.getByLabel("Agent artifacts", { exact: true });
    await expect(pane.getByText("Files, changes, and preview")).toBeVisible();
    await expectStacked(pane, [
      ".agent-artifact-pane__header",
      ".agent-artifact-pane__canvas",
    ]);
  });
});

test.describe("narrow title bar", () => {
  test.use({ viewport: { width: 600, height: 800 } });

  for (const mode of ["agent", "editor"] as const) {
    test(`keeps ${mode} title-bar controls apart with an icon-sized search`, async ({
      page,
    }) => {
      await openScenario(page, "agent-single", mode);
      const bar = page.locator(".title-bar");
      const controls = bar.locator(
        '[role="radiogroup"], button:not([role="radiogroup"] *)',
      );
      const boxes = (
        await Promise.all((await controls.all()).map((c) => c.boundingBox()))
      ).filter((box) => box !== null);
      expect(boxes.length).toBeGreaterThanOrEqual(3);
      for (const [index, a] of boxes.entries())
        for (const b of boxes.slice(index + 1))
          expect(
            a.x < b.x + b.width &&
              b.x < a.x + a.width &&
              a.y < b.y + b.height &&
              b.y < a.y + a.height,
          ).toBe(false);
      const search = bar.getByRole("button", {
        name: mode === "agent" ? "Search chats" : "Search files",
        exact: true,
      });
      await expect(search).toBeVisible();
      const box = await search.boundingBox();
      expect(box!.width).toBe(box!.height);
      expect(box!.width).toBeLessThanOrEqual(32);
    });
  }
});

test.describe("Agent rail chat row", () => {
  // Opacity and zero width hide these controls, which toBeVisible ignores;
  // poll because the opacity fades.
  const painted = (locator: Locator) =>
    locator.evaluate((node) => {
      const style = getComputedStyle(node);
      return (
        Number(style.opacity) > 0 && node.getBoundingClientRect().width > 0
      );
    });

  test("trades its state for its actions as keyboard focus moves in", async ({
    page,
  }) => {
    await openScenario(page, "waiting", "agent");
    const chat = page
      .getByRole("complementary", { name: "Projects and chats" })
      .getByRole("button", { name: "Open chat Inspecting workspace, pinned" });
    const row = chat.locator("..");
    const state = row.getByRole("img", { name: "Working" });
    const quick = row.locator(".project-chat__quick-actions");
    const actions = row.getByRole("button", {
      name: "Chat actions for Inspecting workspace",
      exact: true,
    });
    await expect.poll(() => painted(state)).toBe(true);
    await expect.poll(() => painted(actions)).toBe(false);

    await chat.focus();
    await page.keyboard.press("Tab");
    await expect(row.getByRole("button", { name: /^Unpin / })).toBeFocused();
    await expect.poll(() => painted(state)).toBe(false);
    await expect.poll(() => painted(quick)).toBe(true);

    await page.keyboard.press("Tab");
    await expect(actions).toBeFocused();
    await expect.poll(() => painted(state)).toBe(false);
    await expect.poll(() => painted(actions)).toBe(true);
  });
});

for (const [only, role, other] of [
  ["deletions", "--color-diff-delete", "--color-diff-add"],
  ["additions", "--color-diff-add", "--color-diff-delete"],
] as const) {
  // A lone total is both first and last child; colouring once followed that
  // position rather than whether the count is an addition or a deletion.
  test(`colours a lone ${only} total by its meaning`, async ({ page }) => {
    await openScenario(page, "diff", "agent");
    await settleChatProjections(page);
    await page.evaluate(async (only) => {
      const [fixture, { acpProjectionStoreFor }] = await Promise.all([
        import("/tests/ui/fixtures/cp30cEditFixture.ts"),
        import("/src/app/acpProjectionRegistry.ts"),
      ]);
      // The partial fixture knows deletions only; flip it for additions.
      fixture.installCp30cDisclosure("partial");
      if (only === "deletions") return;
      for (const node of document.querySelectorAll<HTMLElement>(
        ".acp-chat-projection[data-chat-id]",
      )) {
        const store = acpProjectionStoreFor(node.dataset.chatId as never);
        const page = structuredClone(store.getState().page!);
        for (const item of page.turns.flatMap((turn) => turn.items))
          if (item.kind === "fileChangeDiff")
            item.payload.changes = item.payload.changes.map((change) => ({
              ...change,
              additions: change.deletions,
              deletions: null,
            }));
        store.setState({ page });
      }
    }, only);
    const totals = page
      .getByRole("region", { name: "Files edited in this chat" })
      .locator(".acp-file-changes__header .acp-file-changes__totals");
    await expect(totals.locator("span")).toHaveCount(1);
    const total = totals.locator(`.acp-file-changes__${only}`);
    await expect(total).toHaveText(only === "additions" ? /^\+\d+$/ : /^-\d+$/);
    const [expected, unexpected] = await Promise.all([
      roleColour(totals, "color", role),
      roleColour(totals, "color", other),
    ]);
    expect(expected).not.toBe(unexpected);
    await expect(total).toHaveCSS("color", expected);
  });
}

// Replaces the rich answer with one plain code block and returns it.
async function openCodeBlock(page: Page) {
  await openScenario(page, "rich", "agent");
  await settleChatProjections(page);
  await page.evaluate(async () => {
    const [{ buildVisualScenario }, { acpProjectionStoreFor }, ids] =
      await Promise.all([
        import("/tests/ui/fixtures/visualScenarioShell.ts"),
        import("/src/app/acpProjectionRegistry.ts"),
        import("/tests/ui/fixtures/visualFixtureIds.ts"),
      ]);
    const chatPage = buildVisualScenario("rich").pages[0];
    const turn = chatPage.turns[0];
    const answer = turn.items.find(
      (item) => item.kind === "text" && item.payload.role === "assistant",
    );
    if (answer?.kind !== "text") throw new Error("Expected an answer");
    answer.payload.text = "```\nconst wrapped = true;\n```";
    turn.items = [turn.items[0], answer];
    acpProjectionStoreFor(chatPage.chatId).getState().applyAuthoritativePage(
      {
        projectId: ids.PROJECT_ID,
        projectGeneration: ids.GENERATION,
        chatId: chatPage.chatId,
      },
      chatPage,
    );
  });
  await settleTranscriptAtEnd(page);
  return page.getByRole("region", { name: "Code block" });
}

test("keeps a pressed code action on the active fill, even while hovered", async ({
  page,
}) => {
  const code = await openCodeBlock(page);
  const wrap = code.getByRole("button", { name: "Wrap code" });
  const fill = () =>
    wrap.evaluate((node) => getComputedStyle(node).backgroundColor);
  const [panel, hover, active] = await Promise.all([
    code.evaluate((node) => getComputedStyle(node).backgroundColor),
    roleColour(code, "background-color", "--color-surface"),
    roleColour(code, "background-color", "--color-surface-active"),
  ]);
  expect(new Set([panel, hover, active]).size).toBe(3);

  await wrap.hover();
  await expect.poll(fill).toBe(hover);
  await page.mouse.move(0, 0);
  await wrap.focus();
  await page.keyboard.press("Enter");
  await expect(wrap).toHaveAttribute("aria-pressed", "true");
  await expect.poll(fill).toBe(active);
  await wrap.hover();
  await expect.poll(fill).toBe(active);
});

test("keeps the masked wrap glyph visible in forced colours in every state", async ({
  page,
}) => {
  await page.emulateMedia({ forcedColors: "active" });
  const code = await openCodeBlock(page);
  const wrap = code.getByRole("button", { name: "Wrap code" });
  const icon = wrap.locator(".acp-code-block__wrap-icon");
  // The glyph is a mask painted with its own background, so forced colours
  // would erase it unless that paint follows the button's system foreground.
  const systemColour = (keyword: string) =>
    icon.evaluate((node, keyword) => {
      const probe = document.createElement("span");
      probe.style.forcedColorAdjust = "none";
      probe.style.color = keyword;
      node.append(probe);
      const colour = getComputedStyle(probe).color;
      probe.remove();
      return colour;
    }, keyword);
  const paint = () =>
    icon.evaluate((node) => {
      const box = node.getBoundingClientRect();
      return {
        glyph: getComputedStyle(node).backgroundColor,
        button: getComputedStyle(node.closest("button")!).backgroundColor,
        painted: box.width > 0 && box.height > 0,
      };
    });
  const expectGlyph = async (keyword: string) => {
    const expected = await systemColour(keyword);
    await expect.poll(async () => (await paint()).glyph).toBe(expected);
    const { button, painted } = await paint();
    expect(painted).toBe(true);
    expect(button).not.toBe(expected);
  };

  await page.mouse.move(0, 0);
  await expectGlyph("ButtonText");
  await wrap.hover();
  await expectGlyph("HighlightText");
  await page.mouse.move(0, 0);
  await page.keyboard.press("Tab");
  await wrap.focus();
  await expectGlyph("HighlightText");
  await page.keyboard.press("Enter");
  await expect(wrap).toHaveAttribute("aria-pressed", "true");
  await wrap.blur();
  await expectGlyph("HighlightText");
});
