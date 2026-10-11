// Agent Panel V1 conformance.
//
// This spec is NOT a regression test. The rest of `tests/ui/*.visual.ts`
// compares the UI against screenshots the project saved earlier, which answers
// "did this change since last time?" and never "does this match the target?".
// This file answers the second question, against
// `docs/design/agent-panel-references/agent-panel-v1-target.png` as read by
// `docs/design/AGENT-PANEL-V1-REFERENCE.md`.
//
// It deliberately takes no screenshot and makes no pixel comparison against the
// mockup: Sage's palette is its own, so a pixel compare would fail forever and
// teach everyone to ignore this file.
//
// Two halves, and both must be able to fail:
//
//   1. Elements the target requires that Sage renders today are asserted by
//      role, accessible name, DOM order and geometry. Some assertions
//      deliberately pin today's *divergent* placement — notably the
//      titlebar-hosted mode segment — so that closing a gap forces this file
//      and `docs/design/agent-panel-conformance.md` to be updated together.
//   2. Elements the target requires that Sage does NOT render yet are listed
//      once, in `PENDING_CONFORMANCE_ITEMS`, each with a probe asserting the
//      element is still absent. When a lane builds one, its probe fails and the
//      entry must move into part 1. A pending entry is therefore a
//      currently-false assertion, never a silent pass.
//
// The pending list is the ordered work queue for the lanes that follow, and it
// shrinks to empty as they land. Its order mirrors the queue table in
// `docs/design/agent-panel-conformance.md`.
//
// An element that shipped and was then *lost* is a third case: it is written in
// the target's direction and marked `test.fail()`, so it reports as a known
// failure and flips to "expected to fail but passed" the moment it is restored.
// There are none open right now — `M12` was the first and it is fixed — but the
// pattern is the one to reach for, not the pending queue, which means "never
// built".
//

import { expect, test, type Locator, type Page } from "@playwright/test";
import { readFile } from "node:fs/promises";

import { openAgentSnapshotDiff } from "./resourceVisualHelpers";
import {
  expectLiveThinkingFixtureLine,
  openComposerOptions,
  revealWorkedContent,
} from "./agentVisualHelpers";
import {
  installScenario,
  openScenario,
  settleChatProjections,
  settleTranscriptAtEnd,
} from "./shared/visualBrowserPrimitives";

const AGENT_VIEWPORT = { width: 1380, height: 880 };

function rail(page: Page): Locator {
  return page.getByRole("complementary", { name: "Projects and chats" });
}

function artifactRegion(page: Page): Locator {
  return page.getByRole("complementary", { name: "Agent artifacts" });
}

function artifactTabStrip(page: Page): Locator {
  return page.getByRole("navigation", { name: "Agent artifact tabs" });
}

function composer(page: Page): Locator {
  return page.locator("form.composer");
}

function toolRollup(page: Page): Locator {
  return page.locator(".acp-tool-rollup");
}

/**
 * The composer's interactive **controls** in visual reading order — top row left
 * to right, then the next row. The row is single-line at 1380px as of
 * `490e4e0`, but still wraps at narrow widths, so a flat left-to-right
 * comparison would read a wrapped control as "earlier" than one above it.
 *
 */
async function composerControlOrder(page: Page): Promise<string[]> {
  return composer(page).evaluate((form) =>
    [
      ...form.querySelectorAll<HTMLElement>(
        '.composer__toolbar button, .composer__toolbar [role="button"]',
      ),
    ]
      .map((node) => {
        const bounds = node.getBoundingClientRect();
        return {
          name:
            node.getAttribute("aria-label") ?? node.textContent?.trim() ?? "",
          // Bucket by centre line, not top edge: send is deliberately the
          // taller emphasized square, so equal-height rows would be the
          // anomaly and top edges differ by design.
          row: Math.round((bounds.top + bounds.height / 2) / 8),
          x: bounds.left,
        };
      })
      .sort((first, second) => first.row - second.row || first.x - second.x)
      .map((control) => control.name),
  );
}

/**
 * Wait until every container a probe is scoped to has actually mounted.
 *
 * `openScenario` resolves before the chat shell finishes rendering — measured:
 * `form.composer` can still count 0 one call before `form.getByRole("button")`
 * counts four. Any `toHaveCount(0)` scoped inside a container that has not
 * mounted passes instantly and proves nothing, which is precisely the silent
 * pass this file exists to prevent. Every absence assertion below is anchored
 * on a sibling that must be present first.
 */
async function anchorScenario(page: Page, scenario: PendingScenario) {
  await expect(page.locator("main.workbench")).toBeVisible();
  await expect(
    rail(page).getByRole("button", { name: "New task", exact: true }),
  ).toBeVisible();
  await expect(composer(page)).toHaveCount(1);
  await expect(
    page.getByRole("textbox", { name: "Message agent" }),
  ).toBeVisible();
  await expect(
    composer(page).getByRole("button", { name: "Send message" }),
  ).toBeVisible();

  if (scenario === "agent-declared-test") {
    await revealWorkedContent(toolRollup(page).locator("summary"));
  }
  if (scenario === "agent-single") {
    await expect(
      page.locator(".acp-message--user").first().locator(".acp-markdown"),
    ).toBeVisible();
    await expect(
      page.locator(".acp-message--assistant").first().locator(".acp-markdown"),
    ).toBeVisible();
  }
  if (scenario === "diff") {
    // Without the artifact open, a probe scoped to the tab strip would pass
    // because its container is missing rather than because the element is.
    await openAgentSnapshotDiff(page);
    await expect(artifactTabStrip(page).getByRole("tab").first()).toBeVisible();
  }
}

async function boundsOf(locator: Locator) {
  const box = await locator.boundingBox();
  expect(box, "element has no layout box").not.toBeNull();
  if (!box) throw new Error("element has no layout box");
  return box;
}

// ---------------------------------------------------------------------------
// Pending: target elements Sage does not render yet.
// ---------------------------------------------------------------------------

type PendingScenario = "agent-single" | "agent-declared-test" | "diff";

