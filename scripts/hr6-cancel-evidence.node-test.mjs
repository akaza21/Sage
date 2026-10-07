import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { JSDOM } from "jsdom";

import {
  TERMINAL_TOOL_STATES,
  TOOL_TERMINAL_GRACE_MS,
  cancelRunningToolExpression,
  cancelledToolOutcome,
  rollupRescanNeeded,
  runningRollups,
  runningToolReadiness,
  runningTools,
  toolIdentity,
  toolTerminalGrace,
} from "./hr6-cancel-evidence.mjs";
import {
  TERMINAL_LIMITS,
  terminalExpression,
  toolRollupExpression,
} from "./hr6-terminal-expression.mjs";
import { focusedChatSurfacePreamble } from "./hr6-chat-scope.mjs";

const live = {
  composerDisabled: true,
  cancelVisible: true,
  alerts: [],
  turnsExceeded: false,
  alertsExceeded: false,
};
const settled = {
  composerDisabled: false,
  cancelVisible: false,
  alerts: [],
  turnsExceeded: false,
  alertsExceeded: false,
};

function card(state, overrides) {
  return {
    id: "item-1",
    state,
    kind: "read",
    label: "Read alpha.txt",
    outputText: "",
    outputTruncated: false,
    outputVisible: false,
    resources: 0,
    resourcesExceeded: false,
    detail: false,
    ...overrides,
  };
}

function turn(tools, overrides) {
  return {
    id: "cancel",
    status: "Running",
    tools,
    toolsExceeded: false,
    ...overrides,
  };
}

function rollup(state, overrides) {
  return {
    kind: "read",
    state,
    open: false,
    cards: 0,
    ...overrides,
  };
}

function describe(state, overrides) {
  return {
    id: "item-1",
    kind: "read",
    label: "Read alpha.txt",
    state,
    ...overrides,
  };
}

function source() {
  return readFileSync(new URL("./hr6-live-run.mjs", import.meta.url), "utf8");
}

function cardMarkup(id, state, label = "Read alpha.txt") {
  return (
    '<article class="acp-activity-card acp-tool-card" data-activity-kind="read" data-activity-state="' +
    state +
    '" data-acp-item-id="' +
    id +
    '"><strong>' +
    label +
    "</strong></article>"
  );
}

function buildDom(cards, options = {}) {
  const cancelButton = options.cancelMissing
    ? ""
    : '<button aria-label="Cancel turn"' +
      (options.cancelDisabled ? " disabled" : "") +
      "></button>";
  const dom = new JSDOM(
    '<!doctype html><section class="agent-chat-pane agent-chat-pane--focused">' +
      '<section class="chat-surface" aria-label="Agent chat">' +
      '<section class="acp-turn" data-turn-id="earlier"><span data-turn-status aria-label="Completed"></span>' +
      cardMarkup("item-earlier", "completed") +
      '</section><section class="acp-turn" data-turn-id="cancel">' +
      '<span data-turn-status aria-label="Running"></span>' +
      cards.join("") +
      "</section>" +
      '<form class="composer"><textarea aria-label="Message agent" disabled></textarea>' +
      cancelButton +
      "</form></section></section>",
  );
  const button = dom.window.document.querySelector(
    'button[aria-label="Cancel turn"]',
  );
  const clicks = [];
  button?.addEventListener("click", () => clicks.push(Date.now()));
  return { dom, clicks };
}

function evaluate(dom, code) {
  return new Function("document", "return " + code + ";")(dom.window.document);
}

function attemptCancel(cards, toolId, options = {}) {
  const { dom, clicks } = buildDom(cards, options);
  const result = evaluate(
    dom,
    cancelRunningToolExpression(
      focusedChatSurfacePreamble('return {outcome:"scope-lost"};'),
      options.turnId ?? "cancel",
      toolId,
    ),
  );
  return { result, clicks, dom };
}

test("cancellation waits while the turn is live but no tool card exists yet", () => {
  assert.equal(
    runningToolReadiness({ ...live, turns: [turn([])] }, "cancel"),
    null,
  );
  assert.equal(runningToolReadiness({ ...live, turns: [] }, "cancel"), null);
  assert.equal(
    runningToolReadiness(
      { ...live, turns: [turn([card("inProgress")], { id: "other" })] },
      "cancel",
    ),
    null,
  );
});

test("a queued or approval-blocked tool card is never treated as running", () => {
  assert.equal(
    runningToolReadiness(
      { ...live, turns: [turn([card("pending")])] },
      "cancel",
    ),
    null,
  );
  assert.deepEqual(
    runningTools({ ...live, turns: [turn([card("pending")])] }, "cancel"),
    [],
  );
  assert.deepEqual(
    runningToolReadiness(
      { ...live, turns: [turn([card("waitingForApproval")])] },
      "cancel",
    ),
    {
      kind: "tool-awaiting-approval",
      tools: [describe("waitingForApproval")],
    },
  );
});

test("a running card without a stable item id is refused instead of cancelled blindly", () => {
  assert.deepEqual(
    runningToolReadiness(
      { ...live, turns: [turn([card("inProgress", { id: undefined })])] },
      "cancel",
    ),
    {
      kind: "tool-card-unidentified",
      tools: [describe("inProgress", { id: null })],
    },
  );
  assert.equal(toolIdentity({ id: "" }), null);
});

