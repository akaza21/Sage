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
// Reconciled 2026-09-21 against `490e4e0` (single-row control row, project
// scope restored), `c9365fc` (composer chrome) and `b186868` (rollup
// disclosure).

import { expect, test, type Locator, type Page } from "@playwright/test";

import { openAgentSnapshotDiff } from "./resourceVisualHelpers";
import {
  openScenario,
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
 * Only interactive controls appear here. The project scope chip is a plain,
 * non-interactive `<span>` by design, so it is positioned by geometry in its own
 * test rather than being listed as a control it is not.
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
    await expect(toolRollup(page).locator("summary")).toBeVisible();
  }
  if (scenario === "agent-single") {
    await expect(page.locator(".acp-message__meta").first()).toBeVisible();
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
    id: "M11",
    region: "composer",
    element: "thinking-level picker, fourth slot in the control row",
    scenario: "agent-single",
    blockedOn:
      "being re-scoped — threading a Codex-specific effort field through the generic prompt pipeline was the wrong shape",
    probe: (page) =>
      composer(page).getByRole("button", {
        name: /thinking|effort|reasoning/i,
      }),
  },
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
  // M4, M5 and M6 graduated on 2026-09-20 (`c9365fc`): the paperclip, `@` and
  // `/` affordances now render, disabled with truthful reasons. They are
  // asserted in "renders the affordance row beneath the input" below.
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
    probe: (page) =>
      page.locator(".acp-message--user .acp-message__meta").locator("svg, img"),
  },
  {
    id: "C10",
    region: "centre conversation",
    element: "Sage mark on the assistant turn",
    scenario: "agent-single",
    blockedOn: null,
    probe: (page) =>
      page
        .locator(".acp-message--assistant .acp-message__meta")
        .locator("svg, img"),
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

  test("keeps the pending queue and the inventory document in step", () => {
    // Guards the list itself: duplicate or empty ids would let an item drop out
    // of the queue silently.
    const ids = PENDING_CONFORMANCE_ITEMS.map((item) => item.id);
    expect(new Set(ids).size, "pending ids are unique").toBe(ids.length);
    expect(ids.every((id) => /^[A-Z]\d+$/.test(id))).toBe(true);
    expect(
      ids,
      "the pending queue's order mirrors the table in " +
        "docs/design/agent-panel-conformance.md",
    ).toEqual(["M11", "L1", "M14", "C20", "C8", "C10", "R2", "R5", "R6"]);
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

    // L4.
    await expect(
      sidebar.getByRole("searchbox", { name: "Search tasks" }),
    ).toBeVisible();

    // L5, L6, L13 — present, and in the target's top-to-bottom order.
    const sections = sidebar.getByRole("region");
    await expect(sections).toHaveCount(3);
    await expect(sections.nth(0)).toHaveAccessibleName("Pinned");
    await expect(sections.nth(1)).toHaveAccessibleName("Projects");
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
    const projects = sections.nth(1);
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

  test("gives a pinned chat's title its own row above the metadata", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    const pinned = rail(page).getByRole("region", { name: "Pinned" });
    const row = pinned.locator(".project-chat-row").first();
    const label = row.locator(".project-chat__title-label");
    const context = row.locator(".project-chat__context");

    await expect(label).toBeVisible();
    await expect(context).toHaveCount(1);

    expect(
      await label.evaluate((node) => node.scrollWidth - node.clientWidth),
      "the pinned title is shown whole rather than clipped by its column",
    ).toBeLessThanOrEqual(0);

    const [copyBox, titleBox, labelBox, contextBox] = await Promise.all([
      boundsOf(row.locator(".project-chat__copy")),
      boundsOf(row.locator(".project-chat__title")),
      boundsOf(label),
      boundsOf(context),
    ]);
    expect(
      titleBox.width,
      "the title claims the whole copy column rather than sharing it",
    ).toBeGreaterThan(copyBox.width - 1);
    expect(
      contextBox.y,
      "the rail metadata sits below the title, not beside it",
    ).toBeGreaterThanOrEqual(labelBox.y + labelBox.height - 1);
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

  test("renders the task header and chat tab strip", async ({ page }) => {
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

    // C5 — different. `Hand off` exists, but in the pane action row rather
    // than the task header the target places it in.
    await expect(
      header.getByRole("button", { name: "Hand off chat" }),
      "C5: Handoff is not in the breadcrumb row yet",
    ).toHaveCount(0);
    await expect(
      page.getByRole("button", { name: "Hand off chat", exact: true }),
    ).toHaveCount(1);

    // C6, C7 — named closable chat tabs plus a `+`.
    const tabs = page.getByRole("tablist", { name: "Chat tabs" });
    await expect(tabs.getByRole("tab")).toHaveCount(1);
    await expect(
      tabs.getByRole("button", { name: /^Archive chat / }),
    ).toHaveCount(1);
    await expect(
      page.getByRole("button", { name: "New agent chat" }),
    ).toHaveCount(1);
  });

  test("renders turn identity as the target's name-and-timestamp pair", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    await settleTranscriptAtEnd(page);

    // C9, C12 — timestamps present on both roles.
    // C11 — different: the assistant turn is named `Agent`, not `Sage`.
    const user = page.locator(".acp-message--user").first();
    const assistant = page.locator(".acp-message--assistant").first();
    await expect(user.locator(".acp-message__role")).toHaveText("You");
    await expect(user.locator(".acp-message__meta time")).toHaveCount(1);
    await expect(assistant.locator(".acp-message__role")).toHaveText("Agent");
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

    // C24 — `Ran tests` ships off a declared `scripts.test`.
    const rollup = toolRollup(page);
    await expect(rollup).toHaveCount(1);
    const summary = rollup.locator("summary");
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

  test("shows a compact live activity row without a status chip", async ({
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
    await page.emulateMedia({ forcedColors: "active" });
    expect(
      await label.evaluate((node) => getComputedStyle(node).color),
    ).not.toBe("rgba(0, 0, 0, 0)");
  });
});

// ---------------------------------------------------------------------------
// Composer.
// ---------------------------------------------------------------------------

test.describe("Agent Panel V1 conformance — composer", () => {
  test.use({ viewport: AGENT_VIEWPORT });

  test("frames the composer and pins its placeholder", async ({ page }) => {
    await openScenario(page, "agent-single", "agent");

    // M1 — a single framed region.
    await expect(composer(page)).toHaveCount(1);

    // M3 — present since `c9365fc`: the target's own wording. The focus chord
    // moved out of the placeholder into an `sr-only` description, so it is
    // still announced without crowding the visible prompt.
    await expect(
      page.getByRole("textbox", { name: "Message agent" }),
    ).toHaveAttribute(
      "placeholder",
      "Ask Sage to build, investigate, or explain…",
    );
    await expect(
      page.getByRole("textbox", { name: "Message agent" }),
    ).toHaveAccessibleDescription(/Ctrl\+L|⌘L/);
  });

  test("renders the affordance row beneath the input", async ({ page }) => {
    // M4, M5, M6 — present since `c9365fc`, and deliberately **disabled with a
    // truthful reason**: nothing backs attachments, mentions or slash commands
    // yet. Asserting presence alone would let them silently become fake working
    // controls, so each one's disabled state and its stated reason are pinned
    // together.
    await openScenario(page, "agent-single", "agent");
    const affordances = composer(page).getByLabel("Composer actions", {
      exact: true,
    });
    await expect(affordances).toBeVisible();

    const expected = [
      {
        id: "M4",
        name: "Attach files: File attachments are not available in this composer.",
      },
      {
        id: "M5",
        name: "Mention context: Mentions are not available in this composer.",
      },
      {
        id: "M6",
        name: "Insert slash command: Slash commands are not available in this composer.",
      },
    ];
    await expect(affordances.getByRole("button")).toHaveCount(expected.length);
    for (const [index, { id, name }] of expected.entries()) {
      const control = affordances.getByRole("button").nth(index);
      await expect(
        control,
        `${id}: accessible name and order`,
      ).toHaveAccessibleName(name);
      await expect(
        control,
        `${id}: must stay disabled until something backs it`,
      ).toBeDisabled();
    }

    // The row sits beneath the input, as the target draws it.
    const [inputBox, affordanceBox, toolbarBox] = await Promise.all([
      boundsOf(page.getByRole("textbox", { name: "Message agent" })),
      boundsOf(affordances),
      boundsOf(composer(page).locator(".composer__toolbar")),
    ]);
    expect(
      affordanceBox.y,
      "the affordance row sits below the input",
    ).toBeGreaterThanOrEqual(inputBox.y + inputBox.height - 1);
    expect(
      affordanceBox.y,
      "the affordance row sits above the control row",
    ).toBeLessThan(toolbarBox.y);
  });

  test("holds exactly the controls it holds today", async ({ page }) => {
    await openScenario(page, "agent-single", "agent");

    // The composer's full interactive inventory, in DOM order. This is the
    // backstop for every composer slot at once: a paperclip, an `@`, a `/`, an
    // effort picker or a globe toggle all fail here the moment they appear,
    // whatever they end up being called.
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
    expect(
      inventory,
      "the composer's control inventory changed — reconcile " +
        "docs/design/agent-panel-conformance.md and the pending queue",
    ).toEqual([
      "Message agent",
      "Attach files: File attachments are not available in this composer.",
      "Mention context: Mentions are not available in this composer.",
      "Insert slash command: Slash commands are not available in this composer.",
      "Harness",
      "Model",
      "Permission mode",
      "Send message",
    ]);
  });

  test("lays the control row out left to right, with send trailing", async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    const form = composer(page);

    // M10 — present. The settled order is Sage's own, not the target's:
    //
    //   harness → model → project scope → thinking level → mode → globe → send
    //
    // Only the slots that exist can be asserted. Thinking level (M11) and globe
    // (M14) are pending, so today's assertable order is
    // harness → model → [project scope] → send, with mode slotting in when a
    // session advertises one (covered separately). Project scope is a
    // non-interactive chip, so it is absent from `composerControlOrder` by
    // design and is positioned in its own test below.
    const order = await composerControlOrder(page);
    expect(
      order.indexOf("Harness"),
      "M10: the harness picker leads the settled control-row order",
    ).toBe(0);
    expect(
      order.indexOf("Model"),
      "M10: the model picker follows the harness picker",
    ).toBe(1);
    expect(order.at(-1), "the send button trails the control row").toBe(
      "Send message",
    );

    // M15 — the send button is already a filled accent square, so the target's
    // remaining difference is scale and glyph, neither of which is structural.
    const send = await boundsOf(
      form.getByRole("button", { name: "Send message" }),
    );
    expect(
      Math.abs(send.width - send.height),
      "M15: the send button is square",
    ).toBeLessThanOrEqual(1);
  });

  test("keeps the control row on a single line", async ({ page }) => {
    // The row wrapped once — harness and model on one line, mode and the policy
    // chip on the next — and the visual suite certified it, because the
    // baseline had been regenerated alongside the change. Measured by **centre
    // line**, never by top edge: send is deliberately the taller emphasized
    // square, so equal tops would be the anomaly rather than the goal.
    await openScenario(page, "agent-single", "agent");

    const spread = await composer(page).evaluate((form) => {
      const row = form.querySelector<HTMLElement>(".composer__toolbar");
      if (!row) return null;
      const centres = [
        ...row.querySelectorAll<HTMLElement>(
          'button, [role="button"], .project-scope, .access-mode',
        ),
      ]
        .filter((node) => node.getBoundingClientRect().height > 0)
        .map((node) => {
          const bounds = node.getBoundingClientRect();
          return bounds.top + bounds.height / 2;
        });
      if (centres.length === 0) return null;
      return {
        count: centres.length,
        spread: Math.max(...centres) - Math.min(...centres),
      };
    });

    expect(spread, "the control row must exist to be measured").not.toBeNull();
    if (!spread) return;
    expect(
      spread.count,
      "the control row holds harness, model, project scope and send",
    ).toBeGreaterThanOrEqual(4);
    expect(
      spread.spread,
      "the control row wrapped: its items no longer share a centre line",
    ).toBeLessThanOrEqual(1);
  });

  test("names the project the composer will act on", async ({ page }) => {
    // M12 — present again since `490e4e0`, after being deleted as dead code.
    //
    // Asserted in its **honest** shape, not the reference's. The target draws
    // `Sage ⌄`, but Sage's chip is deliberately non-interactive: a chevron on
    // something that cannot be activated is a false affordance, the same class
    // of defect as the fabricated `Ctrl+N` this codebase removed once already.
    // When a later lane makes it a real picker, the chevron returns with the
    // behaviour it advertises — and these assertions will fail and say so.
    await openScenario(page, "agent-single", "agent");
    const scope = composer(page).locator(".project-scope");

    await expect(scope).toHaveCount(1);
    await expect(scope, "M12: the chip names the active project").toHaveText(
      "Sage",
    );
    await expect(
      scope,
      "M12: the chip must state why it cannot be switched",
    ).toHaveAttribute("title", "Project switching is unavailable");

    // Non-interactive: no button, no widget role, not focusable, no chevron.
    await expect(
      scope.getByRole("button"),
      "M12: the scope chip must not offer an action nothing backs",
    ).toHaveCount(0);
    expect(
      await scope.evaluate((node) => ({
        tag: node.tagName.toLowerCase(),
        role: node.getAttribute("role"),
        tabIndex: node.getAttribute("tabindex"),
        // One decorative folder glyph, aria-hidden. A second glyph would be the
        // chevron coming back without the behaviour it promises.
        glyphs: node.querySelectorAll("svg").length,
        glyphsHidden: [...node.querySelectorAll("svg")].every(
          (glyph) => glyph.getAttribute("aria-hidden") === "true",
        ),
      })),
      "M12: the scope chip is a plain, unfocusable span with one hidden glyph",
    ).toEqual({
      tag: "span",
      role: null,
      tabIndex: null,
      glyphs: 1,
      glyphsHidden: true,
    });

    // Settled order: it sits between the model picker and everything after it.
    const [model, scopeBox, send] = await Promise.all([
      boundsOf(composer(page).getByRole("button", { name: "Model" })),
      boundsOf(scope),
      boundsOf(composer(page).getByRole("button", { name: "Send message" })),
    ]);
    expect(model.x, "M12: project scope follows the model picker").toBeLessThan(
      scopeBox.x,
    );
    expect(scopeBox.x, "M12: project scope precedes send").toBeLessThan(send.x);
  });

  test("renders the context meter inside the input's trailing band", async ({
    page,
  }) => {
    // M7, M8, M9 — present since `c9365fc`. The meter now reads `6K context`
    // and sits inside the input area's trailing band, above the affordance and
    // control rows, which is the target's shape.
    await openScenario(page, "agent-composer-context-known", "agent");
    const context = page.getByLabel("Composer context", { exact: true });

    await expect(context).toContainText("6K context");
    const meter = context.getByRole("progressbar", { name: "Reported usage" });
    await expect(meter).toHaveCount(1);

    // The compact label must not cost the real numbers: they stay in the
    // accessible value text, so assistive tech still gets the truth.
    await expect(meter).toHaveAttribute("aria-valuemin", "0");
    await expect(meter).toHaveAttribute("aria-valuemax", "272000");
    await expect(meter).toHaveAttribute("aria-valuenow", "6000");
    await expect(meter).toHaveAttribute(
      "aria-valuetext",
      "6,000 / 272,000 tokens used",
    );

    // M9 — structurally inside the input area, and above the control row.
    expect(
      await context.evaluate((node) =>
        Boolean(node.closest(".composer__input-area")),
      ),
      "M9: the meter lives inside the input area, not in a row of its own",
    ).toBe(true);
    const [toolbarBox, contextBox] = await Promise.all([
      boundsOf(composer(page).locator(".composer__toolbar")),
      boundsOf(context),
    ]);
    expect(
      contextBox.y + contextBox.height,
      "M9: the meter sits above the control row",
    ).toBeLessThanOrEqual(toolbarBox.y + 1);

    const [inputAreaBox, textareaBox, meterBox] = await Promise.all([
      boundsOf(composer(page).locator(".composer__input-area")),
      boundsOf(composer(page).getByRole("textbox", { name: "Message agent" })),
      boundsOf(meter),
    ]);
    expect(
      textareaBox.y + textareaBox.height,
      "M9: the meter clears the prompt instead of overlapping it",
    ).toBeLessThanOrEqual(contextBox.y + 1);
    expect(
      inputAreaBox.x + inputAreaBox.width - (meterBox.x + meterBox.width),
      "M9: the meter is pinned to the trailing edge of the input band",
    ).toBeLessThanOrEqual(16);
    expect(
      meterBox.x,
      "M9: the meter is trailing, not leading",
    ).toBeGreaterThan(inputAreaBox.x + inputAreaBox.width / 2);
  });

  test("keeps the meter honest when capacity is unknown", async ({ page }) => {
    // M7's data contract: a bare reported count and no progressbar when the
    // runtime advertises no capacity. `6K context` must never be implied over
    // a capacity Sage was not told.
    await openScenario(page, "agent-composer-context-unknown", "agent");
    const context = page.getByLabel("Composer context", { exact: true });

    await expect(context).toContainText("Reported usage · 6,000 tokens");
    await expect(context).not.toContainText("6K context");
    await expect(context.getByRole("progressbar")).toHaveCount(0);
  });

  test("renders the mode picker in its settled control-row slot", async ({
    page,
  }) => {
    // M13 — present. The settled order places mode after project scope and
    // before the globe. Project scope is non-interactive, so it does not appear
    // in the control order; its position relative to mode is asserted directly.
    await openScenario(page, "agent-session-mode-closed", "agent");

    const order = await composerControlOrder(page);
    expect(
      order,
      "M13: the mode picker's settled slot is after model and before send",
    ).toEqual([
      "Harness",
      "Model",
      "Session mode",
      "Permission mode",
      "Send message",
    ]);

    const [scope, mode] = await Promise.all([
      boundsOf(composer(page).locator(".project-scope")),
      boundsOf(
        composer(page).getByRole("button", {
          name: "Session mode",
          exact: true,
        }),
      ),
    ]);
    expect(scope.x, "M13: project scope precedes the mode picker").toBeLessThan(
      mode.x,
    );

    await expect(
      composer(page).getByRole("button", { name: "Session mode", exact: true }),
    ).toHaveText("Review changes");
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

    // M2 — present, and in the target's own position. Resolved against the
    // reference image rather than the prose: the image puts the chips on the
    // affordance row *beneath* the input, alongside the paperclip, `@` and `/`.
    const attachments = composer(page).getByRole("list", {
      name: "Attached context",
    });
    await expect(attachments).toBeVisible();
    const chip = attachments.getByRole("listitem");
    await expect(chip).toHaveCount(1);
    await expect(
      chip.getByRole("button", { name: /^Remove .* context$/ }),
    ).toHaveCount(1);

    const [chipBox, inputBox, affordanceBox] = await Promise.all([
      boundsOf(attachments),
      boundsOf(page.getByRole("textbox", { name: "Message agent" })),
      boundsOf(composer(page).getByLabel("Composer actions", { exact: true })),
    ]);
    expect(
      chipBox.y,
      "M2: the attached-context chip row sits beneath the composer input",
    ).toBeGreaterThanOrEqual(inputBox.y + inputBox.height - 1);
    expect(
      Math.abs(
        chipBox.y +
          chipBox.height / 2 -
          (affordanceBox.y + affordanceBox.height / 2),
      ),
      "M2: the chips share the affordance row rather than taking a row of " +
        "their own",
    ).toBeLessThanOrEqual(affordanceBox.height);
    expect(
      chipBox.x,
      "M2: the chips follow the affordance glyphs",
    ).toBeGreaterThanOrEqual(affordanceBox.x);
  });
});