interface PendingConformanceItem {
  /** Row id in `docs/design/agent-panel-conformance.md`. */
  readonly id: string;
  readonly region: string;
  readonly element: string;
  readonly scenario: PendingScenario;
  /** Why it cannot simply be drawn, when something real is missing. */
  readonly blockedOn: string | null;
  /** Must match nothing today. Matching means the element shipped. */
  readonly probe: (page: Page) => Locator;
}

export const PENDING_CONFORMANCE_ITEMS: readonly PendingConformanceItem[] = [
  {
    id: "L1",
    region: "left rail",
    element: "`Agents | Editor` mode segment hosted inside the rail",
    scenario: "agent-single",
    blockedOn: null,
    probe: (page) =>
      rail(page)
        .getByRole("radio", { name: /^agents?$/i })
        .or(rail(page).getByRole("tab", { name: /^agents?$/i })),
  },
  {
    id: "M14",
    region: "composer",
    element: "globe / web toggle in the control row",
    scenario: "agent-single",
    blockedOn: "the browsable tab it opens",
    probe: (page) =>
      composer(page).getByRole("button", {
        name: /globe|web search|browse|internet/i,
      }),
  },
  {
    id: "C20",
    region: "centre conversation",
    element: "timestamp on an activity row",
    scenario: "agent-declared-test",
    blockedOn:
      "a wall-clock time on the item — the row reports elapsed duration only",
    probe: (page) => toolRollup(page).locator("summary time"),
  },
  {
    id: "C8",
    region: "centre conversation",
    element: "avatar on the user turn",
    scenario: "agent-single",
    blockedOn: null,
    probe: (page) => page.locator(".acp-message--user").locator("svg, img"),
  },
  {
    id: "C10",
    region: "centre conversation",
    element: "Sage mark on the assistant turn",
    scenario: "agent-single",
    blockedOn: null,
    probe: (page) =>
      page
        .locator(".acp-message--assistant")
        .locator("svg:not(.acp-code-block svg), img"),
  },
  {
    id: "R2",
    region: "right tab region",
    element: "`Changes` tab carrying an artifact count badge",
    scenario: "diff",
    blockedOn: "a changes-aggregate entity",
    probe: (page) =>
      artifactRegion(page).getByRole("tab", { name: /^changes/i }),
  },
  {
    id: "R5",
    region: "right tab region",
    element: "`Plan` tab in the right region",
    scenario: "diff",
    blockedOn: null,
    probe: (page) => artifactRegion(page).getByRole("tab", { name: /^plan$/i }),
  },
  {
    id: "R6",
    region: "right tab region",
    element: "`+` opening a new artifact tab",
    scenario: "diff",
    blockedOn: null,
    probe: (page) =>
      artifactTabStrip(page)
        .getByRole("button", { name: /^\+$/ })
        .or(
          artifactTabStrip(page).getByRole("button", {
            name: /^(new|add|open) (artifact|tab|resource)/i,
          }),
        ),
  },
];

function pendingFor(scenario: PendingScenario) {
  return PENDING_CONFORMANCE_ITEMS.filter((item) => item.scenario === scenario);
}

/**
 * Every pending item gets its own test, so its message is what fails — never a
 * neighbouring assertion that happened to trip first on the same page.
 */
function registerPendingTests(scenario: PendingScenario) {
  for (const item of pendingFor(scenario)) {
    test(`${item.id} — ${item.element} — is still absent`, async ({ page }) => {
      await openScenario(page, scenario, "agent");
      await anchorScenario(page, scenario);
      await expect(
        item.probe(page),
        `${item.id} (${item.region}) — ${item.element} — is on the pending ` +
          `queue in docs/design/agent-panel-conformance.md, but the UI now ` +
          `renders it. That is good news: move ${item.id} out of ` +
          `PENDING_CONFORMANCE_ITEMS into an asserted check and mark the ` +
          `inventory row present.` +
          (item.blockedOn ? ` Recorded blocker was: ${item.blockedOn}.` : ""),
      ).toHaveCount(0);
    });
  }
}