test("an observably running tool card in the current turn releases the cancel click", () => {
  assert.deepEqual(
    runningToolReadiness(
      {
        ...live,
        turns: [
          turn([
            card("completed"),
            card("inProgress", { id: "item-2", label: "Read README.md" }),
          ]),
        ],
      },
      "cancel",
    ),
    {
      kind: "tool-running",
      tools: [
        describe("inProgress", { id: "item-2", label: "Read README.md" }),
      ],
    },
  );
});

test("a turn that settles before any tool runs fails truthfully instead of cancelling", () => {
  assert.deepEqual(
    runningToolReadiness(
      { ...settled, turns: [turn([], { status: "Completed" })] },
      "cancel",
    ),
    { kind: "tool-absent", tools: [] },
  );
  assert.equal(
    runningToolReadiness(
      {
        ...settled,
        turns: [turn([card("completed")], { status: "Completed" })],
      },
      "cancel",
    ).kind,
    "turn-settled",
  );
  assert.deepEqual(
    runningToolReadiness(
      {
        ...live,
        alerts: ["Harness failure"],
        turns: [turn([card("inProgress")])],
      },
      "cancel",
    ),
    { kind: "error" },
  );
});

test("the cancel click rechecks the same card atomically and fires once while it runs", () => {
  const { result, clicks } = attemptCancel(
    [cardMarkup("item-1", "completed"), cardMarkup("item-2", "inProgress")],
    "item-2",
  );
  assert.deepEqual(result, { outcome: "cancelled", state: "inProgress" });
  assert.equal(clicks.length, 1);
});

test("a card that finishes between observation and click is never cancelled after the fact", () => {
  const { result, clicks } = attemptCancel(
    [cardMarkup("item-1", "completed"), cardMarkup("item-2", "completed")],
    "item-2",
  );
  assert.deepEqual(result, { outcome: "tool-not-running", state: "completed" });
  assert.equal(clicks.length, 0);
});

test("a same-label replacement card never stands in for the observed running card", () => {
  const { result, clicks } = attemptCancel(
    [cardMarkup("item-3", "inProgress")],
    "item-2",
  );
  assert.deepEqual(result, { outcome: "tool-card-lost" });
  assert.equal(clicks.length, 0);
});

test("the atomic click refuses a lost turn or an unavailable cancel control", () => {
  const lostTurn = attemptCancel(
    [cardMarkup("item-2", "inProgress")],
    "item-2",
    {
      turnId: "vanished",
    },
  );
  assert.deepEqual(lostTurn.result, { outcome: "turn-lost" });
  assert.equal(lostTurn.clicks.length, 0);
  const disabled = attemptCancel(
    [cardMarkup("item-2", "inProgress")],
    "item-2",
    {
      cancelDisabled: true,
    },
  );
  assert.deepEqual(disabled.result, {
    outcome: "cancel-unavailable",
    state: "inProgress",
  });
  assert.equal(disabled.clicks.length, 0);
  const missing = attemptCancel(
    [cardMarkup("item-2", "inProgress")],
    "item-2",
    {
      cancelMissing: true,
    },
  );
  assert.equal(missing.result.outcome, "cancel-unavailable");
});

test("post-cancel evidence refuses to conclude before the turn settles", () => {
  const observed = [describe("inProgress")];
  assert.equal(
    cancelledToolOutcome(
      { ...live, turns: [turn([card("inProgress")])] },
      "cancel",
      observed,
    ),
    null,
  );
  assert.equal(
    cancelledToolOutcome({ ...settled, turns: [] }, "cancel", observed),
    null,
  );
});

test("post-cancel evidence refuses an unproven or unidentified observation", () => {
  const cancelled = {
    ...settled,
    turns: [turn([card("cancelled")], { status: "Cancelled" })],
  };
  assert.deepEqual(cancelledToolOutcome(cancelled, "cancel", []), {
    kind: "observation-missing",
  });
  assert.deepEqual(cancelledToolOutcome(cancelled, "cancel", undefined), {
    kind: "observation-missing",
  });
  assert.deepEqual(
    cancelledToolOutcome(cancelled, "cancel", [
      { kind: "read", label: "Read alpha.txt" },
    ]),
    { kind: "observation-missing" },
  );
});

test("a demanded terminal state is checked against the card that was cancelled", () => {
  const naturallyDone = {
    ...settled,
    turns: [turn([card("completed")], { status: "Cancelled" })],
  };
  const stopped = {
    ...settled,
    turns: [turn([card("cancelled")], { status: "Cancelled" })],
  };
  const observed = [describe("inProgress")];
  assert.equal(
    cancelledToolOutcome(naturallyDone, "cancel", observed).kind,
    "tool-terminal",
  );
  const refused = cancelledToolOutcome(naturallyDone, "cancel", observed, [
    "cancelled",
  ]);
  assert.equal(refused.kind, "tool-not-cancelled");
  assert.deepEqual(refused.tools, [
    { id: "item-1", kind: "read", label: "Read alpha.txt", state: "completed" },
  ]);
  assert.equal(
    cancelledToolOutcome(stopped, "cancel", observed, ["cancelled"]).kind,
    "tool-terminal",
  );
  for (const required of [[], [""], ["cancelled", 7], "cancelled", {}])
    assert.throws(
      () => cancelledToolOutcome(stopped, "cancel", observed, required),
      { message: "hr6-cancel-required-states-invalid" },
    );
});

test("repeated identical reads cannot launder a lost card into terminal evidence", () => {
  const outcome = cancelledToolOutcome(
    {
      ...settled,
      turns: [
        turn(
          [
            card("completed", { id: "item-1" }),
            card("cancelled", { id: "item-3" }),
          ],
          { status: "Cancelled" },
        ),
      ],
    },
    "cancel",
    [describe("inProgress", { id: "item-2" })],
  );
  assert.equal(outcome.kind, "tool-card-lost");
  assert.equal(
    cancelledToolOutcome(
      {
        ...settled,
        turns: [
          turn([card("cancelled", { id: "item-1" })], { status: "Cancelled" }),
        ],
      },
      "cancel",
      [describe("inProgress"), describe("inProgress", { id: "item-2" })],
    ).kind,
    "tool-card-lost",
  );
});

test("a tool card left non-terminal after cancellation is refused", () => {
  const observed = [describe("inProgress")];
  assert.deepEqual(
    cancelledToolOutcome(
      {
        ...settled,
        turns: [turn([card("inProgress")], { status: "Cancelled" })],
      },
      "cancel",
      observed,
    ),
    { kind: "tool-not-terminal", tools: [describe("inProgress")] },
  );
  assert.equal(
    cancelledToolOutcome(
      {
        ...settled,
        turns: [
          turn(
            [
              card("cancelled"),
              card("pending", { id: "item-2", label: "Read README.md" }),
            ],
            { status: "Cancelled" },
          ),
        ],
      },
      "cancel",
      observed,
    ).kind,
    "tool-not-terminal",
  );
});

test("a cancelled running tool that reaches a terminal state is accepted", () => {
  const observed = [describe("inProgress")];
  assert.deepEqual(
    cancelledToolOutcome(
      {
        ...settled,
        turns: [turn([card("cancelled")], { status: "Cancelled" })],
      },
      "cancel",
      observed,
    ),
    { kind: "tool-terminal", tools: [describe("cancelled")] },
  );
  for (const state of TERMINAL_TOOL_STATES)
    assert.equal(
      cancelledToolOutcome(
        { ...settled, turns: [turn([card(state)], { status: "Cancelled" })] },
        "cancel",
        observed,
      ).kind,
      "tool-terminal",
    );
  assert.equal(
    cancelledToolOutcome(
      {
        ...settled,
        alerts: ["Harness failure"],
        turns: [turn([card("cancelled")], { status: "Cancelled" })],
      },
      "cancel",
      observed,
    ).kind,
    "error",
  );
});

test("the live projection carries the stable card id into the running-tool receipt", () => {
  const { dom } = buildDom([cardMarkup("item-2", "inProgress")]);
  const state = evaluate(
    dom,
    terminalExpression(focusedChatSurfacePreamble("return null;")),
  );
  assert.equal(state.cancelVisible, true);
  assert.deepEqual(runningToolReadiness(state, "cancel"), {
    kind: "tool-running",
    tools: [describe("inProgress", { id: "item-2" })],
  });
});

test("rollup expansion stays inside the current turn and never sweeps the transcript", () => {
  const dom = new JSDOM(
    '<!doctype html><section class="agent-chat-pane agent-chat-pane--focused">' +
      '<section class="chat-surface" aria-label="Agent chat">' +
      '<section class="acp-turn" data-turn-id="earlier">' +
      '<details class="acp-tool-rollup"></details></section>' +
      '<section class="acp-turn" data-turn-id="cancel">' +
      '<details class="acp-tool-rollup"></details></section></section></section>',
  );
  const opened = evaluate(
    dom,
    toolRollupExpression(focusedChatSurfacePreamble("return 0;"), "cancel"),
  );
  const all = [...dom.window.document.querySelectorAll("details")];
  assert.equal(opened, 1);
  assert.equal(all[0].open, false);
  assert.equal(all[1].open, true);
  assert.equal(
    evaluate(
      dom,
      toolRollupExpression(focusedChatSurfacePreamble("return 0;"), "vanished"),
    ),
    0,
  );
});

test("rollup expansion opens at most the bounded number of disclosures per poll", () => {
  const overflow = TERMINAL_LIMITS.tools + 7;
  const dom = new JSDOM(
    '<!doctype html><section class="agent-chat-pane agent-chat-pane--focused">' +
      '<section class="chat-surface" aria-label="Agent chat">' +
      '<section class="acp-turn" data-turn-id="cancel">' +
      '<details class="acp-tool-rollup"></details>'.repeat(overflow) +
      "</section></section></section>",
  );
  const opened = evaluate(
    dom,
    toolRollupExpression(focusedChatSurfacePreamble("return 0;"), "cancel"),
  );
  const details = [...dom.window.document.querySelectorAll("details")];
  assert.equal(opened, TERMINAL_LIMITS.tools);
  assert.equal(
    details.filter((node) => node.open).length,
    TERMINAL_LIMITS.tools,
  );
});