test.describe("Agent Panel V1 conformance — pending target elements", () => {
  test.use({ viewport: AGENT_VIEWPORT });

  test("keeps the pending queue and the inventory document in step", async () => {
    // Guards the list itself: duplicate or empty ids would let an item drop out
    // of the queue silently.
    const ids = PENDING_CONFORMANCE_ITEMS.map((item) => item.id);
    expect(new Set(ids).size, "pending ids are unique").toBe(ids.length);
    expect(ids.every((id) => /^[A-Z]\d+$/.test(id))).toBe(true);
    const inventory = await readFile(
      new URL("../../docs/design/agent-panel-conformance.md", import.meta.url),
      "utf8",
    );
    const queue = inventory.split(/^## Pending queue\s*$/m)[1];
    expect(queue, "inventory has a pending queue section").toBeDefined();
    const documentedIds = [
      ...queue!.matchAll(/^\|\s*\d+\s*\|\s*`([A-Z]\d+)`/gm),
    ].map((match) => match[1]);
    expect(ids, "pending probes mirror the document's ordered queue").toEqual(
      documentedIds,
    );
    const statuses = new Map(
      [
        ...inventory.matchAll(
          /^\|\s*([A-Z]\d+)\s*\|.*?\|\s*(present|different|absent)\s*\|/gm,
        ),
      ].map((match) => [match[1], match[2]]),
    );
    for (const id of ids)
      expect(statuses.get(id), `${id} is inventoried as absent`).toBe("absent");
    expect(statuses.get("M11")).toBe("present");
  });

  registerPendingTests("agent-single");
  registerPendingTests("agent-declared-test");
  registerPendingTests("diff");
});

// ---------------------------------------------------------------------------
// Shell, titlebar and left rail.
// ---------------------------------------------------------------------------

test.describe("Agent Panel V1 conformance — shell, titlebar, left rail", () => {
  test.use({ viewport: AGENT_VIEWPORT });

  test("renders the three target regions left to right", async ({ page }) => {
    await openScenario(page, "agent-single", "agent");

    const [railBox, centreBox, artifactBox] = await Promise.all([
      boundsOf(rail(page)),
      boundsOf(page.locator(".center-region")),
      boundsOf(artifactRegion(page)),
    ]);

    expect(
      railBox.x,
      "left rail must precede the centre conversation",
    ).toBeLessThan(centreBox.x);
    expect(
      centreBox.x,
      "centre conversation must precede the right tab region",
    ).toBeLessThan(artifactBox.x);
  });

  test("renders the titlebar brand, and pins the mode segment's current region", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    const titleBar = page.locator("header.title-bar");

    // T1 — present.
    await expect(titleBar.getByText("Sage", { exact: true })).toBeVisible();

    // T2 — different. The target and the reference's region map both put the
    // mode segment inside the left rail; Sage hosts it in the titlebar. Pinned
    // here so moving it fails loudly rather than silently passing.
    const modeSegment = titleBar.getByRole("radiogroup", {
      name: "Window mode",
    });
    await expect(modeSegment).toHaveCount(1);
    await expect(modeSegment.getByRole("radio")).toHaveCount(2);
    await expect(
      modeSegment.getByRole("radio", { name: "Agent" }),
    ).toBeChecked();
    await expect(
      modeSegment.getByRole("radio", { name: "Editor" }),
    ).toHaveCount(1);
    // Anchored on the rail having mounted, so the absence is the rail's and
    // not the renderer's.
    await expect(
      rail(page).getByRole("button", { name: "New task", exact: true }),
    ).toBeVisible();
    await expect(
      rail(page).getByRole("radiogroup"),
      "T2: the mode segment still lives in the titlebar, not the rail",
    ).toHaveCount(0);

    // T3 — different. The target draws a search *field* with a chord hint;
    // Sage ships an icon-only button and no field.
    await expect(
      titleBar.getByRole("button", { name: "Search chats" }),
    ).toHaveCount(1);
    await expect(
      titleBar.getByRole("searchbox").or(titleBar.getByRole("textbox")),
      "T3: the titlebar still has no search field",
    ).toHaveCount(0);
  });

  test("renders the rail's task-rail structure in target order", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    const sidebar = rail(page);

    // L2 — the button exists with the right name. Its plate is the gap, and a
    // plate is a style change this structural spec does not adjudicate.
    await expect(
      sidebar.getByRole("button", { name: "New task", exact: true }),
    ).toBeVisible();

    const filter = sidebar.getByRole("button", {
      name: "Filter tasks",
      exact: true,
    });
    const search = sidebar.getByRole("searchbox", { name: "Search tasks" });
    await expect(filter).toBeVisible();
    await expect(filter).toHaveAttribute("aria-expanded", "false");
    await expect(search).toHaveCount(0);
    await filter.click();
    await expect(filter).toHaveAttribute("aria-expanded", "true");
    await expect(search).toBeVisible();
    await expect(search).toBeFocused();
    await expect(
      sidebar.getByRole("button", { name: "Search all chats" }),
    ).toBeVisible();
    await search.press("Escape");
    await expect(filter).toHaveAttribute("aria-expanded", "false");
    await expect(search).toHaveCount(0);
    await expect(filter).toBeFocused();

    // L5, L6, L13 — present, and in the target's top-to-bottom order.
    const sections = sidebar.getByRole("region");
    await expect(sections).toHaveCount(3);
    await expect(sections.nth(0)).toHaveAccessibleName("Projects");
    await expect(sections.nth(1)).toHaveAccessibleName("Pinned");
    await expect(sections.nth(2)).toHaveAccessibleName("Recent");
    for (let index = 0; index < 3; index += 1) {
      await expect(
        sections.nth(index).getByRole("button", { expanded: true }).first(),
        "each rail section is collapsible with a disclosure control",
      ).toBeVisible();
    }

    // L7, L10 — the project tree's active row and its branch chip. The rows
    // carry no role beyond `button`, so `aria-current` is the only semantic
    // handle on "this is the active project".
    const projects = sections.nth(0);
    const activeProject = projects.locator('button[aria-current="page"]');
    await expect(activeProject).toHaveCount(1);
    await expect(activeProject.locator(".project-row__branch")).toHaveCount(1);

    // L11, L12 — worktree and PR chips are decided omissions; the branch chip
    // is the one repository fact the projection actually supplies.
    await expect(
      projects.getByText(/worktree/i),
      "L11: no worktree chip may be invented",
    ).toHaveCount(0);

    // L14 — the footer identity line, below every section.
    const footer = sidebar.locator(".project-sidebar__account");
    await expect(footer).toBeVisible();
    const [recentBox, footerBox] = await Promise.all([
      boundsOf(sections.nth(2)),
      boundsOf(footer),
    ]);
    expect(
      footerBox.y,
      "the identity footer sits below Recent",
    ).toBeGreaterThan(recentBox.y);
  });

  test("opens Settings from the rail footer gear", async ({ page }) => {
    await openScenario(page, "agent-single", "agent");

    const gear = rail(page)
      .locator(".project-sidebar__account")
      .getByRole("button", { name: /settings|preferences/i });
    await expect(gear).toHaveCount(1);
    await gear.click();

    await expect(
      page.getByRole("radio", { name: "Editor" }),
      "L15: the rail gear leaves Agent mode so Settings is actually reachable",
    ).toBeChecked();
  });

  test("creates a task from the chord the rail advertises", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");

    const newTask = rail(page).getByRole("button", {
      name: "New task",
      exact: true,
    });
    await expect(
      newTask,
      "L3: the rail advertises the chord the keymap actually binds",
    ).toContainText(/Ctrl\+N|⌘N/);
  });

  test("keeps a pinned chat's title legible without redundant metadata", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    const pinned = rail(page).getByRole("region", { name: "Pinned" });
    const row = pinned.locator(".project-chat-row").first();
    const label = row.locator(".project-chat__title-label");
    const context = row.locator(".project-chat__context");

    await expect(label).toBeVisible();
    await expect(context).toHaveCount(1);
    await expect(context).toBeHidden();

    expect(
      await label.evaluate((node) => node.scrollWidth - node.clientWidth),
      "the pinned title is shown whole rather than clipped by its column",
    ).toBeLessThanOrEqual(0);

    const [copyBox, titleBox, labelBox] = await Promise.all([
      boundsOf(row.locator(".project-chat__copy")),
      boundsOf(row.locator(".project-chat__title")),
      boundsOf(label),
    ]);
    expect(
      titleBox.width,
      "the title claims the whole copy column rather than sharing it",
    ).toBeGreaterThan(copyBox.width - 1);
    expect(
      labelBox.x + labelBox.width,
      "the whole title stays inside its copy column",
    ).toBeLessThanOrEqual(copyBox.x + copyBox.width + 1);
  });

  test("renders Recent entries carrying a trailing state", async ({ page }) => {
    // L13, partly. The target's Recent list is relative-timestamped, and
    // `ProjectChatRailGlyph` does append ` ago` — but only for a chat whose
    // trailing state resolves to `time`, which needs a terminal status, no
    // unread badge and an `updatedAt`. Live status outranks the stamp, and no
    // visual fixture reaches that combination, so the ` ago` suffix itself is
    // unit-covered only (`src/workbench/agentRailStatusView.test.ts`). What is
    // reachable here — that Recent lists real entries, each with a trailing
    // state — is asserted; the stamp is recorded as a fixture gap in
    // docs/design/agent-panel-conformance.md rather than faked.
    await openScenario(page, "agent-dual", "agent");
    const recent = rail(page).getByRole("region", { name: "Recent" });

    const entries = recent.locator(".project-chat-row");
    await expect(entries).toHaveCount(1);
    await expect(entries.first().locator(".project-chat__state")).toHaveCount(
      1,
    );
  });
});

// ---------------------------------------------------------------------------
// Centre conversation.
// ---------------------------------------------------------------------------

test.describe("Agent Panel V1 conformance — centre conversation", () => {
  test.use({ viewport: AGENT_VIEWPORT });

  test("renders the task header with rail-owned chat navigation", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    const header = page.locator(".agent-task-header");

    // C1 — different. Sage draws `Project / Task`; the target draws three
    // segments. Pinned at two so adding a truthful third fails here first.
    await expect(header.locator('[aria-label="Chat location"]')).toHaveCount(1);
    await expect(header.locator(".agent-task-header__separator")).toHaveCount(
      1,
    );
    await expect(header.locator(".agent-task-header__title")).toBeVisible();

    // C2 — different. The status is announced, but renders as bare coloured
    // text rather than the target's pill with a leading icon.
    const status = header.getByRole("status");
    await expect(status).toHaveCount(1);
    await expect(status).toHaveAccessibleName(/^Chat status: /);
    await expect(
      status.locator("svg"),
      "C2: the status still has no leading icon",
    ).toHaveCount(0);

    // C3 — `Share` is a decided omission and must stay absent.
    await expect(
      header.getByRole("button", { name: /share/i }),
      "C3: Share is deliberately out of V1 and must not appear",
    ).toHaveCount(0);

    // C4.
    await expect(
      header.getByRole("button", { name: /^Chat actions for / }),
    ).toHaveCount(1);

    await expect(
      header.getByRole("button", { name: "Hand off chat" }),
      "Handoff is an action-menu item, not a breadcrumb-row button",
    ).toHaveCount(0);
    const actions = header.getByRole("button", { name: /^Chat actions for / });
    await actions.click();
    await expect(
      page.getByRole("menuitem", { name: "Hand off chat", exact: true }),
    ).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(actions).toBeFocused();
    await expect(page.getByRole("tablist", { name: "Chat tabs" })).toHaveCount(
      0,
    );
    await expect(
      rail(page).getByRole("button", {
        name: /^Open chat Agent navigation polish/,
      }),
    ).toBeVisible();
    await expect(
      rail(page).getByRole("button", { name: "New task", exact: true }),
    ).toBeVisible();
  });

  test("keeps message bodies visible without redundant identity chrome", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    await settleTranscriptAtEnd(page);

    // C9, C12 — timestamps present on both roles.
    // C11 — different: the assistant turn is named `Agent`, not `Sage`.
    const user = page.locator(".acp-message--user").first();
    const assistant = page.locator(".acp-message--assistant").first();
    await expect(user).toBeVisible();
    await expect(assistant).toBeVisible();
    await expect(user.locator(".acp-markdown")).toBeVisible();
    await expect(user.locator(".acp-markdown")).toHaveText(
      "Review the Agent and Editor surfaces and make the experience feel coherent.",
    );
    await expect(assistant.locator(".acp-markdown")).toBeVisible();
    await expect(assistant.locator(".acp-markdown")).toContainText(
      "The core hierarchy is now consistent.",
    );
    await expect(user.locator(".acp-message__role")).toHaveText("You");
    await expect(user.locator(".acp-message__meta")).toBeHidden();
    await expect(user.locator(".acp-message__meta time")).toHaveCount(1);
    await expect(assistant.locator(".acp-message__role")).toHaveText("Agent");
    await expect(assistant.locator(".acp-message__meta")).toBeHidden();
    await expect(assistant.locator(".acp-message__meta time")).toHaveCount(1);
  });

  test("runs turn content to the same measure as the composer", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    await settleTranscriptAtEnd(page);

    const items = page.locator(".acp-turn__items").first();
    const composer = page.locator(".composer").first();
    const itemsBox = await items.boundingBox();
    const composerBox = await composer.boundingBox();
    expect(itemsBox, "the first turn exposes an item column").not.toBeNull();
    expect(composerBox, "the composer is laid out").not.toBeNull();

    const itemsRight = itemsBox!.x + itemsBox!.width;
    const composerRight = composerBox!.x + composerBox!.width;
    expect(
      composerRight - itemsRight,
      "turn content reaches the composer's trailing edge",
    ).toBeLessThanOrEqual(24);
  });

  test("keeps the user turn a right-aligned bubble narrower than its column", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    await settleTranscriptAtEnd(page);

    const user = page.locator(".acp-message--user").first();
    const column = page.locator(".acp-turn__items").first();
    const [userBox, columnBox] = await Promise.all([
      boundsOf(user),
      boundsOf(column),
    ]);

    expect(
      userBox.width,
      "C6: the user turn is a bubble, not a full-width block",
    ).toBeLessThan(columnBox.width - 1);
    expect(
      columnBox.x + columnBox.width - (userBox.x + userBox.width),
      "C6: the bubble hangs off the trailing edge of the column",
    ).toBeLessThanOrEqual(1);
    expect(
      userBox.x,
      "C6: the bubble leaves leading space rather than starting at the margin",
    ).toBeGreaterThan(columnBox.x + 1);
  });

  test("leaves the design gap between the last turn and the composer", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    await settleTranscriptAtEnd(page);

    const transcript = page.locator(".message-list").first();
    const distance = await transcript.evaluate(
      (node) => node.scrollHeight - node.scrollTop - node.clientHeight,
    );
    expect(
      distance,
      "the transcript is anchored at its end",
    ).toBeLessThanOrEqual(1);

    const [contentBox, transcriptBox, composerBox] = await Promise.all([
      boundsOf(transcript.locator(".message-list__content")),
      boundsOf(transcript),
      boundsOf(composer(page)),
    ]);
    expect(
      transcriptBox.y +
        transcriptBox.height -
        (contentBox.y + contentBox.height),
      "the last line clears the transcript floor by the design spacing",
    ).toBeGreaterThanOrEqual(23);
    expect(
      composerBox.y,
      "the composer sits below the transcript rather than over it",
    ).toBeGreaterThanOrEqual(transcriptBox.y + transcriptBox.height - 1);
  });

  test("renders the plan card with per-step state and a collapse control", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 1200 });
    await openScenario(page, "agent-activity-family", "agent");

    // C14, C15, C16.
    const plan = page.getByRole("region", { name: "Agent plan" });
    await expect(plan).toBeVisible();
    await expect(plan.getByText("Plan", { exact: true })).toBeVisible();

    const collapse = plan.getByRole("button", { expanded: true });
    await expect(collapse).toHaveCount(1);
    await expect(collapse).toContainText("Hide");

    const steps = plan.locator("ol > li");
    await expect(steps.first()).toBeVisible();
    const states = await steps.evaluateAll((items) =>
      items.map((item) => item.getAttribute("data-plan-state")),
    );
    expect(
      new Set(states).size,
      "the plan card renders more than one step state",
    ).toBeGreaterThan(1);
    for (const state of states) {
      expect(
        ["pending", "inProgress", "completed", "failed"],
        "every plan step carries a known state",
      ).toContain(state);
    }

    await collapse.click();
    await expect(plan.locator("ol")).toHaveCount(0);
  });

  test("renders activity rollups, and pins today's row anatomy", async ({
    page,
  }) => {
    await openScenario(page, "agent-declared-test", "agent");
    await settleTranscriptAtEnd(page);
    const worked = page.locator("details.chat-transcript-work");
    const workedSummary = worked.locator(":scope > summary");
    await expect(workedSummary).toBeVisible();
    await expect(workedSummary).toContainText(/^Worked/);
    await expect(worked).not.toHaveAttribute("open", "");
    await expect(toolRollup(page).locator("summary")).toBeHidden();
    await revealWorkedContent(toolRollup(page).locator("summary"));

    // C24 — `Ran tests` ships off a declared `scripts.test`.
    const rollup = toolRollup(page);
    await expect(rollup).toHaveCount(1);
    const summary = rollup.locator("summary");
    await expect(summary).toBeVisible();
    await expect(summary).toContainText("Ran tests");

    // C25 — the pass count is a decided omission: the projection carries a
    // tool lifecycle state, never an exit code or a parsed reporter.
    await expect(
      summary,
      "C25: `N passed` must not be claimed from a lifecycle state",
    ).not.toContainText(/passed/i);

    await expect(rollup).toHaveClass(/acp-activity-card/);
    await expect(
      summary.locator(".acp-tool-rollup__summary-copy > svg").first(),
      "C18: the activity row leads with a state glyph",
    ).toBeVisible();
    await expect(summary.locator(".acp-tool-rollup__label")).toContainText(
      "Ran tests",
    );
    await expect(
      summary.locator(".acp-tool-rollup__duration"),
      "C21: the row reports how long the activity took",
    ).not.toBeEmpty();
    await expect(
      summary.locator(".acp-tool-rollup__chevron"),
      "C22: the row ends on a disclosure chevron",
    ).toHaveCount(1);
    await expect(summary.locator(".acp-state")).toHaveText("Completed");
    await expect(
      summary.locator("time"),
      "C20: elapsed duration is not a wall-clock timestamp",
    ).toHaveCount(0);

    // C19 — the row starts collapsed and actually opens. Collapsed state is
    // read from the `<details>` element's own `open`, never from a duplicate
    // `aria-expanded`: native `<details>` already exposes that state, and the
    // duplicate could drift out of step with it.
    //
    // This assertion only became possible with `b186868`, which rebuilt the row
    // as an uncontrolled `<details>` driven by its own `toggle` event. Before
    // that it reported `aria-expanded="true"` while staying visually closed —
    // see "Defects found while inventorying" in
    // docs/design/agent-panel-conformance.md.
    expect(
      await rollup.evaluate((node) => (node as HTMLDetailsElement).open),
      "C19: the activity row starts collapsed",
    ).toBe(false);
    await expect(rollup.locator(".acp-tool-rollup__items")).toHaveCount(0);

    await summary.focus();
    await page.keyboard.press("Enter");
    expect(
      await rollup.evaluate((node) => (node as HTMLDetailsElement).open),
      "C19: activating the summary actually opens the row",
    ).toBe(true);
    await expect(rollup.locator(".acp-tool-rollup__items")).toBeVisible();
  });

  test("shows a compact live activity row with an accessible running indicator", async ({
    page,
  }) => {
    await openScenario(page, "agent-activity-family", "agent");

    const liveRow = page.locator(
      '.acp-tool-rollup[data-activity-state="inProgress"]',
    );
    await expect(liveRow).toHaveCount(1);
    await expect(liveRow.locator(".acp-tool-rollup__label")).toHaveClass(
      /acp-live-shimmer/,
    );
    await expect(liveRow.locator("summary .acp-state")).toHaveCount(0);
    const running = liveRow.getByRole("status", { name: "Running" });
    await expect(running).toBeVisible();
    const spinner = running.locator("svg");
    expect(
      await spinner.evaluate((node) => getComputedStyle(node).animationName),
    ).toBe("acp-tool-spin");
    await expect(
      liveRow.locator("summary .acp-tool-rollup__chevron"),
    ).toBeVisible();
    const label = liveRow.locator(".acp-tool-rollup__label");
    expect(
      await label.evaluate((node) => getComputedStyle(node).backgroundImage),
    ).toContain("linear-gradient");
    expect(await label.evaluate((node) => getComputedStyle(node).color)).toBe(
      "rgba(0, 0, 0, 0)",
    );
    expect(
      await label.evaluate((node) => getComputedStyle(node).animationName),
    ).toBe("acp-live-shimmer");

    await page.emulateMedia({ reducedMotion: "reduce" });
    expect(
      await label.evaluate((node) => getComputedStyle(node).animationDuration),
    ).toBe("0s");
    expect(
      await spinner.evaluate((node) => getComputedStyle(node).animationName),
    ).toBe("none");
    expect(
      await label.evaluate((node) => getComputedStyle(node).backgroundImage),
    ).toBe("none");
    expect(
      await label.evaluate((node) => getComputedStyle(node).color),
    ).not.toBe("rgba(0, 0, 0, 0)");
    await page.emulateMedia({ forcedColors: "active" });
    expect(
      await label.evaluate((node) => getComputedStyle(node).color),
    ).not.toBe("rgba(0, 0, 0, 0)");

    await expect(page.locator('[data-activity-kind="thought"]')).toHaveCount(0);
    await page.emulateMedia({
      forcedColors: "none",
      reducedMotion: "no-preference",
    });
    await installScenario(page, "agent-thinking-live");
    await expectLiveThinkingFixtureLine(page);
  });
});