test("the runner cancels through the atomic seam and asserts the terminal card state", () => {
  const text = source();
  assert.match(text, /cancelRunningToolExpression\(/u);
  assert.match(
    text,
    /cancelledToolOutcome\(\s*terminal,\s*currentTurnId,\s*observedRunningTools,\s*options\.cancelTerminalStates \?\? null,?\s*\)/u,
  );
  const start = text.indexOf("async function submit(");
  const body = text.slice(start, text.indexOf("\nasync function ", start + 1));
  assert.ok(start > 0);
  assert.match(body, /observedRunningTools = await cancelRunningTool\(/u);
  assert.equal(body.includes("controls[0].click()"), false);
  assert.match(text, /hr6-timeout-\$\{label\}-cancel-tool-terminal/u);
});

test("the cancellation prompt drives repeated read-only tool calls and never mutates the scratch project", () => {
  const text = source();
  const prompt = text.match(/const TOOL_CANCEL_PROMPT =\n?\s*"([^"]+)";/u);
  assert.ok(prompt);
  assert.match(prompt[1], /read-only/iu);
  assert.match(prompt[1], /alpha\.txt/u);
  assert.match(prompt[1], /repeat/iu);
  assert.match(prompt[1], /do not (write|create|modify)/iu);
});

function countingDom(html) {
  const dom = new JSDOM(html);
  const reads = new Map();
  const iterations = new Map();
  const enumerated = new Map();
  const bump = (map, key) => map.set(key, (map.get(key) ?? 0) + 1);
  const visits = { matches: 0 };
  const originalAll = dom.window.Element.prototype.querySelectorAll;
  const originalMatches = dom.window.Element.prototype.matches;
  dom.window.Element.prototype.matches = function (selector) {
    visits.matches += 1;
    return originalMatches.call(this, selector);
  };
  dom.window.Element.prototype.querySelectorAll = function (selector) {
    bump(enumerated, selector);
    const list = originalAll.call(this, selector);
    return new Proxy(list, {
      get(target, prop) {
        if (prop === Symbol.iterator) bump(iterations, selector);
        else if (typeof prop === "string" && /^[0-9]+$/u.test(prop))
          bump(reads, selector);
        const value = Reflect.get(target, prop);
        return typeof value === "function" ? value.bind(target) : value;
      },
    });
  };
  dom.window.Element.prototype.querySelector = function (selector) {
    bump(enumerated, selector);
    return originalAll.call(this, selector)[0] ?? null;
  };
  return {
    dom,
    reads: (selector) => reads.get(selector) ?? 0,
    iterations: (selector) => iterations.get(selector) ?? 0,
    enumerated: (selector) => enumerated.get(selector) ?? 0,
    visits,
  };
}

const TRANSCRIPT_SELECTORS = [
  "section.acp-turn",
  ".acp-tool-card",
  '[role="alert"]',
  '.acp-message[data-message-role="assistant"] .acp-markdown',
];

function assertNoTranscriptEnumeration(counted) {
  for (const selector of TRANSCRIPT_SELECTORS) {
    assert.equal(counted.enumerated(selector), 0);
    assert.equal(counted.iterations(selector), 0);
    assert.equal(counted.reads(selector), 0);
  }
}

function turnMarkup(id, status, cards) {
  return (
    '<section class="acp-turn" data-turn-id="' +
    id +
    '"><span data-turn-status aria-label="' +
    status +
    '"></span>' +
    cards.join("") +
    "</section>"
  );
}

function surfaceMarkup(turns) {
  return (
    '<!doctype html><section class="agent-chat-pane agent-chat-pane--focused">' +
    '<section class="chat-surface" aria-label="Agent chat">' +
    turns.join("") +
    '<form class="composer"><textarea aria-label="Message agent" disabled></textarea>' +
    '<button aria-label="Cancel turn"></button></form>' +
    "</section></section>"
  );
}

function overflowMarkup(turnCount, cardCount) {
  const turns = [];
  for (let index = 0; index < turnCount; index += 1) {
    const last = index === turnCount - 1;
    const cards = [];
    for (let slot = 0; slot < cardCount; slot += 1) {
      const running = last && slot === cardCount - 1;
      cards.push(
        cardMarkup(
          "item-" + index + "-" + slot,
          running ? "inProgress" : "completed",
        ),
      );
    }
    turns.push(
      turnMarkup(
        last ? "cancel" : "turn-" + index,
        last ? "Running" : "Completed",
        cards,
      ),
    );
  }
  return surfaceMarkup(turns);
}

function clickLog(dom) {
  const clicks = [];
  dom.window.document
    .querySelector('button[aria-label="Cancel turn"]')
    .addEventListener("click", () => clicks.push(Date.now()));
  return clicks;
}

function cancelScanWork(turnCount, cardCount) {
  const counted = countingDom(overflowMarkup(turnCount, cardCount));
  const clicks = clickLog(counted.dom);
  const result = evaluate(
    counted.dom,
    cancelRunningToolExpression(
      focusedChatSurfacePreamble('return {outcome:"scope-lost"};'),
      "cancel",
      "item-" + (turnCount - 1) + "-" + (cardCount - 1),
    ),
  );
  return { counted, clicks, result };
}

function terminalScanWork(turnCount, cardCount) {
  const counted = countingDom(overflowMarkup(turnCount, cardCount));
  const state = evaluate(
    counted.dom,
    terminalExpression(focusedChatSurfacePreamble("return null;")),
  );
  return { counted, state };
}

test("the atomic cancel click finds its turn and card without scanning the transcript", () => {
  const counted = countingDom(
    overflowMarkup(TERMINAL_LIMITS.turns, TERMINAL_LIMITS.tools),
  );
  const clicks = clickLog(counted.dom);
  const result = evaluate(
    counted.dom,
    cancelRunningToolExpression(
      focusedChatSurfacePreamble('return {outcome:"scope-lost"};'),
      "cancel",
      "item-" + (TERMINAL_LIMITS.turns - 1) + "-" + (TERMINAL_LIMITS.tools - 1),
    ),
  );
  assert.deepEqual(result, { outcome: "cancelled", state: "inProgress" });
  assert.equal(clicks.length, 1);
  assertNoTranscriptEnumeration(counted);
  const small = cancelScanWork(60, TERMINAL_LIMITS.tools);
  const large = cancelScanWork(400, TERMINAL_LIMITS.tools);
  assert.deepEqual(small.result, { outcome: "turn-scan-exceeded" });
  assert.deepEqual(large.result, { outcome: "turn-scan-exceeded" });
  assert.equal(small.clicks.length, 0);
  assert.equal(large.clicks.length, 0);
  assert.equal(large.counted.visits.matches, small.counted.visits.matches);
});

test("the terminal projection reads bounded turns and cards instead of the whole transcript", () => {
  const counted = countingDom(overflowMarkup(200, 40));
  const state = evaluate(
    counted.dom,
    terminalExpression(focusedChatSurfacePreamble("return null;")),
  );
  assert.equal(state.turnCount, null);
  assert.equal(state.turnsExceeded, true);
  assert.equal(state.turns.length, TERMINAL_LIMITS.turns);
  assert.equal(state.turns[0].tools.length, TERMINAL_LIMITS.tools);
  assert.equal(state.turns[0].toolsExceeded, true);
  assert.equal(state.turns[TERMINAL_LIMITS.turns - 1].id, "cancel");
  assertNoTranscriptEnumeration(counted);
  const small = terminalScanWork(60, 40);
  const large = terminalScanWork(400, 40);
  assert.equal(large.state.turns.length, TERMINAL_LIMITS.turns);
  assert.equal(large.counted.visits.matches, small.counted.visits.matches);
});

test("a stale running card never redirects the cancel click at a newer turn", () => {
  const dom = new JSDOM(
    surfaceMarkup([
      turnMarkup("cancel", "Running", [cardMarkup("item-hot", "inProgress")]),
      turnMarkup("later", "Running", [cardMarkup("item-new", "inProgress")]),
    ]),
  );
  const clicks = clickLog(dom);
  const result = evaluate(
    dom,
    cancelRunningToolExpression(
      focusedChatSurfacePreamble('return {outcome:"scope-lost"};'),
      "cancel",
      "item-hot",
    ),
  );
  assert.deepEqual(result, { outcome: "turn-superseded" });
  assert.equal(clicks.length, 0);
});

test("a duplicated turn id is refused instead of guessing which turn to cancel", () => {
  const dom = new JSDOM(
    surfaceMarkup([
      turnMarkup("cancel", "Completed", [cardMarkup("item-old", "completed")]),
      turnMarkup("cancel", "Running", [cardMarkup("item-hot", "inProgress")]),
    ]),
  );
  const clicks = clickLog(dom);
  const result = evaluate(
    dom,
    cancelRunningToolExpression(
      focusedChatSurfacePreamble('return {outcome:"scope-lost"};'),
      "cancel",
      "item-hot",
    ),
  );
  assert.deepEqual(result, { outcome: "turn-ambiguous" });
  assert.equal(clicks.length, 0);
});

test("a turn with more tool cards than the scan admits is never cancelled on its visible tail", () => {
  const cards = [cardMarkup("item-hot", "inProgress")];
  for (let index = 0; index < TERMINAL_LIMITS.tools; index += 1)
    cards.push(cardMarkup("item-" + index, "completed"));
  const { dom, clicks } = buildDom(cards);
  const state = evaluate(
    dom,
    terminalExpression(focusedChatSurfacePreamble("return null;")),
  );
  const projected = state.turns.find((candidate) => candidate.id === "cancel");
  const readiness = runningToolReadiness(state, "cancel");

  assert.equal(projected.tools.length, TERMINAL_LIMITS.tools);
  assert.equal(
    projected.tools.some((tool) => tool.id === "item-hot"),
    false,
  );
  assert.equal(projected.toolsExceeded, true);
  assert.equal(readiness.kind, "scan-exceeded");
  assert.equal(readiness.tools.length, TERMINAL_LIMITS.tools);
  assert.equal(clicks.length, 0);
});

test("an overflowed tool-card or turn scan fails closed on every non-false flag", () => {
  const observed = [describe("inProgress")];
  const cancelled = (overrides) =>
    turn([card("cancelled")], { status: "Cancelled", ...overrides });
  for (const flag of [true, undefined, "false"]) {
    assert.deepEqual(
      runningToolReadiness(
        {
          ...live,
          turns: [turn([card("inProgress")], { toolsExceeded: flag })],
        },
        "cancel",
      ),
      { kind: "scan-exceeded", tools: [describe("inProgress")] },
    );
    assert.deepEqual(
      cancelledToolOutcome(
        { ...settled, turns: [cancelled({ toolsExceeded: flag })] },
        "cancel",
        observed,
      ),
      { kind: "scan-exceeded", tools: [describe("cancelled")] },
    );
    assert.equal(
      runningToolReadiness(
        { ...live, turnsExceeded: flag, turns: [turn([card("inProgress")])] },
        "cancel",
      ).kind,
      "scan-exceeded",
    );
    assert.equal(
      cancelledToolOutcome(
        { ...settled, turnsExceeded: flag, turns: [cancelled()] },
        "cancel",
        observed,
      ).kind,
      "scan-exceeded",
    );
  }
});

test("the atomic click refuses a card id it cannot prove unique inside the scan", () => {
  const overflow = [cardMarkup("item-hot", "inProgress")];
  for (let index = 0; index < TERMINAL_LIMITS.tools - 1; index += 1)
    overflow.push(cardMarkup("item-" + index, "completed"));
  overflow.push(cardMarkup("item-hot", "inProgress"));
  const beyondCap = attemptCancel(overflow, "item-hot");
  const withinCap = attemptCancel(
    [
      cardMarkup("item-hot", "inProgress"),
      cardMarkup("item-hot", "inProgress"),
    ],
    "item-hot",
  );

  assert.deepEqual(beyondCap.result, { outcome: "tool-scan-exceeded" });
  assert.equal(beyondCap.clicks.length, 0);
  assert.deepEqual(withinCap.result, { outcome: "tool-card-ambiguous" });
  assert.equal(withinCap.clicks.length, 0);
});

test("the runner gives up on an unprovable card scan instead of retrying to its deadline", () => {
  const text = source();
  assert.match(text, /result\?\.outcome === "tool-scan-exceeded"/u);
  assert.match(text, /result\?\.outcome === "tool-card-ambiguous"/u);
  assert.match(text, /result\?\.outcome === "turn-scan-exceeded"/u);
  assert.match(text, /result\?\.outcome === "control-scan-exceeded"/u);
});

test("a transcript past the turn cap is refused by the atomic expression, not clicked", () => {
  const counted = countingDom(overflowMarkup(TERMINAL_LIMITS.turns + 1, 1));
  const clicks = clickLog(counted.dom);
  const state = evaluate(
    counted.dom,
    terminalExpression(focusedChatSurfacePreamble("return null;")),
  );
  const result = evaluate(
    counted.dom,
    cancelRunningToolExpression(
      focusedChatSurfacePreamble('return {outcome:"scope-lost"};'),
      "cancel",
      "item-" + TERMINAL_LIMITS.turns + "-0",
    ),
  );

  assert.equal(state.turnsExceeded, true);
  assert.deepEqual(result, { outcome: "turn-scan-exceeded" });
  assert.equal(clicks.length, 0);
});

test("a composer past the cancel-control scan cap is refused as a scan failure", () => {
  const dom = new JSDOM(
    '<!doctype html><section class="agent-chat-pane agent-chat-pane--focused">' +
      '<section class="chat-surface" aria-label="Agent chat">' +
      turnMarkup("cancel", "Running", [cardMarkup("item-hot", "inProgress")]) +
      '<form class="composer"><textarea aria-label="Message agent" disabled></textarea>' +
      '<button aria-label="Cancel turn"></button>'.repeat(3) +
      "</form></section></section>",
  );
  const clicks = [];
  for (const button of dom.window.document.querySelectorAll(
    'button[aria-label="Cancel turn"]',
  ))
    button.addEventListener("click", () => clicks.push(Date.now()));
  const result = evaluate(
    dom,
    cancelRunningToolExpression(
      focusedChatSurfacePreamble('return {outcome:"scope-lost"};'),
      "cancel",
      "item-hot",
    ),
  );

  assert.deepEqual(result, {
    outcome: "control-scan-exceeded",
    state: "inProgress",
  });
  assert.equal(clicks.length, 0);
});

test("a duplicated turn id is never laundered into cancellation evidence", () => {
  assert.deepEqual(
    runningToolReadiness(
      {
        ...live,
        turns: [
          turn([card("completed")], { status: "Completed" }),
          turn([card("inProgress")]),
        ],
      },
      "cancel",
    ),
    { kind: "turn-ambiguous" },
  );
  assert.deepEqual(
    cancelledToolOutcome(
      {
        ...settled,
        turns: [
          turn([card("cancelled")], { status: "Cancelled" }),
          turn([card("inProgress")], { status: "Cancelled" }),
        ],
      },
      "cancel",
      [describe("cancelled")],
    ),
    { kind: "turn-ambiguous" },
  );
});

test("a same-id turn with an unreadable status is never outvoted by its twin", () => {
  assert.deepEqual(
    runningToolReadiness(
      {
        ...live,
        turns: [turn([card("inProgress")]), turn([], { status: null })],
      },
      "cancel",
    ),
    { kind: "turn-ambiguous" },
  );
  assert.deepEqual(
    cancelledToolOutcome(
      {
        ...settled,
        turns: [
          turn([card("cancelled")], { status: "Cancelled" }),
          turn([], { status: undefined }),
        ],
      },
      "cancel",
      [describe("cancelled")],
    ),
    { kind: "turn-ambiguous" },
  );
});

test("an alert region that outran its scan is never read as a quiet surface", () => {
  for (const flag of [true, undefined, "false"]) {
    assert.deepEqual(
      runningToolReadiness(
        {
          ...live,
          alertsExceeded: flag,
          turns: [turn([card("inProgress")])],
        },
        "cancel",
      ),
      { kind: "scan-exceeded", tools: [describe("inProgress")] },
    );
    assert.deepEqual(
      cancelledToolOutcome(
        {
          ...settled,
          alertsExceeded: flag,
          turns: [turn([card("cancelled")], { status: "Cancelled" })],
        },
        "cancel",
        [describe("cancelled")],
      ),
      { kind: "scan-exceeded", tools: [describe("cancelled")] },
    );
  }
});

test("two terminal cards sharing an id are never counted as the observed one", () => {
  assert.deepEqual(
    cancelledToolOutcome(
      {
        ...settled,
        alertsExceeded: false,
        turns: [
          turn([card("cancelled"), card("completed")], { status: "Cancelled" }),
        ],
      },
      "cancel",
      [describe("cancelled")],
    ),
    {
      kind: "tool-card-ambiguous",
      tools: [describe("cancelled"), describe("completed")],
    },
  );
});

test("a surface with no composer never lets a stray cancel control be clicked", () => {
  const dom = new JSDOM(
    '<!doctype html><section class="agent-chat-pane agent-chat-pane--focused">' +
      '<section class="chat-surface" aria-label="Agent chat">' +
      '<section class="acp-turn" data-turn-id="cancel">' +
      '<span data-turn-status aria-label="Running"></span>' +
      cardMarkup("item-hot", "inProgress") +
      "</section>" +
      '<div class="toolbar"><button aria-label="Cancel turn"></button></div>' +
      "</section></section>",
  );
  const clicks = [];
  dom.window.document
    .querySelector('button[aria-label="Cancel turn"]')
    .addEventListener("click", () => clicks.push(Date.now()));
  assert.deepEqual(
    evaluate(
      dom,
      cancelRunningToolExpression(
        focusedChatSurfacePreamble('return {outcome:"scope-lost"};'),
        "cancel",
        "item-hot",
      ),
    ),
    { outcome: "composer-unresolved", state: "inProgress" },
  );
  assert.equal(clicks.length, 0);
});

test("the runner gives up on an unresolved composer instead of clicking elsewhere", () => {
  assert.match(source(), /result\?\.outcome === "composer-unresolved"/u);
});

test("the runner refuses a superseded or ambiguous turn and scopes its cancel screenshots", () => {
  const text = source();
  assert.match(
    text,
    /async function snapshot\(cdp, chat, name, turnId = null\)/u,
  );
  assert.match(text, /await expandToolRollups\(cdp, chat, turnId\);/u);
  assert.match(text, /result\?\.outcome === "turn-superseded"/u);
  assert.match(text, /result\?\.outcome === "turn-ambiguous"/u);
  assert.match(text, /hr6-cancel-tool-\$\{result\.outcome\}-\$\{label\}/u);
  assert.equal(text.includes("surface?.innerText"), false);
  assert.equal(text.includes("boundWide(x.innerText)"), false);
  const cancelStart = text.indexOf("async function cancelRunningTool(");
  const cancelBody = text.slice(
    cancelStart,
    text.indexOf("\nasync function ", cancelStart + 1),
  );
  assert.ok(cancelStart > 0);
  for (const match of cancelBody.matchAll(/await snapshot\(([^;]+)\);/gu))
    assert.match(match[1], /turnId/u);
});

test("a not-yet-terminal tool card earns a bounded grace, never an open wait", () => {
  const evidence = {
    kind: "tool-not-terminal",
    tools: [describe("inProgress")],
  };
  assert.ok(TOOL_TERMINAL_GRACE_MS > 0);
  const opened = toolTerminalGrace(evidence, null, 1000);
  assert.deepEqual(opened, { kind: "grace", startedAt: 1000, waitedMs: 0 });
  const waiting = toolTerminalGrace(
    evidence,
    opened.startedAt,
    1000 + TOOL_TERMINAL_GRACE_MS - 1,
  );
  assert.equal(waiting.kind, "grace");
  assert.equal(waiting.waitedMs, TOOL_TERMINAL_GRACE_MS - 1);
  assert.equal(waiting.startedAt, 1000);
  const expired = toolTerminalGrace(
    evidence,
    opened.startedAt,
    1000 + TOOL_TERMINAL_GRACE_MS,
  );
  assert.equal(expired.kind, "expired");
  assert.equal(expired.waitedMs, TOOL_TERMINAL_GRACE_MS);
});

test("the grace covers only a not-yet-terminal card and never other evidence", () => {
  const idle = { kind: "other", startedAt: null, waitedMs: 0 };
  for (const kind of [
    "tool-terminal",
    "tool-card-lost",
    "observation-missing",
    "error",
  ])
    assert.deepEqual(toolTerminalGrace({ kind }, 1000, 99999), idle);
  assert.deepEqual(toolTerminalGrace(null, 1000, 99999), idle);
  assert.deepEqual(toolTerminalGrace(undefined, 1000, 99999), idle);
});

test("a card still in progress is never graced into a terminal reading", () => {
  const observed = [describe("inProgress")];
  const evidence = cancelledToolOutcome(
    {
      ...settled,
      turns: [turn([card("inProgress")], { status: "Cancelled" })],
    },
    "cancel",
    observed,
  );
  assert.equal(evidence.kind, "tool-not-terminal");
  for (const now of [
    0,
    1,
    TOOL_TERMINAL_GRACE_MS - 1,
    TOOL_TERMINAL_GRACE_MS,
    TOOL_TERMINAL_GRACE_MS * 10,
  ]) {
    const grace = toolTerminalGrace(evidence, 0, now);
    assert.notEqual(evidence.kind, "tool-terminal");
    assert.ok(grace.kind === "grace" || grace.kind === "expired");
    assert.equal(grace.waitedMs, now);
  }
});

test("the runner polls the same card through the grace before it gives up", () => {
  const text = source();
  const start = text.indexOf("} else if (cancel) {");
  assert.ok(start > 0);
  const body = text.slice(
    start,
    text.indexOf("} else if (outcome) break;", start),
  );
  assert.match(
    body,
    /toolTerminalGrace\(\s*cancelEvidence,\s*graceStartedAt,\s*Date\.now\(\),?\s*\)/u,
  );
  const graceStart = body.indexOf(String.raw`if (grace.kind === "grace")`);
  const fatal = body.indexOf(
    String.raw`cancelEvidence.kind !== "tool-terminal"`,
  );
  assert.ok(graceStart > 0);
  assert.ok(fatal > graceStart);
  const graceBody = body.slice(graceStart, fatal);
  assert.match(graceBody, /continue;/u);
  assert.equal(graceBody.includes("observations.steps.push"), false);
  assert.equal(graceBody.includes("await snapshot("), false);
  assert.match(body, /graceWaitedMs/u);
});

test("an oversized activity state is bounded before it crosses CDP", () => {
  const state = "s".repeat(TERMINAL_LIMITS.label * 4);
  const { result, clicks } = attemptCancel(
    [cardMarkup("item-hot", state)],
    "item-hot",
  );
  assert.equal(result.outcome, "tool-not-running");
  assert.equal(
    result.state.length,
    TERMINAL_LIMITS.label + TERMINAL_LIMITS.marker.length,
  );
  assert.ok(result.state.endsWith(TERMINAL_LIMITS.marker));
  assert.equal(clicks.length, 0);
});

test("a rollup that reports work in progress is reported without an item identity", () => {
  const state = {
    ...live,
    turns: [turn([], { rollups: [rollup("inProgress"), rollup("completed")] })],
  };
  const found = runningRollups(state, "cancel");
  assert.deepEqual(found, [
    { kind: "read", state: "inProgress", open: false, cards: 0 },
  ]);
  assert.deepEqual(runningTools(state, "cancel"), []);
  assert.equal(runningRollups(state, "other").length, 0);
});

test("a collapsed running rollup asks for an expand and rescan instead of a blind wait", () => {
  const collapsed = {
    ...live,
    turns: [turn([], { rollups: [rollup("inProgress")] })],
  };
  assert.equal(rollupRescanNeeded(collapsed, "cancel"), true);
  assert.equal(runningToolReadiness(collapsed, "cancel"), null);

  const opened = {
    ...live,
    turns: [
      turn([card("completed")], {
        rollups: [rollup("inProgress", { open: true, cards: 1 })],
      }),
    ],
  };
  assert.equal(rollupRescanNeeded(opened, "cancel"), false);

  const openedEmpty = {
    ...live,
    turns: [turn([], { rollups: [rollup("inProgress", { open: true })] })],
  };
  assert.equal(rollupRescanNeeded(openedEmpty, "cancel"), true);
});

test("a rescan is never requested once a card is running or the turn has settled", () => {
  const running = {
    ...live,
    turns: [
      turn([card("inProgress")], {
        rollups: [rollup("inProgress", { open: true, cards: 1 })],
      }),
    ],
  };
  assert.equal(rollupRescanNeeded(running, "cancel"), false);
  assert.equal(runningToolReadiness(running, "cancel").kind, "tool-running");

  const done = {
    ...settled,
    turns: [turn([], { status: "Completed", rollups: [rollup("inProgress")] })],
  };
  assert.equal(rollupRescanNeeded(done, "cancel"), false);

  const quiet = { ...live, turns: [turn([], { rollups: [] })] };
  assert.equal(rollupRescanNeeded(quiet, "cancel"), false);
  assert.equal(
    rollupRescanNeeded({ ...live, turns: [turn([])] }, "cancel"),
    false,
  );
  assert.equal(rollupRescanNeeded(collapsedMissingTurn(), "cancel"), false);
});

function collapsedMissingTurn() {
  return {
    ...live,
    turns: [turn([], { id: "other", rollups: [rollup("inProgress")] })],
  };
}

test("the projection reports a collapsed rollup aggregate that hides its tool cards", () => {
  const dom = new JSDOM(
    surfaceMarkup([
      turnMarkup("cancel", "Running", [
        '<details class="acp-activity-card acp-activity-card--tool-rollup acp-tool-rollup" data-activity-kind="read" data-activity-state="inProgress"><summary>Read activity</summary></details>',
      ]),
    ]),
  );
  const state = evaluate(
    dom,
    terminalExpression(focusedChatSurfacePreamble("return null;")),
  );
  const projected = state.turns.find((candidate) => candidate.id === "cancel");
  assert.deepEqual(projected.tools, []);
  assert.equal(projected.rollupsExceeded, false);
  assert.deepEqual(projected.rollups, [
    { kind: "read", state: "inProgress", open: false, cards: 0 },
  ]);
  assert.equal(rollupRescanNeeded(state, "cancel"), true);
});

test("an expanded rollup reports its children so the exact card rule still decides", () => {
  const dom = new JSDOM(
    surfaceMarkup([
      turnMarkup("cancel", "Running", [
        '<details open class="acp-activity-card acp-activity-card--tool-rollup acp-tool-rollup" data-activity-kind="read" data-activity-state="inProgress"><summary>Read activity</summary>' +
          cardMarkup("item-hot", "inProgress") +
          cardMarkup("item-cold", "completed") +
          "</details>",
      ]),
    ]),
  );
  const state = evaluate(
    dom,
    terminalExpression(focusedChatSurfacePreamble("return null;")),
  );
  const projected = state.turns.find((candidate) => candidate.id === "cancel");
  assert.deepEqual(projected.rollups, [
    { kind: "read", state: "inProgress", open: true, cards: 2 },
  ]);
  assert.equal(projected.tools.length, 2);
  assert.equal(rollupRescanNeeded(state, "cancel"), false);
  assert.deepEqual(
    runningTools(state, "cancel").map((tool) => tool.id),
    ["item-hot"],
  );
});

test("the cancel loop expands and rescans a collapsed running rollup before yielding", () => {
  const text = source();
  const body = text.slice(
    text.indexOf("async function cancelRunningTool("),
    text.indexOf("async function waitForSubmissionReadiness("),
  );
  assert.match(body, /rollupRescanNeeded\(state, turnId\)/u);
  assert.match(body, /rollupRescans/u);
  const rescan = body.indexOf("rollupRescanNeeded(state, turnId)");
  const yieldAt = body.indexOf("await pollYield(runningDeadline);");
  assert.ok(rescan > 0);
  assert.ok(yieldAt > rescan);
  assert.match(body.slice(rescan), /continue;/u);
  assert.match(body, /attempts,\s*rollupRescans,/u);
});