// ---------------------------------------------------------------------------
// Composer.
// ---------------------------------------------------------------------------

test.describe("Agent Panel V1 conformance — composer", () => {
  test.use({ viewport: AGENT_VIEWPORT });

  test("renders M11 Thinking level when the bound session advertises effort", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    await page.evaluate(async () => {
      const fixtures =
        await import("/tests/ui/fixtures/surfaces/composerEffortVisualFixture.ts");
      fixtures.installVisualComposerEffortFixture();
    });
    const thinking = composer(page).getByRole("button", {
      name: "Thinking level",
      exact: true,
    });
    await expect(thinking).toBeVisible();
    await expect(thinking).toBeEnabled();
    await expect(thinking).toHaveText("Medium");
    await thinking.click();
    const input = page.getByRole("combobox", {
      name: "Thinking level",
      exact: true,
    });
    const choices = page.getByRole("listbox", {
      name: "Thinking level",
      exact: true,
    });
    await expect(input).toBeFocused();
    await expect(choices.getByRole("option")).toHaveText([
      "Low",
      "Medium",
      "High",
    ]);
    await expect(
      choices.getByRole("option", { name: "Medium", exact: true }),
    ).toHaveAttribute("aria-selected", "true");
    await page.keyboard.press("Escape");
    await expect(thinking).toBeFocused();
  });

  test("frames the composer and pins its placeholder", async ({ page }) => {
    await openScenario(page, "agent-single", "agent");
    await expect(composer(page)).toHaveCount(1);
    const input = page.getByRole("textbox", { name: "Message agent" });
    await expect(input).toHaveAttribute("placeholder", "Do anything");
    await expect(input).toHaveAccessibleDescription(/Ctrl\+L|⌘L/);
  });

  test("offers options, supported image input and dictation below the prompt", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    const form = composer(page);
    const toolbar = form.locator(".composer__toolbar");
    await expect(toolbar).toBeVisible();
    const options = toolbar.getByRole("button", {
      name: "Composer options",
      exact: true,
    });
    await expect(options).toBeEnabled();
    await expect(options).toHaveAttribute("aria-haspopup", "dialog");
    await expect(options).toHaveAttribute("aria-expanded", "false");
    const images = toolbar.getByRole("button", {
      name: "Add images",
      exact: true,
    });
    await expect(images).toBeDisabled();
    await expect(images).toHaveAttribute(
      "title",
      "Image input is unavailable for the selected binding.",
    );
    const chooseImages = form.getByLabel("Choose images", { exact: true });
    await expect(chooseImages).toBeDisabled();
    await expect(chooseImages).toHaveAttribute("type", "file");
    await expect(chooseImages).toHaveAttribute(
      "accept",
      "image/png,image/jpeg",
    );
    await expect(chooseImages).toHaveAttribute("multiple", "");
    await expect(chooseImages).toHaveAttribute("tabindex", "-1");
    const dictation = toolbar.getByRole("button", {
      name: "Start dictation",
      exact: true,
    });
    await expect(dictation).toBeEnabled();
    await expect(dictation).toHaveAttribute("aria-pressed", "false");
    await expect(dictation).toHaveAttribute(
      "title",
      "Speech may be processed by an online service",
    );
    await expect(toolbar.getByRole("button")).toHaveCount(8);
    const [inputBox, toolbarBox] = await Promise.all([
      boundsOf(form.getByRole("textbox", { name: "Message agent" })),
      boundsOf(toolbar),
    ]);
    expect(toolbarBox.y).toBeGreaterThanOrEqual(
      inputBox.y + inputBox.height - 1,
    );
  });

  test("pins the closed composer's interactive inventory", async ({ page }) => {
    await openScenario(page, "agent-single", "agent");
    await expect(
      composer(page).getByRole("button", { name: "Send message" }),
    ).toBeVisible();
    const inventory = await composer(page).evaluate((form) =>
      [
        ...form.querySelectorAll<HTMLElement>(
          'button, textarea, input, select, [role="button"], [role="combobox"]',
        ),
      ].map(
        (node) =>
          node.getAttribute("aria-label") ?? node.textContent?.trim() ?? "",
      ),
    );
    expect(inventory).toEqual([
      "Message agent",
      "Composer options",
      "Choose images",
      "Add images",
      "Access options",
      "Harness",
      "Model",
      "Start dictation",
      "Send message",
    ]);
    await expect(composer(page).locator(".composer__access-label")).toHaveText(
      "Read-only",
    );
    const options = composer(page).getByRole("button", {
      name: "Composer options",
      exact: true,
    });
    await expect(options).toHaveAttribute("aria-controls", /\S+/);
    const optionsId = await options.getAttribute("aria-controls");
    await expect(page.locator(`[id="${optionsId}"]`)).toHaveCount(0);
  });

  test("orders toolbar actions, harness and model with send trailing", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    await expect(
      composer(page).getByRole("button", { name: "Send message" }),
    ).toBeVisible();
    expect(await composerControlOrder(page)).toEqual([
      "Composer options",
      "Add images",
      "Access options",
      "Harness",
      "Model",
      "Start dictation",
      "Send message",
    ]);
    const [images, access, harness, send] = await Promise.all([
      boundsOf(
        composer(page).getByRole("button", { name: "Add images", exact: true }),
      ),
      boundsOf(composer(page).locator(".composer__access-label")),
      boundsOf(
        composer(page).getByRole("button", { name: "Harness", exact: true }),
      ),
      boundsOf(composer(page).getByRole("button", { name: "Send message" })),
    ]);
    expect(access.x).toBeGreaterThanOrEqual(images.x + images.width - 1);
    expect(access.x + access.width).toBeLessThanOrEqual(harness.x + 1);
    expect(Math.abs(send.width - send.height)).toBeLessThanOrEqual(1);
  });

  test("keeps the closed toolbar on one line", async ({ page }) => {
    await openScenario(page, "agent-single", "agent");
    await expect(
      composer(page).getByRole("button", { name: "Send message" }),
    ).toBeVisible();
    const centres = await composer(page)
      .locator(".composer__toolbar")
      .evaluate((row) =>
        [
          ...row.querySelectorAll<HTMLElement>(
            'button, [role="button"], .composer__access-label',
          ),
        ].map((node) => {
          const bounds = node.getBoundingClientRect();
          return bounds.top + bounds.height / 2;
        }),
      );
    expect(centres).toHaveLength(7);
    expect(Math.max(...centres) - Math.min(...centres)).toBeLessThanOrEqual(1);
  });

  test("names the active project in the options menu", async ({ page }) => {
    await openScenario(page, "agent-single", "agent");
    const options = composer(page).getByRole("button", {
      name: "Composer options",
      exact: true,
    });
    await expect(options).toBeVisible();
    await expect(
      composer(page).getByRole("button", { name: "Project", exact: true }),
    ).toHaveCount(0);
    const menu = await openComposerOptions(page);
    await expect(options).toHaveAttribute("aria-expanded", "true");
    const project = menu.getByRole("button", { name: "Project", exact: true });
    await expect(project).toBeVisible();
    await expect(project).toBeEnabled();
    await expect(project).toHaveText("Sage");
    await expect(project).toHaveAttribute("aria-haspopup", "listbox");
    const permission = menu.getByRole("button", {
      name: "Permission mode",
      exact: true,
    });
    await expect(permission).toBeVisible();
    await expect(permission).toHaveText("Read-only");
    await options.click();
    await expect(menu).toHaveCount(0);
    await expect(options).toHaveAttribute("aria-expanded", "false");
  });

  test("keeps reported context in the toolbar without overlapping the prompt", async ({
    page,
  }) => {
    await openScenario(page, "agent-composer-context-known", "agent");
    const context = composer(page).getByLabel("Composer context", {
      exact: true,
    });
    await expect(context).toBeVisible();
    await expect(context).toContainText("6K context");
    const meter = context.getByRole("progressbar", {
      name: "6,000 used / 272,000 total tokens",
      exact: true,
    });
    await expect(meter).toHaveCount(1);
    await expect(meter).toHaveAttribute("aria-valuemin", "0");
    await expect(meter).toHaveAttribute("aria-valuemax", "272000");
    await expect(meter).toHaveAttribute("aria-valuenow", "6000");
    await expect(meter).toHaveAttribute(
      "aria-valuetext",
      "6,000 / 272,000 tokens used",
    );
    expect(
      await context.evaluate((node) =>
        Boolean(node.closest(".composer__toolbar")),
      ),
    ).toBe(true);
    const [inputBox, contextBox, micBox, sendBox] = await Promise.all([
      boundsOf(composer(page).getByRole("textbox", { name: "Message agent" })),
      boundsOf(context),
      boundsOf(
        composer(page).getByRole("button", {
          name: "Start dictation",
          exact: true,
        }),
      ),
      boundsOf(composer(page).getByRole("button", { name: "Send message" })),
    ]);
    expect(contextBox.y).toBeGreaterThanOrEqual(
      inputBox.y + inputBox.height - 1,
    );
    expect(contextBox.x).toBeGreaterThanOrEqual(micBox.x + micBox.width - 1);
    expect(contextBox.x + contextBox.width).toBeLessThanOrEqual(sendBox.x + 1);
    expect(
      Math.abs(
        contextBox.y + contextBox.height / 2 - (sendBox.y + sendBox.height / 2),
      ),
    ).toBeLessThanOrEqual(1);
  });

  test("does not imply a context meter when capacity is unknown", async ({
    page,
  }) => {
    await openScenario(page, "agent-composer-context-unknown", "agent");
    await expect(
      composer(page).getByRole("textbox", { name: "Message agent" }),
    ).toBeVisible();
    await expect(
      composer(page).getByLabel("Composer context", { exact: true }),
    ).toHaveCount(0);
    await expect(composer(page).getByRole("progressbar")).toHaveCount(0);
    await expect(composer(page)).not.toContainText("6K context");
  });

  test("keeps session and permission modes in the options menu", async ({
    page,
  }) => {
    await openScenario(page, "agent-session-mode-closed", "agent");
    const options = composer(page).getByRole("button", {
      name: "Composer options",
      exact: true,
    });
    await expect(options).toBeVisible();
    await expect(
      composer(page).getByRole("button", { name: "Session mode", exact: true }),
    ).toHaveCount(0);
    const menu = await openComposerOptions(page);
    expect(
      await menu
        .getByRole("button")
        .evaluateAll((nodes) =>
          nodes.map((node) => node.getAttribute("aria-label")),
        ),
    ).toEqual(["Project", "Session mode", "Permission mode"]);
    await expect(
      menu.getByRole("button", { name: "Session mode", exact: true }),
    ).toHaveText("Review changes");
    await expect(
      menu.getByRole("button", { name: "Permission mode", exact: true }),
    ).toHaveText("Read-only");
  });
});

// ---------------------------------------------------------------------------
// Right tab region.
// ---------------------------------------------------------------------------

test.describe("Agent Panel V1 conformance — right tab region", () => {
  test.use({ viewport: AGENT_VIEWPORT });

  test("renders the artifact header, path row and unified diff", async ({
    page,
  }) => {
    await openScenario(page, "diff", "agent");
    await openAgentSnapshotDiff(page);
    const region = artifactRegion(page);

    // R1 — a real tab strip, but a per-artifact taxonomy rather than the
    // target's kind tabs. Pinned so a `Changes`/`Browser`/`Diagram`/`Plan` tab
    // fails here as well as in its pending probe.
    const tabs = artifactTabStrip(page).getByRole("tab");
    await expect(tabs.first()).toBeVisible();
    const tabNames = await tabs.evaluateAll((nodes) =>
      nodes.map((node) => node.getAttribute("aria-label") ?? ""),
    );
    for (const name of tabNames) {
      expect(
        name,
        "R1: right-pane tabs are still per-artifact File/Diff/Preview tabs",
      ).toMatch(/^(File |Diff |Preview$)/);
    }

    // R7 — different: the region collapses rather than closing.
    await expect(
      region.getByRole("button", {
        name: "Collapse files, changes, and preview",
      }),
    ).toHaveCount(1);

    // R8.
    await expect(
      artifactTabStrip(page).getByRole("button", { name: /^Close / }),
    ).toHaveCount(tabNames.filter((name) => name !== "Preview").length);

    const header = region.locator(".agent-artifact-header");
    // R11, R13, R14, R15, R16.
    await expect(header.getByText("Opened by Sage")).toBeVisible();
    await expect(header.getByText("VIEW ONLY")).toBeVisible();
    await expect(header).toContainText(/\d+ of \d+/);
    await expect(
      header.getByRole("button", { name: "Previous artifact" }),
    ).toHaveCount(1);
    await expect(
      header.getByRole("button", { name: "Next artifact" }),
    ).toHaveCount(1);
    await expect(
      header.getByRole("button", { name: /Open in Editor/ }),
    ).toHaveCount(1);

    // R17, R19.
    const pathRow = header.locator(".agent-artifact-header__path-row");
    await expect(pathRow.locator("code")).toContainText(
      "src/workbench/AgentChatPanes.tsx",
    );
    await expect(
      pathRow.getByRole("button", { name: /Copy path/ }),
    ).toHaveCount(1);

    // R20, R21, R22 — the unified diff with its gutter, hunk headers and
    // add/delete colouring.
    const diff = page.getByRole("table", { name: "Unified file diff" });
    await expect(diff).toBeVisible();
    await expect(diff.locator(".diff-hunk__header").first()).toBeVisible();
    await expect(diff.locator(".diff-line--added").first()).toBeVisible();
    await expect(diff.locator(".diff-line--removed").first()).toBeVisible();
  });

  test("turns a gutter comment into an attached context chip", async ({
    page,
  }) => {
    // R23, R24 and M2: the only write path out of the view-only region, and
    // the one that feeds the composer's attached-context chip row.
    await openScenario(page, "diff", "agent");
    await openAgentSnapshotDiff(page);

    const trigger = page.getByRole("button", { name: /^Comment on / }).first();
    await expect(trigger).toHaveCount(1);
    await trigger.click();

    const draft = page.getByRole("textbox", { name: /^Comment on / });
    await expect(draft).toBeVisible();
    await draft.fill("Conformance probe");
    await page.getByRole("button", { name: "Add to prompt" }).click();

    const attachments = composer(page).getByRole("list", {
      name: "Attached context",
    });
    await expect(attachments).toBeVisible();
    const chip = attachments.getByRole("listitem");
    await expect(chip).toHaveCount(1);
    await expect(
      chip.getByRole("button", { name: /^Remove .* context$/ }),
    ).toHaveCount(1);

    expect(
      await attachments.evaluate((node) =>
        Boolean(node.closest(".composer__attachment-row")),
      ),
    ).toBe(true);
    const [chipBox, inputBox, toolbarBox, formBox] = await Promise.all([
      boundsOf(attachments),
      boundsOf(page.getByRole("textbox", { name: "Message agent" })),
      boundsOf(composer(page).locator(".composer__toolbar")),
      boundsOf(composer(page)),
    ]);
    expect(
      chipBox.y,
      "M2: the attached-context chip row sits beneath the composer input",
    ).toBeGreaterThanOrEqual(inputBox.y + inputBox.height - 1);
    expect(chipBox.y + chipBox.height).toBeLessThanOrEqual(toolbarBox.y + 1);
    expect(chipBox.x).toBeGreaterThanOrEqual(formBox.x);
    expect(chipBox.x + chipBox.width).toBeLessThanOrEqual(
      formBox.x + formBox.width,
    );
    await chip.getByRole("button", { name: /^Remove .* context$/ }).click();
    await expect(attachments).toHaveCount(0);
    await expect(
      composer(page).locator(".composer__attachment-row"),
    ).toHaveCount(0);
  });
});
