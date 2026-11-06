import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { JSDOM } from "jsdom";

import {
  composerReady,
  discoveryReady,
  harnessPickerReadiness,
  harnessReadiness,
  POLL_INTERVAL_MS,
  boundedRead,
  CDP_OPERATION_CAP_MS,
  PROBE_MARGIN_MS,
  pollDelay,
  pollReadiness,
  pollTurnAcceptance,
  probeExpiry,
  submissionReadiness,
  turnAcceptanceOutcome,
  turnIdsPresent,
  turnOutcome,
  turnSettled,
} from "./hr6-readiness.mjs";
import {
  TERMINAL_LIMITS,
  boundedFirstProjection,
  boundedHeadProjection,
  boundedIdentityProjection,
  boundedMatchProjection,
  boundedNodeTextProjection,
  boundedScalarProjection,
  boundedScopeProjection,
  boundedTextProjection,
  boundedTurnsProjection,
  terminalExpression,
  toolCardsProjection,
} from "./hr6-terminal-expression.mjs";
import { focusedChatSurfacePreamble } from "./hr6-chat-scope.mjs";
import {
  TOOL_TERMINAL_GRACE_MS,
  cancelledToolOutcome,
  toolTerminalGrace,
} from "./hr6-cancel-evidence.mjs";

function escapeMarkup(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

test("live harness selection waits until discovery replaces detecting options", () => {
  assert.equal(
    discoveryReady([
      { text: "Codex — detecting" },
      { text: "Claude — detecting" },
    ]),
    false,
  );
  assert.equal(
    discoveryReady([{ text: "Codex — detectingDetecting harnesses…" }]),
    false,
  );
  assert.equal(
    discoveryReady([
      { text: "Codex — Available" },
      { text: "Claude — Sign in required" },
    ]),
    true,
  );
});

test("live harness selection waits through the transient empty state from the Codex frame sequence", () => {
  const sequence = [
    { status: "Checking", composerDisabled: true, alerts: [] },
    { status: null, composerDisabled: true, alerts: [] },
    { status: "Ready", composerDisabled: true, alerts: [] },
    { status: "Checking", composerDisabled: true, alerts: [] },
    { status: "Ready", composerDisabled: false, alerts: [] },
  ];

  assert.deepEqual(sequence.map(harnessReadiness), [
    false,
    false,
    false,
    false,
    true,
  ]);
});

test("a harness selection ends early only with a settled usable or curated auth state", () => {
  assert.equal(
    harnessReadiness({ status: "Created", composerDisabled: true }),
    false,
  );
  assert.equal(
    harnessReadiness({ status: "Unavailable", composerDisabled: true }),
    true,
  );
  assert.equal(
    harnessReadiness({ status: "Sign in required", composerDisabled: true }),
    true,
  );
});

test("harness selection waits for project authority before opening the picker", () => {
  assert.equal(
    harnessPickerReadiness({
      triggerPresent: true,
      triggerDisabled: true,
      projectLoading: true,
      alertCount: 0,
      alertsExceeded: false,
    }),
    "transitioning",
  );
  assert.equal(
    harnessPickerReadiness({
      triggerPresent: true,
      triggerDisabled: false,
      projectLoading: false,
      alertCount: 0,
      alertsExceeded: false,
    }),
    "ready",
  );
  assert.equal(
    harnessPickerReadiness({
      triggerPresent: true,
      triggerDisabled: true,
      projectLoading: false,
      alertCount: 0,
      alertsExceeded: false,
    }),
    "persistent",
  );
  assert.equal(
    harnessPickerReadiness({
      triggerPresent: true,
      triggerDisabled: false,
      projectLoading: false,
      alertCount: 1,
      alertsExceeded: false,
    }),
    "error",
  );
});

test("readiness polling waits through project loading before selecting", async () => {
  const states = [
    {
      triggerPresent: true,
      triggerDisabled: true,
      projectLoading: true,
      alertCount: 0,
      alertsExceeded: false,
    },
    {
      triggerPresent: true,
      triggerDisabled: false,
      projectLoading: false,
      alertCount: 0,
      alertsExceeded: false,
    },
  ];
  const events = [];
  const result = await pollReadiness(
    async () => {
      events.push("probe");
      return states.shift();
    },
    harnessPickerReadiness,
    {
      until: 10,
      now: () => 0,
      yieldFrame: async () => events.push("frame"),
    },
  );

  assert.equal(result.status, "ready");
  assert.deepEqual(events, ["probe", "frame", "probe"]);
});

test("readiness polling reports scope loss, persistent failure, and timeout", async () => {
  const scopeLost = await pollReadiness(
    async () => null,
    harnessPickerReadiness,
    { until: 10, now: () => 0, yieldFrame: async () => undefined },
  );
  assert.equal(scopeLost.status, "scopeLost");

  const undefinedScope = await pollReadiness(
    async () => undefined,
    harnessPickerReadiness,
    { until: 10, now: () => 0, yieldFrame: async () => undefined },
  );
  assert.equal(undefinedScope.status, "scopeLost");

  const persistent = await pollReadiness(
    async () => ({
      triggerPresent: true,
      triggerDisabled: true,
      projectLoading: false,
      alertCount: 0,
      alertsExceeded: false,
    }),
    harnessPickerReadiness,
    { until: 10, now: () => 0, yieldFrame: async () => undefined },
  );
  assert.equal(persistent.status, "persistent");

  let now = 0;
  const timeout = await pollReadiness(
    async () => ({
      triggerPresent: true,
      triggerDisabled: true,
      projectLoading: true,
      alertCount: 0,
      alertsExceeded: false,
    }),
    harnessPickerReadiness,
    {
      until: 10,
      now: () => now,
      yieldFrame: async () => {
        now = 10;
      },
    },
  );
  assert.equal(timeout.status, "timeout");
});

test("live prompt submission waits for React to commit the requested composer text", () => {
  assert.equal(composerReady({ value: "", disabled: false }, "PONG"), false);
  assert.equal(composerReady({ value: "PONG", disabled: true }, "PONG"), false);
  assert.equal(composerReady({ value: "PONG", disabled: false }, "PONG"), true);
});

test("turn acceptance classifies every observable send outcome", () => {
  const initial = {
    expectedPane: "claude",
    left: { turnCount: 2 },
    right: { turnCount: 3 },
  };
  const sample = (overrides = {}) => ({
    focusedPane: "claude",
    left: { turnCount: 2, alertClass: "none" },
    right: {
      turnCount: 3,
      alertClass: "none",
      composerHasValue: true,
      composerDisabled: false,
    },
    ...overrides,
  });

  assert.equal(
    turnAcceptanceOutcome(
      initial,
      sample({ right: { turnCount: 4, alertClass: "none" } }),
    ),
    "accepted",
  );
  assert.equal(
    turnAcceptanceOutcome(
      initial,
      sample({ left: { turnCount: 3, alertClass: "none" } }),
    ),
    "routed-left",
  );
  assert.equal(
    turnAcceptanceOutcome(initial, sample({ focusedPane: "codex" })),
    "focus-drift",
  );
  assert.equal(
    turnAcceptanceOutcome(
      initial,
      sample({ right: { turnCount: 3, alertClass: "present" } }),
    ),
    "rejected-alert",
  );
  assert.equal(
    turnAcceptanceOutcome(
      { ...initial, right: { turnCount: 3, alertClass: "present" } },
      sample({ right: { turnCount: 3, alertClass: "present" } }),
    ),
    "not-dispatched",
  );
  assert.equal(
    turnAcceptanceOutcome(
      initial,
      sample({
        right: {
          turnCount: 3,
          alertClass: "none",
          composerHasValue: true,
          composerDisabled: true,
        },
      }),
    ),
    "activation-stalled",
  );
  assert.equal(
    turnAcceptanceOutcome(
      initial,
      sample({
        right: {
          turnCount: 3,
          alertClass: "none",
          composerHasValue: false,
          composerDisabled: false,
        },
      }),
    ),
    "prompt-stalled",
  );
  assert.equal(turnAcceptanceOutcome(initial, sample()), "not-dispatched");
});

test("turn acceptance sampler records only changes and never exceeds its bound", async () => {
  const initial = {
    expectedPane: "claude",
    left: { turnCount: 1 },
    right: { turnCount: 1 },
  };
  const unchanged = {
    focusedPane: "claude",
    left: { turnCount: 1, alertClass: "none" },
    right: {
      turnCount: 1,
      alertClass: "none",
      composerHasValue: true,
      composerDisabled: false,
    },
  };
  let now = 0;
  let probes = 0;
  let waits = 0;
  const result = await pollTurnAcceptance(
    async () => {
      probes += 1;
      return unchanged;
    },
    initial,
    {
      now: () => now,
      until: 25_000,
      wait: async () => {
        waits += 1;
        now += 250;
      },
    },
  );

  assert.equal(result.outcome, "not-dispatched");
  assert.equal(result.samples.length, 1);
  assert.equal(probes, 101);
  assert.equal(waits, 100);
  assert.equal(now, 25_000);
});

test("turn acceptance sampler caps changing diagnostic samples", async () => {
  const initial = {
    expectedPane: "claude",
    left: { turnCount: 1 },
    right: { turnCount: 1 },
  };
  let index = 0;
  const result = await pollTurnAcceptance(
    async () => ({
      focusedPane: "claude",
      left: { turnCount: 1, alertClass: "none" },
      right: {
        turnCount: 1,
        alertClass: "none",
        composerHasValue: true,
        composerDisabled: false,
        statusClass: String(index++),
      },
    }),
    initial,
    { now: () => 0, until: 25_000, wait: async () => undefined },
  );

  assert.equal(result.outcome, "not-dispatched");
  assert.equal(result.samples.length, 101);
  assert.equal(index, 102);
});

test("turn acceptance snapshot keeps CDP evaluation single-flight", async () => {
  const source = readFileSync(
    new URL("./hr6-live-run.mjs", import.meta.url),
    "utf8",
  );
  const start = source.indexOf("async function turnAcceptanceSample");
  const end = source.indexOf("async function waitForAcceptedTurn", start);
  let pending = false;
  let calls = 0;
  const sampler = new Function(
    "evaluate",
    "expression",
    "deadline",
    "boundedFirstProjection",
    "boundedIdentityProjection",
    "boundedMatchProjection",
    "boundedNodeTextProjection",
    "boundedScopeProjection",
    "boundedTurnsProjection",
    `${source.slice(start, end)}return turnAcceptanceSample;`,
  )(
    async (_cdp, value) => {
      assert.doesNotThrow(() => new Function(value));
      assert.equal(pending, false);
      pending = true;
      calls += 1;
      await Promise.resolve();
      pending = false;
      return { focusedPane: "claude", left: null, right: null };
    },
    (value) => value,
    Number.MAX_SAFE_INTEGER,
    boundedFirstProjection,
    boundedIdentityProjection,
    boundedMatchProjection,
    boundedNodeTextProjection,
    boundedScopeProjection,
    boundedTurnsProjection,
  );

  const sample = await sampler({}, "claude", Number.MAX_SAFE_INTEGER);
  assert.equal(calls, 1);
  assert.deepEqual(sample, {
    focusedPane: "claude",
    left: null,
    right: null,
  });
});

test("a nonaccepted turn diagnostic captures its bounded screenshot", async () => {
  const source = readFileSync(
    new URL("./hr6-live-run.mjs", import.meta.url),
    "utf8",
  );
  const start = source.indexOf("async function waitForAcceptedTurn");
  const end = source.indexOf("async function rememberFocusedChat", start);
  const written = [];
  const shots = [];
  const observations = { steps: [] };
  const diagnosticDeadline = Date.now() + 1_000;
  const waitForAcceptedTurn = new Function(
    "deadline",
    "pollTurnAcceptance",
    "turnAcceptanceSample",
    "runnerStartedAt",
    "observations",
    "writeJson",
    "shot",
    `${source.slice(start, end)}return waitForAcceptedTurn;`,
  )(
    diagnosticDeadline,
    async () => ({ outcome: "routed-left", samples: [] }),
    async () => null,
    Date.now(),
    observations,
    (name, value) => written.push({ name, value }),
    async (_cdp, name, until) => {
      shots.push({ name, until });
      throw new Error("screenshot unavailable");
    },
  );

  await assert.rejects(
    () => waitForAcceptedTurn({}, "claude", {}, "07-claude-pong"),
    /hr6-turn-acceptance-routed-left-07-claude-pong/u,
  );
  assert.deepEqual(shots, [
    { name: "07-claude-pong-turn-acceptance", until: diagnosticDeadline },
  ]);
  assert.equal(written[0]?.name, "07-claude-pong-turn-acceptance");
  assert.equal(observations.steps[0]?.value.outcome, "routed-left");
});

test("turn acceptance sampler stops when a changed sample proves acceptance", async () => {
  const initial = {
    expectedPane: "claude",
    left: { turnCount: 1 },
    right: { turnCount: 1 },
  };
  const samples = [
    {
      focusedPane: "claude",
      left: { turnCount: 1, alertClass: "none" },
      right: {
        turnCount: 1,
        alertClass: "none",
        composerHasValue: true,
        composerDisabled: false,
      },
    },
    {
      focusedPane: "claude",
      left: { turnCount: 1, alertClass: "none" },
      right: {
        turnCount: 2,
        alertClass: "none",
        composerHasValue: false,
        composerDisabled: true,
      },
    },
  ];
  let index = 0;
  const result = await pollTurnAcceptance(
    async () => samples[index++],
    initial,
    {
      now: () => 0,
      until: 25_000,
      wait: async () => undefined,
    },
  );

  assert.equal(result.outcome, "accepted");
  assert.deepEqual(result.samples, samples);
});

test("turn acceptance sampler lets transient post-send stalls reach acceptance", async () => {
  const initial = {
    expectedPane: "claude",
    left: { turnCount: 1 },
    right: { turnCount: 1, alertClass: "present" },
  };
  const samples = [
    {
      focusedPane: "codex",
      left: { turnCount: 1, alertClass: "none" },
      right: {
        turnCount: 1,
        alertClass: "present",
        composerHasValue: true,
        composerDisabled: false,
      },
    },
    {
      focusedPane: "claude",
      left: { turnCount: 1, alertClass: "none" },
      right: {
        turnCount: 1,
        alertClass: "present",
        composerHasValue: true,
        composerDisabled: true,
      },
    },
    {
      focusedPane: "claude",
      left: { turnCount: 1, alertClass: "none" },
      right: {
        turnCount: 2,
        alertClass: "present",
        composerHasValue: false,
        composerDisabled: true,
      },
    },
  ];
  let index = 0;
  let waits = 0;
  const result = await pollTurnAcceptance(
    async () => samples[index++],
    initial,
    {
      now: () => 0,
      until: 25_000,
      wait: async () => {
        waits += 1;
      },
    },
  );

  assert.equal(result.outcome, "accepted");
  assert.equal(waits, 2);
});

test("live prompt submission distinguishes a transition from a persistent block", () => {
  assert.equal(
    submissionReadiness({
      status: "Checking",
      disabled: true,
      alerts: [],
      alertsExceeded: false,
    }),
    "transitioning",
  );
  assert.equal(
    submissionReadiness({
      status: null,
      disabled: true,
      alerts: [],
      alertsExceeded: false,
    }),
    "transitioning",
  );
  assert.equal(
    submissionReadiness({
      status: "Ready",
      disabled: false,
      alerts: [],
      alertsExceeded: false,
    }),
    "ready",
  );
  assert.equal(
    submissionReadiness({
      status: "Sign in required",
      disabled: true,
      alerts: [],
      alertsExceeded: false,
    }),
    "persistent",
  );
  assert.equal(
    submissionReadiness({
      status: "Ready",
      disabled: true,
      alerts: ["Harness failure"],
      alertsExceeded: false,
    }),
    "error",
  );
  assert.equal(turnIdsPresent(["pong", "cancel"], ["pong", "cancel"]), true);
  assert.equal(turnIdsPresent(["pong", "cancel"], ["pong"]), false);
  assert.equal(turnIdsPresent(["pong", "cancel"], ["pong", "fresh"]), false);
  assert.equal(turnIdsPresent([], ["pong"]), false);
});

test("a harness switch waits until a previous turn has settled", () => {
  assert.equal(
    turnSettled({ composerDisabled: true, cancelVisible: true }),
    false,
  );
  assert.equal(
    turnSettled({ composerDisabled: false, cancelVisible: false }),
    true,
  );
});

test("the exact terminal CDP expression evaluates assistant text with a literal newline separator", () => {
  const assistantMarkup = ["PONG", "SECOND"]
    .map(
      (value) =>
        '<article class="acp-message" data-message-role="assistant">' +
        '<div class="acp-markdown"><p>' +
        escapeMarkup(value) +
        "</p></div></article>",
    )
    .join("");
  const dom = new JSDOM(
    '<!doctype html><section class="agent-chat-pane agent-chat-pane--focused">' +
      '<section class="chat-surface" aria-label="Agent chat">' +
      '<div class="session-state">Ready</div>' +
      '<section class="acp-turn" data-turn-id="current">' +
      '<span data-turn-status aria-label="Completed"></span>' +
      '<span data-turn-reason="cancelledByUser"></span>' +
      assistantMarkup +
      "</section>" +
      '<form class="composer">' +
      '<textarea aria-label="Message agent"></textarea>' +
      "</form></section></section>",
  );
  dom.window.Element.prototype.getClientRects = function getClientRects() {
    return this.hasAttribute("data-hidden") ? [] : [{}];
  };

  const result = new Function(
    "document",
    `return ${terminalExpression(focusedChatSurfacePreamble("return null;"))};`,
  )(dom.window.document);

  assert.deepEqual(result.turns, [
    {
      id: "current",
      status: "Completed",
      reason: "cancelledByUser",
      assistantText: "PONG\nSECOND",
      assistantTextTruncated: false,
      tools: [],
      toolsExceeded: false,
      rollups: [],
      rollupsExceeded: false,
    },
  ]);
});

function terminalSurfaceState(turnBody, trailing = "") {
  const dom = new JSDOM(
    '<!doctype html><section class="agent-chat-pane agent-chat-pane--focused">' +
      '<section class="chat-surface" aria-label="Agent chat">' +
      '<div class="session-state">Ready</div>' +
      '<section class="acp-turn" data-turn-id="current">' +
      '<span data-turn-status aria-label="Completed"></span>' +
      turnBody +
      "</section>" +
      trailing +
      '<form class="composer">' +
      '<textarea aria-label="Message agent"></textarea>' +
      "</form></section></section>",
  );
  dom.window.Element.prototype.getClientRects = function getClientRects() {
    return this.hasAttribute("data-hidden") ? [] : [{}];
  };
  return new Function(
    "document",
    `return ${terminalExpression(focusedChatSurfacePreamble("return null;"))};`,
  )(dom.window.document);
}

function assistantMessage(blocks) {
  return (
    '<article class="acp-message" data-message-role="assistant">' +
    '<div class="acp-markdown">' +
    blocks.map((value) => "<p>" + escapeMarkup(value) + "</p>").join("") +
    "</div></article>"
  );
}

test("assistant text the message scan could not reach never passes as an exact reply", () => {
  const values = ["PONG", "", "", "", "", "", "", "", "EXTRA"];
  const state = terminalSurfaceState(
    values.map((value) => assistantMessage([value])).join(""),
  );
  assert.equal(state.turns[0].assistantText, "PONG");
  assert.equal(state.turns[0].assistantTextTruncated, true);
  assert.equal(
    turnOutcome(state, "current", false, /^PONG$/u)?.kind,
    "response-mismatch",
  );
});

test("assistant blocks past the block cap never pass as an exact reply", () => {
  const blocks = ["PONG"];
  while (blocks.length <= TERMINAL_LIMITS.blocks) blocks.push("");
  const state = terminalSurfaceState(assistantMessage(blocks));
  assert.equal(state.turns[0].assistantText, "PONG");
  assert.equal(state.turns[0].assistantTextTruncated, true);
  assert.equal(
    turnOutcome(state, "current", false, /^PONG$/u)?.kind,
    "response-mismatch",
  );
});

test("an alert the scan could not reach never reads as a quiet surface", () => {
  const state = terminalSurfaceState(
    assistantMessage(["PONG"]) +
      "<div>" +
      "<span></span>".repeat(2047) +
      "</div>",
    '<div role="alert">Harness failure</div>',
  );
  assert.equal(state.turnsExceeded, false);
  assert.deepEqual(state.alerts, []);
  assert.equal(state.alertsExceeded, true);
  assert.deepEqual(turnOutcome(state, "current", false, /^PONG$/u), {
    kind: "scan-exceeded",
  });
});

test("an ambiguous, half scanned, or truncated terminal read never completes a turn", () => {
  const state = (turns, extra) => ({
    composerDisabled: false,
    cancelVisible: false,
    alerts: [],
    turnsExceeded: false,
    alertsExceeded: false,
    turns,
    ...extra,
  });
  const reply = {
    id: "second",
    status: "Completed",
    assistantText: "PONG",
    assistantTextTruncated: false,
  };
  assert.deepEqual(
    turnOutcome(
      state([reply, { id: "second", status: null }]),
      "second",
      false,
      /^PONG$/u,
    ),
    { kind: "turn-ambiguous" },
  );
  assert.equal(
    turnOutcome(
      state([{ id: "second", status: null, assistantText: "PONG" }]),
      "second",
      false,
      /^PONG$/u,
    ),
    null,
  );
  assert.deepEqual(turnOutcome(state([reply]), "second", false, /^PONG$/u), {
    kind: "completed",
  });
  for (const flag of [true, undefined, "false"]) {
    assert.deepEqual(
      turnOutcome(
        state([reply], { alertsExceeded: flag }),
        "second",
        false,
        /^PONG$/u,
      ),
      { kind: "scan-exceeded" },
    );
    assert.equal(
      turnOutcome(
        state([{ ...reply, assistantTextTruncated: flag }]),
        "second",
        false,
        /^PONG$/u,
      )?.kind,
      "response-mismatch",
    );
  }
});

test("a current turn cannot complete from prior content or a later turn", () => {
  assert.deepEqual(
    turnOutcome(
      {
        composerDisabled: false,
        cancelVisible: false,
        alerts: [],
        turnsExceeded: false,
        alertsExceeded: false,
        turns: [
          {
            id: "second",
            status: "Completed",
            assistantText: "PONG",
            assistantTextTruncated: false,
          },
        ],
      },
      "second",
      false,
      /^PONG[.!]?$/u,
    ),
    { kind: "completed" },
  );
  assert.equal(
    turnOutcome(
      {
        composerDisabled: false,
        cancelVisible: false,
        alerts: [],
        turnsExceeded: false,
        alertsExceeded: false,
        surfaceText: "PONG",
        toolCards: 1,
        turns: [
          { id: "first", status: "Completed", assistantText: "PONG" },
          { id: "second", status: "Completed", assistantText: "" },
        ],
      },
      "second",
      false,
      /^PONG[.!]?$/u,
    )?.kind,
    "response-mismatch",
  );
  assert.equal(
    turnOutcome(
      {
        composerDisabled: true,
        cancelVisible: true,
        alerts: [],
        turnsExceeded: false,
        alertsExceeded: false,
        toolCards: 1,
        surfaceText: "PONG",
        turns: [
          { id: "first", status: "Completed" },
          { id: "second", status: "Running" },
          { id: "later", status: "Interrupted" },
        ],
      },
      "second",
      true,
    ),
    null,
  );
  assert.deepEqual(
    turnOutcome(
      {
        composerDisabled: false,
        cancelVisible: false,
        alerts: [],
        turnsExceeded: false,
        alertsExceeded: false,
        turns: [
          { id: "first", status: "Completed" },
          { id: "second", status: "Interrupted" },
        ],
      },
      "second",
      true,
    ),
    { kind: "interrupted" },
  );
  assert.deepEqual(
    turnOutcome(
      {
        composerDisabled: false,
        cancelVisible: false,
        alerts: [],
        turnsExceeded: false,
        alertsExceeded: false,
        turns: [
          {
            id: "second",
            status: "Cancelled",
            reason: "cancelledByUser",
          },
        ],
      },
      "second",
      true,
    ),
    { kind: "cancelled" },
  );
  assert.equal(
    turnOutcome(
      {
        composerDisabled: true,
        cancelVisible: true,
        alerts: [],
        turnsExceeded: false,
        alertsExceeded: false,
        turns: [
          {
            id: "second",
            status: "Cancelled",
            reason: "cancelledByUser",
          },
        ],
      },
      "second",
      true,
    ),
    null,
  );
  assert.deepEqual(
    turnOutcome(
      {
        composerDisabled: false,
        cancelVisible: false,
        alerts: [],
        turnsExceeded: false,
        alertsExceeded: false,
        turns: [
          {
            id: "second",
            status: "Cancelled",
            reason: "harnessExited",
          },
        ],
      },
      "second",
      true,
    ),
    { kind: "cancel-refused" },
  );
  assert.deepEqual(
    turnOutcome(
      {
        composerDisabled: false,
        cancelVisible: false,
        alerts: [],
        turnsExceeded: false,
        alertsExceeded: false,
        turns: [{ id: "second", status: "Cancelled", reason: null }],
      },
      "second",
      true,
    ),
    { kind: "cancel-refused" },
  );
  assert.deepEqual(
    turnOutcome(
      {
        composerDisabled: false,
        cancelVisible: false,
        alerts: [],
        turnsExceeded: false,
        alertsExceeded: false,
        turns: [
          { id: "second", status: "Interrupted", assistantText: "not PONG" },
        ],
      },
      "second",
      true,
      /^PONG[.!]?$/u,
    ),
    { kind: "interrupted" },
  );
  assert.equal(
    turnOutcome(
      {
        composerDisabled: false,
        cancelVisible: false,
        alerts: [],
        turnsExceeded: false,
        alertsExceeded: false,
        turns: [
          { id: "first", status: "Completed" },
          { id: "second", status: "Interrupted" },
        ],
      },
      "second",
      false,
    )?.kind,
    "error",
  );
  assert.equal(
    turnOutcome(
      {
        composerDisabled: false,
        cancelVisible: false,
        alerts: [],
        turnsExceeded: false,
        alertsExceeded: false,
        turns: [
          { id: "first", status: "Completed" },
          { id: "second", status: "Completed" },
        ],
      },
      "second",
      true,
    )?.kind,
    "cancel-refused",
  );
  assert.equal(
    turnOutcome(
      {
        composerDisabled: true,
        cancelVisible: true,
        alerts: [],
        turnsExceeded: false,
        alertsExceeded: false,
        turns: [
          { id: "first", status: "Completed" },
          { id: "second", status: "Interrupted" },
        ],
      },
      "second",
      true,
    ),
    null,
  );
  assert.equal(
    turnOutcome(
      {
        composerDisabled: false,
        cancelVisible: false,
        alerts: ["Harness failure"],
        turnsExceeded: false,
        alertsExceeded: false,
        turns: [
          { id: "first", status: "Completed" },
          { id: "second", status: "Interrupted" },
        ],
      },
      "second",
      true,
    )?.kind,
    "error",
  );
});

test("an ambiguous or half scanned transcript never reads as a completed turn", () => {
  assert.deepEqual(
    turnOutcome(
      {
        composerDisabled: false,
        cancelVisible: false,
        alerts: [],
        turnsExceeded: false,
        alertsExceeded: false,
        turns: [
          {
            id: "second",
            status: "Completed",
            assistantText: "PONG",
            assistantTextTruncated: false,
          },
          { id: "second", status: "Running" },
        ],
      },
      "second",
      false,
      /^PONG[.!]?$/u,
    ),
    { kind: "turn-ambiguous" },
  );
  for (const flag of [true, undefined, "false"]) {
    assert.deepEqual(
      turnOutcome(
        {
          composerDisabled: false,
          cancelVisible: false,
          alerts: [],
          turnsExceeded: flag,
          turns: [
            {
              id: "second",
              status: "Completed",
              assistantText: "PONG",
              assistantTextTruncated: false,
            },
          ],
        },
        "second",
        false,
        /^PONG[.!]?$/u,
      ),
      { kind: "scan-exceeded" },
    );
  }
  assert.deepEqual(
    turnOutcome(
      {
        composerDisabled: false,
        cancelVisible: false,
        alerts: [],
        turnsExceeded: false,
        alertsExceeded: false,
        turns: [
          {
            id: "second",
            status: "Completed",
            assistantText: "PONG",
            assistantTextTruncated: false,
          },
        ],
      },
      "second",
      false,
      /^PONG[.!]?$/u,
    ),
    { kind: "completed" },
  );
});

test("the runner stops on an ambiguous or half scanned terminal read", () => {
  const text = runnerSource();
  assert.match(text, /outcome\?\.kind === "scan-exceeded"/u);
  assert.match(text, /outcome\?\.kind === "turn-ambiguous"/u);
});

test("a half scanned alert region blocks submission instead of reading as ready", () => {
  const ready = {
    alerts: [],
    disabled: false,
    status: "Ready",
    alertsExceeded: false,
  };
  assert.equal(submissionReadiness(ready), "ready");
  for (const flag of [true, undefined, "false"])
    assert.equal(
      submissionReadiness({ ...ready, alertsExceeded: flag }),
      "error",
    );
  const text = runnerSource();
  assert.equal(
    /findAlerts\(surface,'\[role="alert"\]',false\)\.nodes/u.test(text),
    false,
  );
  const unpruned = /'\[role="alert"\]',false\)/gu;
  const pruned = /'\[role="alert"\]',false,"section\.acp-turn"\)/gu;
  assert.equal((text.match(pruned) ?? []).length, 5);
  const unprunedAt = [...text.matchAll(unpruned)].map((hit) => hit.index);
  assert.equal(unprunedAt.length, 1);
  const snapshotStart = text.indexOf("async function snapshot(");
  const snapshotEnd = text.indexOf("async function click(");
  assert.ok(snapshotStart >= 0 && snapshotEnd > snapshotStart);
  assert.ok(unprunedAt[0] > snapshotStart && unprunedAt[0] < snapshotEnd);
  assert.match(
    text,
    /alertClass:alerts\.exceeded!==false\|\|alerts\.nodes\.length>0\?"present":"none"/u,
  );
});

test("a half scanned harness picker alert region never reads as ready", () => {
  const ready = {
    triggerPresent: true,
    triggerDisabled: false,
    projectLoading: false,
    alertCount: 0,
    alertsExceeded: false,
  };
  assert.equal(harnessPickerReadiness(ready), "ready");
  for (const flag of [true, undefined, "false"])
    assert.equal(
      harnessPickerReadiness({ ...ready, alertsExceeded: flag }),
      "error",
    );
  assert.match(
    runnerSource(),
    /alertCount:alerts\.nodes\.length,alertsExceeded:alerts\.exceeded/u,
  );
});

test("the live journey cannot write completedAt after an unavailable harness or a blocked submission", () => {
  const source = readFileSync(
    new URL("./hr6-live-run.mjs", import.meta.url),
    "utf8",
  );

  assert.match(
    source,
    /if \(!codex\) throw new Error\("hr6-harness-unavailable-02-codex"\)/,
  );
  assert.match(
    source,
    /if \(!claude\) throw new Error\("hr6-harness-unavailable-05-claude"\)/,
  );
  assert.ok(source.includes("hr6-submission-${state}-${label}"));
  assert.ok(source.includes("hr6-timeout-${label}-readiness"));
});

test("the live journey keeps Claude and Codex in their recorded Chat panes", () => {
  const source = readFileSync(
    new URL("./hr6-live-run.mjs", import.meta.url),
    "utf8",
  );

  const claudeSelection = source.indexOf(
    'selectHarness(cdp, "claude", "Claude", "05-claude")',
  );
  const leftAssertion = source.indexOf(
    'assertRememberedHarness(cdp, "codex", "Codex", "06-codex")',
  );
  const leftFocus = source.indexOf(
    'focusRememberedChat(cdp, "codex", "06-codex")',
  );
  const continuity = source.indexOf(
    'assertRememberedTurnContinuity(cdp, "codex", codexTurnIds, "06-codex")',
  );
  const leftContinuation = source.indexOf('"06-codex-restart-pong"');
  const rightFocus = source.indexOf(
    'focusRememberedChat(cdp, "claude", "07-claude")',
  );
  const rightContinuation = source.indexOf('"07-claude-pong"', rightFocus);

  assert.ok(claudeSelection >= 0);
  assert.ok(leftAssertion > claudeSelection);
  assert.ok(leftFocus > leftAssertion);
  assert.ok(continuity > leftFocus);
  assert.ok(leftContinuation > continuity);
  assert.ok(rightFocus > leftContinuation);
  assert.ok(rightContinuation > rightFocus);
  assert.doesNotMatch(source, /__hr6Ipc|adapterProcesses|CommandLine/);
  assert.doesNotMatch(source, /consoleErrors/);
});

test("harness selection waits for project authority before opening its picker", () => {
  const source = readFileSync(
    new URL("./hr6-live-run.mjs", import.meta.url),
    "utf8",
  );
  const selectionStart = source.indexOf("async function selectHarness");
  const selectionEnd = source.indexOf("async function submit", selectionStart);
  const selection = source.slice(selectionStart, selectionEnd);
  const readiness = selection.indexOf(
    "await waitForHarnessPickerReadiness(cdp, chat, label)",
  );
  const open = selection.indexOf("await click(");
  const waitStart = source.indexOf(
    "async function waitForHarnessPickerReadiness",
  );
  const waitEnd = source.indexOf("async function selectHarness", waitStart);
  const wait = source.slice(waitStart, waitEnd);

  assert.ok(readiness >= 0);
  assert.ok(open > readiness);
  assert.ok(wait.includes("pollReadiness("));
  assert.ok(wait.includes("harnessPickerReadiness,"));
});

test("the Claude send path and failure evidence retain compact timing", () => {
  const source = readFileSync(
    new URL("./hr6-live-run.mjs", import.meta.url),
    "utf8",
  );
  const submitStart = source.indexOf("async function submit");
  const submitEnd = source.indexOf(
    "async function waitForSubmissionReadiness",
    submitStart,
  );
  const submit = source.slice(submitStart, submitEnd);

  for (const phase of ["fill", "composer", "send", "accepted"])
    assert.ok(
      submit.includes(`markClaudePhase(label, "${phase}")`),
      `missing Claude ${phase} marker`,
    );
  let previousMarker = -1;
  for (const [phase, action] of [
    ["fill", "`${label}-fill`"],
    ["composer", "`${label}-composer`"],
    ["send", "`${label}-send`"],
    [
      "accepted",
      "await waitForAcceptedTurn(cdp, chat, initialAcceptance, label)",
    ],
  ]) {
    const marker = submit.indexOf(`markClaudePhase(label, "${phase}")`);
    assert.ok(
      marker < submit.indexOf(action),
      `Claude ${phase} marker follows its action`,
    );
    assert.ok(
      marker > previousMarker,
      `Claude ${phase} marker is out of order`,
    );
    previousMarker = marker;
  }
  const catchStart = source.lastIndexOf("\n} catch (error) {");
  const catchEnd = source.indexOf("} finally {", catchStart);
  const catchBody = source.slice(catchStart, catchEnd);
  assert.ok(catchBody.includes("if (cdp) observations.cdp = cdp.stats"));
  assert.ok(
    catchBody.indexOf("if (cdp) observations.cdp = cdp.stats") <
      catchBody.indexOf('writeJson("observations", observations)'),
  );
});

test("each send has a bounded remembered-pane Turn acceptance check", () => {
  const source = readFileSync(
    new URL("./hr6-live-run.mjs", import.meta.url),
    "utf8",
  );
  const submitStart = source.indexOf("async function submit");
  const submitEnd = source.indexOf(
    "async function waitForSubmissionReadiness",
    submitStart,
  );
  const submit = source.slice(submitStart, submitEnd);
  const send = submit.indexOf("`${label}-send`");
  const accepted = submit.indexOf(
    "await waitForAcceptedTurn(cdp, chat, initialAcceptance, label)",
  );

  assert.ok(send >= 0);
  assert.ok(accepted > send);
  assert.ok(
    source.includes("turnAcceptanceSample(cdp, chat, acceptanceDeadline)"),
  );
  assert.ok(source.includes("setTimeout(resolve, 250)"));
  assert.ok(source.includes("operationDeadline(Math.min(deadline, until))"));
  assert.ok(source.includes("hr6-turn-acceptance-${result.outcome}-${label}"));
  const diagnosticStart = source.indexOf("async function turnAcceptanceSample");
  const diagnosticEnd = source.indexOf("async function rememberFocusedChat");
  const diagnostic = source.slice(diagnosticStart, diagnosticEnd);
  assert.ok(diagnostic.includes("newestTurnId"));
  assert.ok(diagnostic.includes("alertClass"));
  assert.doesNotMatch(diagnostic, /harness:/);
});

test("the native launcher keeps diagnostics at warning level and names its disposable profile", () => {
  const source = readFileSync(
    new URL("./hr6-live-run.ps1", import.meta.url),
    "utf8",
  );

  assert.match(source, /\$sageRustLog = "warn"/);
  assert.match(source, /\$env:RUST_LOG = \$sageRustLog/);
  assert.doesNotMatch(source, /\$env:RUST_LOG = "debug"/);
  assert.match(
    source,
    /\$disposableProfile = Resolve-DisposableChildPath \$output "profile"/,
  );
  assert.doesNotMatch(source, /\$profile = Join-Path \$output "profile"/);
  assert.match(
    source,
    /\$scratch = Resolve-DisposableChildPath \$output "scratch"/,
  );
  assert.doesNotMatch(source, /C:\\sage-hr6-scratch/);
  assert.match(source, /Assert-LoopbackPortAvailable 1420/);
  assert.match(source, /Assert-LoopbackPortAvailable \$Port/);
  assert.match(source, /if \(\$Port -eq 1420\)/);
  assert.match(source, /Remove-DisposableDirectory \$scratch \$output/);
  assert.match(
    source,
    /Remove-DisposableDirectory \$disposableProfile \$output/,
  );
  assert.match(source, /Start-Process -FilePath "node\.exe"/);
  assert.doesNotMatch(source, /cmd\.exe|npm\.cmd run dev/);
  assert.match(source, /\$null = \$vite\.Handle/);
  assert.match(source, /\$null = \$sage\.Handle/);
  assert.ok(
    source.indexOf("Get-ProcessExitReceipt $sage") <
      source.indexOf("taskkill.exe /pid $sage.Id"),
  );
  assert.ok(
    source.indexOf("Get-ProcessExitReceipt $vite") <
      source.indexOf("taskkill.exe /pid $vite.Id"),
  );
  assert.equal(source.includes("Remove-SageStdout"), false);
  assert.match(
    source,
    /-RedirectStandardOutput \$sink\.stdout -RedirectStandardError \$sink\.stderr/u,
  );
  const receipt = readFileSync(
    new URL("./hr6-process-receipt.ps1", import.meta.url),
    "utf8",
  );
  assert.match(receipt, /hasExited/);
  assert.match(receipt, /exitCode/);
  assert.match(receipt, /exitTime/);
  for (const [variable, prior] of [
    [
      "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
      "Webview2AdditionalBrowserArguments",
    ],
    ["SAGE_DISPOSABLE_PROFILE_ROOT", "SageDisposableProfileRoot"],
    ["SAGE_PROJECT_ROOT", "SageProjectRoot"],
    ["RUST_LOG", "RustLog"],
  ]) {
    assert.match(source, new RegExp(`\\$prior${prior} = \\$env:${variable}`));
    assert.match(source, new RegExp(`\\$env:${variable} = \\$prior${prior}`));
  }
});

test("Sage log validation refuses escapes, existing targets, and impure filters", () => {
  const result = spawnSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      fileURLToPath(new URL("./hr6-sage-log.test.ps1", import.meta.url)),
      "-ModulePath",
      fileURLToPath(new URL("./hr6-sage-log.ps1", import.meta.url)),
      "-DisposablePath",
      fileURLToPath(new URL("./hr6-disposable.ps1", import.meta.url)),
    ],
    { encoding: "utf8", timeout: 120000 },
  );
  assert.equal(result.error, undefined, `${result.error}`);
  assert.equal(result.signal, null);
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
  assert.match(result.stdout, /hr6-sage-log checks passed/);
});

test("the native launcher maps the OpenCode switch to an explicit flag value", () => {
  const result = spawnSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      fileURLToPath(new URL("./hr6-opencode-flag.test.ps1", import.meta.url)),
      "-LauncherPath",
      fileURLToPath(new URL("./hr6-live-run.ps1", import.meta.url)),
      "-OuterDeadlineMs",
      "120000",
    ],
    { encoding: "utf8", timeout: 120000 },
  );
  assert.equal(result.error, undefined, `${result.error}`);
  assert.equal(result.signal, null);
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
  assert.match(result.stdout, /hr6-opencode-flag checks passed/);
});

test("every PowerShell child this suite spawns carries a deadline", () => {
  const source = readFileSync(
    new URL("./hr6-live-run.node-test.mjs", import.meta.url),
    "utf8",
  );
  const calls = source.match(/const result = spawnSync\(/gu) ?? [];
  assert.ok(calls.length >= 5, `found ${calls.length} spawnSync calls`);
  const bounded =
    source.match(/\{\s*encoding:\s*"utf8",\s*timeout:\s*120000,?\s*\}/gu) ?? [];
  assert.equal(bounded.length, calls.length);
  assert.doesNotMatch(source, /\{\s*encoding:\s*"utf8",?\s*\}/u);
  assert.match(source, /"-OuterDeadlineMs",\s*"120000",/u);
  for (const value of source.match(/timeout:\s*(\d+)/gu) ?? []) {
    assert.ok(Number(value.replace(/timeout:\s*/u, "")) > 0, value);
  }
});

test("the native launcher delegates Sage diagnostics to the validated module", () => {
  const source = readFileSync(
    new URL("./hr6-live-run.ps1", import.meta.url),
    "utf8",
  );
  assert.match(source, /\[string\]\$SageLogDirectory/);
  assert.match(source, /\[string\]\$SageRustLog = "warn"/);
  assert.match(source, /\. \(Join-Path \$PSScriptRoot "hr6-sage-log\.ps1"\)/);
  assert.match(source, /Assert-SageRustLog \$SageRustLog/);
  assert.equal(source.includes("function Resolve-SageLogDirectory"), false);
  assert.match(
    source,
    /\$logDirectory = Resolve-SageLogDirectory \$output \$SageLogDirectory/,
  );
  assert.match(source, /\$sageOut = \$sink\.stdout/);
  assert.match(source, /\$sageErr = \$sink\.stderr/);
  assert.match(
    source,
    /-RedirectStandardOutput \$sageOut -RedirectStandardError \$sageErr/,
  );
  assert.ok(
    source.indexOf("$sageOut = $sink.stdout") <
      source.indexOf("if ($captureRequested) {"),
  );
  assert.ok(
    source.indexOf("if ($captureRequested) {") <
      source.indexOf("sage.stdout.log"),
  );
  const guards = source.match(
    /Assert-SageLogTargets \$output \$logDirectory/gu,
  );
  assert.equal(guards.length, 2);
  assert.ok(
    source.lastIndexOf("Assert-SageLogTargets $output $logDirectory") <
      source.indexOf("$sage = Start-Process"),
  );
  assert.equal(source.includes("vite.log"), false);
  assert.equal(source.includes("vite.stderr.log"), false);
});

test("launcher receipt retains an exited child process", () => {
  const result = spawnSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      fileURLToPath(new URL("./hr6-process-receipt.test.ps1", import.meta.url)),
      "-ModulePath",
      fileURLToPath(new URL("./hr6-process-receipt.ps1", import.meta.url)),
    ],
    { encoding: "utf8", timeout: 120000 },
  );

  assert.equal(result.error, undefined, `${result.error}`);
  assert.equal(result.signal, null);
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
});

test("discarded native streams never reach disk", () => {
  const result = spawnSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      fileURLToPath(new URL("./hr6-null-sink.test.ps1", import.meta.url)),
      "-ModulePath",
      fileURLToPath(new URL("./hr6-null-sink.ps1", import.meta.url)),
    ],
    { encoding: "utf8", timeout: 120000 },
  );

  assert.equal(result.error, undefined, `${result.error}`);
  assert.equal(result.signal, null);
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
});
test("disposable cleanup keeps parent and junction targets intact", () => {
  const result = spawnSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      fileURLToPath(new URL("./hr6-disposable.test.ps1", import.meta.url)),
      "-ModulePath",
      fileURLToPath(new URL("./hr6-disposable.ps1", import.meta.url)),
    ],
    { encoding: "utf8", timeout: 120000 },
  );

  assert.equal(result.error, undefined, `${result.error}`);
  assert.equal(result.signal, null);
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
});

const STALE_USAGE_SELECTOR =
  '.acp-usage,[aria-label="Usage"],[aria-label*="Context"]';
const USAGE_TEXT_LIMIT = 96;
const USAGE_NODE_LIMIT = 4;
const USAGE_SCAN_BUDGET = 2048;

test("poll yields run on a host timer so an occluded page still makes progress", () => {
  assert.equal(POLL_INTERVAL_MS, 50);
  assert.equal(pollDelay(1000, 900), 50);
  assert.equal(pollDelay(910, 900), 10);
  assert.equal(pollDelay(900, 900), 0);
  assert.equal(pollDelay(800, 900), 0);
  assert.equal(pollDelay(Number.NaN, 900), 0);
  assert.equal(pollDelay(1000, Number.NaN), 0);
  assert.equal(pollDelay(1000, 900, 5), 5);

  const text = runnerSource();
  assert.equal(/requestAnimationFrame/u.test(text), false);
  assert.equal((text.match(/await pollYield\(/gu) ?? []).length, 16);
  assert.match(text, /yieldFrame: \(\) => pollYield\(readinessDeadline\)/u);
  assert.match(
    text,
    /const wait = pollDelay\(Math\.min\(deadline, until\), Date\.now\(\)\);/u,
  );
  assert.match(text, /setTimeout\(resolve, wait\);/u);
});

test("a probe the page never answers fails closed instead of stalling", async () => {
  const never = () => new Promise(() => {});
  const answered = await boundedRead(() => Promise.resolve("state"), {
    expire: never,
  });
  assert.deepEqual(answered, { kind: "value", value: "state" });

  const stalled = await boundedRead(never, { expire: () => Promise.resolve() });
  assert.deepEqual(stalled, { kind: "unresponsive" });

  const failure = new Error("journey-cdp-deadline");
  const failed = await boundedRead(() => Promise.reject(failure), {
    expire: never,
  });
  assert.deepEqual(failed, { kind: "error", error: failure });

  const late = new Promise((resolve, reject) => {
    setTimeout(() => reject(new Error("journey-cdp-deadline")), 5);
  });
  assert.deepEqual(
    await boundedRead(() => late, { expire: () => Promise.resolve() }),
    {
      kind: "unresponsive",
    },
  );
  await new Promise((resolve) => {
    setTimeout(resolve, 25);
  });
});

test("the usage poll and the terminal wait name an unresponsive page", () => {
  const text = runnerSource();
  const slice = (from, to) => {
    const start = text.indexOf(from);
    const end = text.indexOf(to, start);
    assert.ok(start >= 0 && end > start);
    return text.slice(start, end);
  };
  const usage = slice(
    "async function assertReportedUsage(",
    "function markClaudePhase(",
  );
  assert.match(usage, /probeRead\(/u);
  assert.match(usage, /usageDeadline,\n\s*\);/u);
  assert.match(usage, /hr6-unresponsive-\$\{label\}-reported-usage/u);

  const terminalWait = slice(
    "async function submit(",
    "async function cancelRunningTool(",
  );
  assert.match(terminalWait, /probeRead\(/u);
  assert.match(terminalWait, /turnDeadline,\n\s*\);/u);
  assert.match(terminalWait, /hr6-unresponsive-\$\{label\}-terminal/u);
});

test("a frozen frame callback stalls a poll loop while a host timer lets it refuse", async () => {
  const waiting = {
    triggerPresent: true,
    triggerDisabled: true,
    projectLoading: true,
    alertCount: 0,
    alertsExceeded: false,
  };
  const frozen = {
    evaluate: (value) =>
      /requestAnimationFrame/u.test(value)
        ? new Promise(() => {})
        : Promise.resolve(waiting),
  };
  const probe = () => frozen.evaluate("probe");

  const stalled = await Promise.race([
    pollReadiness(probe, harnessPickerReadiness, {
      until: 50,
      now: () => 0,
      yieldFrame: () =>
        frozen.evaluate(
          "new Promise((resolve)=>requestAnimationFrame(()=>resolve(true)))",
        ),
    }).then((result) => result.status),
    new Promise((resolve) => {
      setTimeout(() => resolve("stalled"), 25);
    }),
  ]);
  assert.equal(stalled, "stalled");

  let clock = 0;
  const timed = await pollReadiness(probe, harnessPickerReadiness, {
    until: 100,
    now: () => clock,
    yieldFrame: () =>
      new Promise((resolve) => {
        setTimeout(() => {
          clock += pollDelay(100, clock);
          resolve();
        }, 0);
      }),
  });
  assert.equal(timed.status, "timeout");
  assert.equal(clock, 100);
});

test("probe expiry stays inside both the caller deadline and the CDP cap", () => {
  assert.equal(CDP_OPERATION_CAP_MS, 10_000);
  assert.equal(PROBE_MARGIN_MS, 250);
  assert.equal(probeExpiry(15_000, 0), 9_750);
  assert.equal(probeExpiry(5_000, 0), 4_750);
  assert.equal(probeExpiry(100, 0), 0);
  assert.equal(probeExpiry(120_000, 1_000), 10_750);
  assert.equal(probeExpiry(Number.NaN, 5), 5);
  assert.ok(probeExpiry(45_000, 0) < CDP_OPERATION_CAP_MS);
});

test("a hung read is named unresponsive before a CDP cap reports a generic deadline", async () => {
  const cap = 60;
  const margin = 20;
  const expiry = probeExpiry(200, 0, cap, margin);
  assert.equal(expiry, 40);
  assert.ok(expiry < cap);

  const timeline = [];
  let reportCap = null;
  let reportExpiry = null;
  const hung = () =>
    new Promise((resolve, reject) => {
      reportCap = () => {
        timeline.push("cap");
        reject(new Error("journey-cdp-deadline"));
      };
    });
  const read = boundedRead(hung, {
    expire: () =>
      new Promise((resolve) => {
        reportExpiry = () => {
          timeline.push("expiry");
          resolve();
        };
      }),
  });
  assert.equal(typeof reportCap, "function");
  assert.equal(typeof reportExpiry, "function");

  reportExpiry();
  assert.deepEqual(await read, { kind: "unresponsive" });
  assert.deepEqual(timeline, ["expiry"]);

  reportCap();
  await Promise.resolve();
  assert.deepEqual(await read, { kind: "unresponsive" });
  assert.deepEqual(timeline, ["expiry", "cap"]);
});

test("the opt-in OpenCode PONG step reuses the shared helpers and fails closed", () => {
  const source = runnerSource();

  assert.match(
    source,
    /import \{\s*harnessSelectionOutcome,\s*includeOpenCodeRequested,\s*\} from "\.\/hr6-harness-selection\.mjs";/u,
  );
  assert.match(
    source,
    /const includeOpenCode = includeOpenCodeRequested\(values\);/u,
  );
  assert.match(source, /if \(includeOpenCode\) \{/u);
  assert.equal((source.match(/if \(includeOpenCode\) \{/gu) ?? []).length, 1);
  assert.match(
    source,
    /selectHarness\(\s*cdp,\s*"opencode",\s*"OpenCode",\s*"13-opencode",\s*selection,?\s*\)/u,
  );
  assert.match(
    source,
    /const outcome = harnessSelectionOutcome\(selection\.state\);/u,
  );
  assert.ok(source.includes("hr6-${outcome.kind}-13-opencode-pong"));
  assert.match(
    source,
    /"13-opencode-pong",\s*false,\s*\/\^PONG\[\.!\]\?\$\/u,/u,
  );
  assert.equal(
    (source.match(/"Reply with the single word PONG\."/gu) ?? []).length,
    4,
  );
  assert.equal((source.match(/"13-opencode-pong"/gu) ?? []).length, 1);
  const cancelStep = source.indexOf('"12-claude-long-tool-cancel"');
  const guard = source.indexOf("if (includeOpenCode) {");
  const completed = source.indexOf("observations.completedAt");
  assert.ok(cancelStep > 0);
  assert.ok(cancelStep < guard);
  assert.ok(guard < completed);
});

test("harness selection records every non-selected exit for classification", () => {
  const source = runnerSource();

  assert.match(
    source,
    /async function selectHarness\(cdp, chat, fragment, label, selection = null\)/u,
  );
  assert.ok(source.includes("if (selection) selection.state = result;"));
  assert.ok(
    source.includes('if (selection) selection.state = "readiness-timeout";'),
  );
  assert.ok(source.includes('if (selection) selection.state = "alternative";'));
  assert.equal((source.match(/selection\.state = /gu) ?? []).length, 3);
});

test("the native launcher keeps the OpenCode step opt-in through an explicit switch", () => {
  const source = readFileSync(
    new URL("./hr6-live-run.ps1", import.meta.url),
    "utf8",
  );

  assert.match(source, /\[switch\]\$IncludeOpenCode/u);
  assert.match(
    source,
    /\$openCodeFlag = if \(\$IncludeOpenCode\) \{ "true" \} else \{ "false" \}/u,
  );
  assert.match(source, /--include-opencode \$openCodeFlag/u);
  assert.doesNotMatch(source, /\$includeOpenCode = /u);
  assert.ok(
    source.indexOf("$openCodeFlag = if ($IncludeOpenCode)") <
      source.indexOf("--include-opencode $openCodeFlag"),
  );
});

function runnerSource() {
  return readFileSync(new URL("./hr6-live-run.mjs", import.meta.url), "utf8");
}

const LONG_TOOL_SOURCE =
  "const LONG_TOOL_COMMAND = `$hr6Deadline=(Get-Date).AddSeconds(120); while (-not (Test-Path -LiteralPath ${LONG_TOOL_RELEASE_FILE}) -and (Get-Date) -lt $hr6Deadline) { Start-Sleep -Seconds 2 }`;";
const CANCEL_BOUND_MARGIN_MS = 20_000;

function composerContextSource() {
  return readFileSync(
    new URL("../src/features/chat/ComposerUsageContext.tsx", import.meta.url),
    "utf8",
  );
}

function composerContextProjection(source) {
  const match = source.match(
    /const COMPOSER_CONTEXT_PROJECTION = `([\s\S]*?)`;/u,
  );
  assert.ok(match, "runner has no composer-context projection");
  return new Function(`return (${match[1]});`)();
}

function reportedUsageOutcomeFrom(source) {
  const start = source.indexOf("function reportedUsageOutcome(");
  const end = source.indexOf("async function assertReportedUsage(", start);
  assert.ok(start >= 0 && end > start, "runner has no reported-usage outcome");
  return new Function(
    `${source.slice(start, end)} return reportedUsageOutcome;`,
  )();
}

function surfaceWith(markup) {
  const dom = new JSDOM(
    `<!doctype html><section class="chat-surface" aria-label="Agent chat"><div class="composer">${markup}</div></section>`,
  );
  return dom.window.document.querySelector("section.chat-surface");
}

function compactTokenCount(tokens) {
  if (tokens >= 1_000 && tokens % 1_000 === 0) return `${tokens / 1_000}K`;
  return tokens.toLocaleString("en-US");
}

function meteredContextMarkup(total, capacity, overrides = {}) {
  const progress = Math.min(Math.max(total, 0), capacity);
  const formattedTotal = total.toLocaleString("en-US");
  const formattedCapacity = capacity.toLocaleString("en-US");
  const label = overrides.label ?? `${compactTokenCount(total)} context`;
  const valueMax = overrides.valueMax ?? String(capacity);
  const valueText =
    overrides.valueText ??
    `${formattedTotal} / ${formattedCapacity} tokens used`;
  const valueNow = overrides.omitValueNow
    ? ""
    : ` aria-valuenow="${overrides.valueNow ?? String(progress)}"`;
  return `<p class="composer__context" aria-label="Composer context"><span class="composer__context-item composer__context-item--usage"><span class="composer__context-usage-label">${label}</span><span class="composer__context-usage" title="${formattedTotal} used / ${formattedCapacity} total tokens" role="progressbar" aria-label="${formattedTotal} used / ${formattedCapacity} total tokens" tabindex="0" aria-valuemin="0" aria-valuemax="${valueMax}"${valueNow} aria-valuetext="${valueText}"><span class="composer__context-usage-fill" aria-hidden="true"></span></span></span></p>`;
}

function regionMarkup(inner) {
  return `<p class="composer__context" aria-label="Composer context">${inner}</p>`;
}

function crowdedContextMarkup(groups, perGroup) {
  const filler = '<span class="composer__context-item"></span>'.repeat(
    perGroup,
  );
  const group = `<span class="composer__context-group">${filler}</span>`;
  return meteredContextMarkup(14_260, 258_400).replace(
    "</p>",
    `${group.repeat(groups)}</p>`,
  );
}

test("the runner reads the rendered composer context the chat surface actually renders", () => {
  const component = composerContextSource();
  const source = runnerSource();

  for (const token of [
    'aria-label="Composer context"',
    'className="composer__context"',
    'className="composer__context-usage-label"',
    "aria-label={`${formattedTotal} used / ${formattedCapacity} total tokens`}",
    'role="progressbar"',
    "Math.min(Math.max(total, 0), capacity)",
  ])
    assert.ok(component.includes(token), `ComposerContext lost ${token}`);

  assert.ok(
    source.includes('p.composer__context[aria-label="Composer context"]'),
  );
  assert.ok(source.includes('.composer__context-usage[role="progressbar"]'));
  assert.ok(!source.includes(STALE_USAGE_SELECTOR));
  assert.ok(!source.includes(".acp-usage"));
});

test("the composer-context projection reads the metered indicator the stale selector cannot see", () => {
  const source = runnerSource();
  const project = composerContextProjection(source);
  const surface = surfaceWith(meteredContextMarkup(14_260, 258_400));

  assert.equal(surface.querySelectorAll(STALE_USAGE_SELECTOR).length, 0);
  assert.deepEqual(project(surface), {
    regionCount: 1,
    regionsExceeded: false,
    scanExceeded: false,
    rendered: true,
    truncated: false,
    labelCount: 1,
    label: "14,260 context",
    summary: "14,260 context",
    meterCount: 1,
    valueNow: "14260",
    valueMax: "258400",
    valueText: "14,260 / 258,400 tokens used",
  });
});

test("the composer-context projection stays chat scoped and separates a static label from a reported count", () => {
  const project = composerContextProjection(runnerSource());
  const empty = project(surfaceWith(regionMarkup("")));

  assert.equal(empty.regionCount, 1);
  assert.equal(empty.rendered, false);
  assert.equal(empty.label, null);
  assert.equal(empty.valueNow, null);
  assert.equal(project(surfaceWith("")).regionCount, 0);
  assert.equal(
    project(
      surfaceWith(
        `${meteredContextMarkup(7, 1_000_000)}${meteredContextMarkup(
          209,
          1_000_000,
        )}`,
      ),
    ).regionCount,
    2,
  );
});

test("the composer-context projection bounds every string and node count it records", () => {
  const project = composerContextProjection(runnerSource());
  const flood = "9".repeat(USAGE_TEXT_LIMIT * 4);
  const oversized = project(
    surfaceWith(
      meteredContextMarkup(14_260, 258_400, {
        label: `${flood} context`,
        valueText: `${flood} / 258,400 tokens used`,
      }),
    ),
  );

  assert.equal(oversized.truncated, true);
  assert.equal(oversized.label.length, USAGE_TEXT_LIMIT);
  assert.equal(oversized.valueText.length, USAGE_TEXT_LIMIT);
  assert.ok(oversized.summary.length <= USAGE_TEXT_LIMIT);

  const crowded = project(
    surfaceWith(regionMarkup("").repeat(USAGE_NODE_LIMIT * 3)),
  );

  assert.equal(crowded.regionCount, USAGE_NODE_LIMIT);
  assert.equal(crowded.regionsExceeded, true);
});

test("a reported usage assertion accepts only a truthful rendered token count", () => {
  const source = runnerSource();
  const outcome = reportedUsageOutcomeFrom(source);
  const project = composerContextProjection(source);
  const read = (markup) => outcome(project(surfaceWith(markup)));

  assert.equal(read(meteredContextMarkup(14_260, 258_400)), "usage");
  assert.equal(read(meteredContextMarkup(7, 1_000_000)), "usage");
  assert.equal(read(meteredContextMarkup(14_000, 258_400)), "usage");
  assert.equal(read(meteredContextMarkup(300_000, 258_400)), "usage");
  assert.equal(
    read(
      regionMarkup(
        '<span class="composer__context-item">Reported usage · 209 tokens</span>',
      ),
    ),
    "shape",
  );
  assert.equal(outcome(project(surfaceWith(""))), "unmetered");
  assert.equal(read(regionMarkup("")), "absent");
  assert.equal(
    read(regionMarkup('<span class="composer__context-item">Read-only</span>')),
    "shape",
  );
});

test("an unmetered composer is reported distinctly and never passes the live assertion", () => {
  const source = runnerSource();
  const outcome = reportedUsageOutcomeFrom(source);
  const project = composerContextProjection(source);
  assert.equal(outcome(project(surfaceWith(""))), "unmetered");

  const assertion = source.slice(
    source.indexOf("async function assertReportedUsage("),
    source.indexOf(
      "function markClaudePhase(",
      source.indexOf("async function assertReportedUsage("),
    ),
  );
  assert.match(assertion, /if \(outcome === "usage"\) return;/u);
  assert.match(
    assertion,
    /throw new Error\(`hr6-reported-usage-\$\{outcome\}-\$\{label\}`\);/u,
  );
});

test("a reported usage assertion rejects a meter that contradicts its displayed count", () => {
  const source = runnerSource();
  const outcome = reportedUsageOutcomeFrom(source);
  const project = composerContextProjection(source);
  const read = (markup) => outcome(project(surfaceWith(markup)));

  assert.equal(
    read(meteredContextMarkup(14_260, 258_400, { omitValueNow: true })),
    "meter",
  );
  assert.equal(
    read(meteredContextMarkup(14_260, 258_400, { valueNow: "0" })),
    "meter",
  );
  assert.equal(
    read(meteredContextMarkup(14_260, 258_400, { valueNow: "" })),
    "meter",
  );
  assert.equal(
    read(meteredContextMarkup(14_260, 258_400, { valueMax: "0" })),
    "meter",
  );
  assert.equal(
    read(meteredContextMarkup(14_260, 258_400, { valueMax: "999" })),
    "meter",
  );
  assert.equal(
    read(
      meteredContextMarkup(14_260, 258_400, {
        valueText: "9,999 / 258,400 tokens used",
      }),
    ),
    "meter",
  );
  assert.equal(
    read(meteredContextMarkup(14_260, 258_400, { label: "9,999 context" })),
    "label",
  );
  assert.equal(
    read(meteredContextMarkup(14_260, 258_400, { valueText: "14,260 tokens" })),
    "meter",
  );
  assert.equal(
    read(meteredContextMarkup(14_001, 258_400, { label: "14K context" })),
    "label",
  );
  assert.equal(
    read(meteredContextMarkup(14_000, 258_400, { label: "14,000 context" })),
    "label",
  );
  assert.equal(
    read(meteredContextMarkup(14_260, 258_400, { label: "Checking context" })),
    "label",
  );
  assert.equal(
    read(
      regionMarkup(
        '<span class="composer__context-usage-label">14,260 context</span>',
      ),
    ),
    "shape",
  );
});

test("the live journey asserts rendered usage on the Codex and Claude pong turns", () => {
  const source = runnerSource();
  const codexPong = source.indexOf('"03-codex-pong",');
  const codexUsage = source.indexOf(
    'assertReportedUsage(cdp, "codex", "03-codex-pong")',
  );
  const claudePong = source.indexOf('"07-claude-pong",');
  const claudeUsage = source.indexOf(
    'assertReportedUsage(cdp, "claude", "07-claude-pong")',
  );

  assert.ok(codexPong >= 0);
  assert.ok(codexUsage > codexPong);
  assert.ok(claudePong > codexUsage);
  assert.ok(claudeUsage > claudePong);
  assert.ok(source.includes("hr6-reported-usage-${outcome}-${label}"));
  assert.ok(
    source.includes(
      'focusedRememberedChatSurfacePreamble(chat, "return null;")',
    ),
  );
  assert.doesNotMatch(source, /contextWindow|258_400|totalTokens/u);
});

function countingDom(html) {
  const dom = new JSDOM(html);
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
    return originalAll.call(this, selector);
  };
  dom.window.Element.prototype.querySelector = function (selector) {
    bump(enumerated, selector);
    return originalAll.call(this, selector)[0] ?? null;
  };
  return {
    dom,
    enumerated: (selector) => enumerated.get(selector) ?? 0,
    visits,
  };
}

const TRANSCRIPT_SELECTORS = [
  "section.acp-turn",
  ".acp-tool-card",
  '[role="alert"]',
  'section.chat-surface[aria-label="Agent chat"]',
  "section.agent-chat-pane.agent-chat-pane--focused",
];

function acceptanceMarkup(turnCount, cardCount) {
  const turns = [];
  for (let index = 0; index < turnCount; index += 1) {
    const cards = [];
    for (let slot = 0; slot < cardCount; slot += 1)
      cards.push(
        '<article class="acp-activity-card acp-tool-card" data-activity-state="completed" data-activity-kind="read" data-acp-item-id="item-' +
          index +
          "-" +
          slot +
          '"><strong>Read alpha.txt</strong></article>',
      );
    turns.push(
      '<section class="acp-turn" data-turn-id="turn-' +
        index +
        '"><span data-turn-status aria-label="Completed"></span>' +
        cards.join("") +
        "</section>",
    );
  }
  return (
    '<!doctype html><section class="agent-chat-pane agent-chat-pane--focused">' +
    '<section class="chat-surface" aria-label="Agent chat">' +
    '<div class="session-state">Ready</div>' +
    turns.join("") +
    '<form class="composer"><textarea aria-label="Message agent"></textarea></form>' +
    "</section></section>"
  );
}

function acceptanceSampler(run) {
  const source = runnerSource();
  const start = source.indexOf("async function turnAcceptanceSample");
  const end = source.indexOf("async function waitForAcceptedTurn", start);
  return new Function(
    "evaluate",
    "expression",
    "deadline",
    "boundedFirstProjection",
    "boundedIdentityProjection",
    "boundedMatchProjection",
    "boundedNodeTextProjection",
    "boundedScopeProjection",
    "boundedTurnsProjection",
    `${source.slice(start, end)}return turnAcceptanceSample;`,
  )(
    run,
    (value) => value,
    Number.MAX_SAFE_INTEGER,
    boundedFirstProjection,
    boundedIdentityProjection,
    boundedMatchProjection,
    boundedNodeTextProjection,
    boundedScopeProjection,
    boundedTurnsProjection,
  );
}

async function acceptanceScanWork(turnCount, cardCount) {
  const counted = countingDom(acceptanceMarkup(turnCount, cardCount));
  const pane = counted.dom.window.document.body.firstElementChild;
  counted.dom.window.__hr6Chats = { claude: pane, codex: null };
  const sampler = acceptanceSampler(async (_cdp, value) =>
    new Function("window", "document", value)(
      counted.dom.window,
      counted.dom.window.document,
    ),
  );
  const sample = await sampler({}, "claude", Number.MAX_SAFE_INTEGER);
  return { counted, sample };
}

test("the turn acceptance sample reads its panes without enumerating the transcript", async () => {
  const small = await acceptanceScanWork(60, 40);
  const large = await acceptanceScanWork(400, 40);

  for (const work of [small, large]) {
    assert.equal(work.sample.focusedPane, "claude");
    assert.equal(work.sample.right.present, true);
    assert.equal(work.sample.right.turnCount, null);
    assert.equal(work.sample.right.turnsExceeded, true);
    assert.equal(
      work.sample.right.newestTurnId,
      "turn-" + (work === small ? 59 : 399),
    );
    assert.equal(work.sample.right.statusClass, "ready");
    assert.equal(work.sample.right.alertClass, "none");
    for (const selector of TRANSCRIPT_SELECTORS)
      assert.equal(work.counted.enumerated(selector), 0);
  }

  assert.ok(small.counted.visits.matches <= 3 * 60);
  assert.ok(large.counted.visits.matches <= 3 * 400);
  assert.ok(
    large.counted.visits.matches - small.counted.visits.matches <=
      3 * (400 - 60),
  );
});

test("the cancellation path captures its streaming snapshot after the atomic cancel click", () => {
  const source = runnerSource();
  const start = source.indexOf("async function submit(");
  const end = source.indexOf("async function cancelRunningTool(", start);
  assert.ok(start >= 0 && end > start);
  const body = source.slice(start, end);
  const cancelCall = body.indexOf(
    "observedRunningTools = await cancelRunningTool(",
  );
  const streaming = body.indexOf("${label}-streaming");

  assert.ok(cancelCall > 0);
  assert.ok(streaming > cancelCall);
  assert.ok(
    body.includes(
      "await snapshot(cdp, chat, `${label}-streaming`, currentTurnId);",
    ),
  );
  assert.equal(
    body.includes("await snapshot(cdp, chat, `${label}-streaming`);"),
    false,
  );
});

function snapshotProjection(turnId) {
  const text = runnerSource();
  const start = text.indexOf("async function snapshot(");
  assert.ok(start > 0, "runner has no snapshot");
  const open = text.indexOf("expression(`", start) + "expression(`".length;
  const close = text.indexOf("`),", open);
  assert.ok(close > open, "runner snapshot has no projection");
  const composer = text.match(
    /const COMPOSER_CONTEXT_PROJECTION = `([\s\S]*?)`;/u,
  );
  assert.ok(composer, "runner has no composer-context projection");
  const build = new Function(
    "focusedRememberedChatSurfacePreamble",
    "boundedFirstProjection",
    "boundedScopeProjection",
    "boundedTurnsProjection",
    "boundedMatchProjection",
    "toolCardsProjection",
    "boundedTextProjection",
    "boundedNodeTextProjection",
    "boundedHeadProjection",
    "boundedScalarProjection",
    "boundedIdentityProjection",
    "TERMINAL_LIMITS",
    "COMPOSER_CONTEXT_PROJECTION",
    "chat",
    "turnId",
    `return \`${text.slice(open, close)}\`;`,
  );
  return `(()=>{${build(
    () => "const surface=document.body.firstElementChild;",
    boundedFirstProjection,
    boundedScopeProjection,
    boundedTurnsProjection,
    boundedMatchProjection,
    toolCardsProjection,
    boundedTextProjection,
    boundedNodeTextProjection,
    boundedHeadProjection,
    boundedScalarProjection,
    boundedIdentityProjection,
    TERMINAL_LIMITS,
    composer[1],
    "claude",
    turnId,
  )}})()`;
}

function snapshotTurn(id, cardId, state) {
  return `<section class="acp-turn" data-turn-id="${id}"><span data-turn-status aria-label="Running"></span><article class="acp-activity-card acp-tool-card" data-activity-kind="read" data-activity-state="${state}" data-acp-item-id="${cardId}"><strong>Read alpha.txt</strong></article></section>`;
}

function snapshotDom(title = "Sage") {
  const dom = new JSDOM(
    `<!doctype html><html><head><title>${title}</title></head><body><section class="chat-surface" aria-label="Agent chat">${snapshotTurn("earlier", "item-earlier", "completed")}${snapshotTurn("cancel", "item-hot", "inProgress")}<form class="composer"><textarea aria-label="Message agent"></textarea></form></section></body></html>`,
  );
  return dom;
}

function readSnapshot(dom, turnId) {
  return new Function("document", `return ${snapshotProjection(turnId)};`)(
    dom.window.document,
  );
}

test("a scoped snapshot projects only the turn it was asked about", () => {
  const dom = snapshotDom();
  const scoped = readSnapshot(dom, "cancel");
  assert.equal(scoped.turnScope, "cancel");
  assert.equal(scoped.turnScopeResolved, true);
  assert.deepEqual(
    scoped.turns.map((x) => x.id),
    ["cancel"],
  );
  assert.equal(scoped.toolCards, 1);
  assert.deepEqual(
    scoped.toolStates.map((x) => x.id),
    ["item-hot"],
  );
});

test("an unscoped snapshot still reports the whole bounded surface", () => {
  const whole = readSnapshot(snapshotDom(), null);
  assert.equal(whole.turnScope, null);
  assert.equal(whole.turnScopeResolved, null);
  assert.deepEqual(
    whole.turns.map((x) => x.id),
    ["earlier", "cancel"],
  );
  assert.equal(whole.toolCards, 2);
});

test("a snapshot scope that cannot be resolved says so and borrows nothing", () => {
  const lost = readSnapshot(snapshotDom(), "missing");
  assert.equal(lost.turnScope, "missing");
  assert.equal(lost.turnScopeResolved, false);
  assert.deepEqual(lost.turns, []);
  assert.equal(lost.toolCards, 0);
  assert.deepEqual(lost.toolStates, []);
});

test("a snapshot never carries a raw document title or turn id across CDP", () => {
  const title = "t".repeat(TERMINAL_LIMITS.label * 3);
  const wide = readSnapshot(snapshotDom(title), null);
  assert.equal(
    wide.title.length,
    TERMINAL_LIMITS.label + TERMINAL_LIMITS.marker.length,
  );
  assert.ok(wide.title.endsWith(TERMINAL_LIMITS.marker));
  const longId = "z".repeat(TERMINAL_LIMITS.identity + 1);
  const dom = new JSDOM(
    `<!doctype html><html><head><title>Sage</title></head><body><section class="chat-surface" aria-label="Agent chat">${snapshotTurn(longId, "item-hot", "inProgress")}<form class="composer"><textarea aria-label="Message agent"></textarea></form></section></body></html>`,
  );
  const seen = new Function("document", `return ${snapshotProjection(null)};`)(
    dom.window.document,
  );
  assert.deepEqual(
    seen.turns.map((x) => x.id),
    [null],
  );
});

function toolTerminalDeadlineFrom(source) {
  const start = source.indexOf("function toolTerminalDeadline(");
  const end = source.indexOf("\n}", start);
  assert.ok(start >= 0 && end > start, "runner has no tool terminal deadline");
  return new Function(
    `${source.slice(start, end)}\n} return toolTerminalDeadline;`,
  )();
}

test("the composer-context finder reports overflow only past its node cap", () => {
  const project = composerContextProjection(runnerSource());
  const exact = project(surfaceWith(regionMarkup("").repeat(USAGE_NODE_LIMIT)));
  assert.equal(exact.regionCount, USAGE_NODE_LIMIT);
  assert.equal(exact.regionsExceeded, false);
  const under = project(
    surfaceWith(regionMarkup("").repeat(USAGE_NODE_LIMIT - 1)),
  );
  assert.equal(under.regionCount, USAGE_NODE_LIMIT - 1);
  assert.equal(under.regionsExceeded, false);
  const over = project(
    surfaceWith(regionMarkup("").repeat(USAGE_NODE_LIMIT + 1)),
  );
  assert.equal(over.regionCount, USAGE_NODE_LIMIT);
  assert.equal(over.regionsExceeded, true);
});

test("a reported usage assertion refuses a meter read from a half scanned region", () => {
  const source = runnerSource();
  const outcome = reportedUsageOutcomeFrom(source);
  const project = composerContextProjection(source);
  const scanned = project(surfaceWith(crowdedContextMarkup(4, 8)));
  const crowded = surfaceWith(crowdedContextMarkup(64, 64));
  const unscanned = project(crowded);

  assert.ok(
    crowded.querySelectorAll("p.composer__context *").length >
      USAGE_SCAN_BUDGET,
  );
  assert.equal(unscanned.labelCount, 1);
  assert.equal(unscanned.meterCount, 1);
  assert.equal(unscanned.label, "14,260 context");
  assert.equal(unscanned.valueText, "14,260 / 258,400 tokens used");
  assert.equal(outcome(scanned), "usage");
  assert.equal(outcome(unscanned), "scan");
  assert.equal(scanned.scanExceeded, false);
  assert.equal(unscanned.scanExceeded, true);
  assert.equal(outcome({ ...unscanned, scanExceeded: undefined }), "scan");
});

test("a reported usage assertion refuses a usage read with no truncation flag", () => {
  const source = runnerSource();
  const outcome = reportedUsageOutcomeFrom(source);
  const project = composerContextProjection(source);
  const value = project(surfaceWith(meteredContextMarkup(14_260, 258_400)));
  const { truncated, ...omitted } = value;

  assert.equal(truncated, false);
  assert.equal(outcome(value), "usage");
  assert.equal(outcome({ ...value, truncated: undefined }), "oversized");
  assert.equal(outcome(omitted), "oversized");
  assert.equal(outcome({ ...value, truncated: "false" }), "oversized");
});

test("a reported usage assertion refuses a usage read with no region scan flag", () => {
  const source = runnerSource();
  const outcome = reportedUsageOutcomeFrom(source);
  const project = composerContextProjection(source);
  const value = project(surfaceWith(meteredContextMarkup(14_260, 258_400)));
  const { regionsExceeded, ...omitted } = value;

  assert.equal(regionsExceeded, false);
  assert.equal(typeof project(surfaceWith("")).regionsExceeded, "boolean");
  assert.equal(outcome(value), "usage");
  assert.equal(outcome({ ...value, regionsExceeded: undefined }), "region");
  assert.equal(outcome(omitted), "region");
  assert.equal(outcome({ ...value, regionsExceeded: "false" }), "region");
});

test("the composer-context projection caps oversized text before it trims it", () => {
  const project = composerContextProjection(runnerSource());
  const flood = "9".repeat(USAGE_TEXT_LIMIT * 60);
  const surface = surfaceWith(
    meteredContextMarkup(14_260, 258_400, {
      label: flood,
      valueText: flood,
    }),
  );
  const trim = String.prototype.trim;
  let widest = 0;
  String.prototype.trim = function bounded() {
    widest = Math.max(widest, this.length);
    return trim.call(this);
  };
  let read;
  try {
    read = project(surface);
  } finally {
    String.prototype.trim = trim;
  }
  assert.ok(widest > 0);
  assert.ok(widest <= USAGE_TEXT_LIMIT * 4, `trimmed ${widest} characters`);
  assert.equal(read.truncated, true);
  assert.equal(read.label.length, USAGE_TEXT_LIMIT);
  assert.equal(read.valueText.length, USAGE_TEXT_LIMIT);
  assert.ok(read.summary.length <= USAGE_TEXT_LIMIT);
});

test("a late settling turn keeps its whole grace without outliving the run", () => {
  const deadlineFor = toolTerminalDeadlineFrom(runnerSource());
  const grace = 8000;
  const action = 45_000;
  const hard = 600_000;
  assert.equal(deadlineFor(action, hard, null, grace), action);
  assert.equal(deadlineFor(action, hard, undefined, grace), action);
  assert.equal(deadlineFor(action, hard, 1000, grace), action);
  assert.equal(
    deadlineFor(action, hard, action - 1, grace),
    action - 1 + grace,
  );
  assert.equal(deadlineFor(action, hard, action, grace), action + grace);
  assert.equal(deadlineFor(action, action + 2, action, grace), action + 2);
  assert.ok(deadlineFor(action, hard, action, grace) <= hard);
});

function cancelWaitSimulation(options) {
  const deadlineFor = toolTerminalDeadlineFrom(runnerSource());
  const state = (now) => ({
    alerts: [],
    composerDisabled: now < options.settledAt,
    cancelVisible: now < options.settledAt,
    turnsExceeded: false,
    alertsExceeded: false,
    turns: [
      {
        id: "cancel",
        status: now < options.settledAt ? "Running" : "Cancelled",
        reason: "cancelledByUser",
        toolsExceeded: false,
        tools: [
          {
            id: "item-hot",
            kind: "read",
            label: "Read alpha.txt",
            state: now < options.cardTerminalAt ? "inProgress" : "cancelled",
          },
        ],
      },
    ],
  });
  let graceStartedAt = null;
  let evidence = null;
  let now = 0;
  while (
    now <
    deadlineFor(
      options.actionDeadline,
      options.hard,
      graceStartedAt,
      TOOL_TERMINAL_GRACE_MS,
    )
  ) {
    evidence = cancelledToolOutcome(state(now), "cancel", [{ id: "item-hot" }]);
    const grace = toolTerminalGrace(evidence, graceStartedAt, now);
    graceStartedAt = grace.startedAt;
    now += options.tick;
    if (grace.kind === "grace") continue;
    if (evidence) break;
  }
  return { evidence, graceStartedAt, now };
}

test("a tool card that settles after the action deadline is still accepted once", () => {
  const accepted = cancelWaitSimulation({
    settledAt: 40_000,
    cardTerminalAt: 47_500,
    actionDeadline: 45_000,
    hard: 600_000,
    tick: 500,
  });

  assert.equal(accepted.evidence.kind, "tool-terminal");
  assert.ok(accepted.now > 45_000);

  const abandoned = cancelWaitSimulation({
    settledAt: 40_000,
    cardTerminalAt: Number.POSITIVE_INFINITY,
    actionDeadline: 45_000,
    hard: 600_000,
    tick: 500,
  });

  assert.equal(abandoned.evidence.kind, "tool-not-terminal");
  assert.equal(abandoned.graceStartedAt, 40_000);
  assert.equal(abandoned.now, 40_000 + TOOL_TERMINAL_GRACE_MS);

  const clipped = cancelWaitSimulation({
    settledAt: 40_000,
    cardTerminalAt: 47_500,
    actionDeadline: 45_000,
    hard: 46_000,
    tick: 500,
  });

  assert.equal(clipped.evidence.kind, "tool-not-terminal");
  assert.equal(clipped.now, 46_000);
});

test("the wait loop reads its bound from the grace, not the action deadline", () => {
  const text = runnerSource();
  const start = text.indexOf("const actionDeadline = Math.min(");
  assert.ok(start > 0);
  const body = text.slice(
    start,
    text.indexOf("await expandToolRollups", start),
  );
  assert.equal(body.includes("while (Date.now() < actionDeadline)"), false);
  assert.match(
    body,
    /while \(\s*Date\.now\(\) <\s*toolTerminalDeadline\(\s*actionDeadline,\s*deadline,\s*graceStartedAt,\s*TOOL_TERMINAL_GRACE_MS,?\s*\)\s*\)/u,
  );
});

test("a clock that crosses the deadline during a yield refuses the next probe", async () => {
  const ready = {
    triggerPresent: true,
    triggerDisabled: false,
    projectLoading: false,
    alertCount: 0,
    alertsExceeded: false,
  };
  const states = [{ ...ready, projectLoading: true }, ready];
  const probes = [];
  let clock = 0;
  const result = await pollReadiness(
    async () => {
      probes.push(clock);
      return states.shift() ?? ready;
    },
    harnessPickerReadiness,
    {
      until: 45_000,
      now: () => clock,
      yieldFrame: async () => {
        clock = 45_000;
      },
    },
  );

  assert.equal(result.status, "timeout");
  assert.deepEqual(probes, [0]);
});

test("every bounded probe carries the caller deadline it was handed", () => {
  const text = runnerSource();
  const region = (from, to) => {
    const start = text.indexOf(from);
    const end = text.indexOf(to, start);
    assert.ok(start >= 0 && end > start);
    return text.slice(start, end);
  };
  const wait = region("async function waitFor(", "async function shot(");
  assert.match(wait, /if \(await evaluate\(cdp, value, until\)\) return;/u);

  const picker = region(
    "async function waitForHarnessPickerReadiness(",
    "async function selectHarness(",
  );
  assert.match(
    picker,
    /probeState\([\s\S]*?readinessDeadline,[\s\S]*?"harness-picker"/u,
  );

  const cancel = region(
    "async function cancelRunningTool(",
    "async function waitForSubmissionReadiness(",
  );
  assert.match(
    cancel,
    /await expandToolRollups\(cdp, chat, turnId, runningDeadline\);/u,
  );

  assert.equal(text.includes("POLL_PROBE_MARGIN_MS"), false);
});

test("the terminal wait refuses to probe once its deadline has passed", () => {
  const text = runnerSource();
  const expand = text.indexOf(
    "await expandToolRollups(cdp, chat, currentTurnId, turnDeadline);",
  );
  assert.ok(expand > 0);
  assert.match(
    text.slice(expand),
    /^await expandToolRollups\(cdp, chat, currentTurnId, turnDeadline\);\n\s*if \(Date\.now\(\) >= turnDeadline\) break;/u,
  );

  const probeStart = text.indexOf("async function probeRead(");
  assert.ok(probeStart > 0);
  const probe = text.slice(probeStart, text.indexOf("\n}", probeStart));
  assert.match(
    probe,
    /if \(Date\.now\(\) >= until\) return \{ kind: "expired" \};/u,
  );
  assert.match(
    probe,
    /expire: expireAt\(probeExpiry\(until, Date\.now\(\)\)\)/u,
  );
});

test("a probe that resolves Ready after the deadline has passed is refused", async () => {
  let clock = 0;
  const result = await pollReadiness(
    async () => {
      clock = 45_000;
      return {
        triggerPresent: true,
        triggerDisabled: false,
        projectLoading: false,
        alertCount: 0,
        alertsExceeded: false,
      };
    },
    harnessPickerReadiness,
    {
      until: 45_000,
      now: () => clock,
      yieldFrame: async () => undefined,
    },
  );

  assert.equal(result.status, "timeout");
});

test("a read that finishes after its deadline is reported late, not accepted", async () => {
  const { freshRead } = await import("./hr6-readiness.mjs");
  assert.equal(typeof freshRead, "function");

  let clock = 0;
  const late = await freshRead(
    async () => {
      clock = 6_000;
      return { turnIds: ["turn-a"] };
    },
    { now: () => clock, until: 5_000 },
  );
  assert.deepEqual(late, { kind: "late", value: { turnIds: ["turn-a"] } });

  clock = 0;
  const fresh = await freshRead(
    async () => {
      clock = 10;
      return { turnIds: ["turn-a"] };
    },
    { now: () => clock, until: 5_000 },
  );
  assert.deepEqual(fresh, { kind: "value", value: { turnIds: ["turn-a"] } });

  clock = 5_000;
  const exact = await freshRead(async () => "state", {
    now: () => clock,
    until: 5_000,
  });
  assert.deepEqual(exact, { kind: "late", value: "state" });
});

test("remembered turn continuity refuses transcript evidence read after its deadline", () => {
  const text = runnerSource();
  const read = text.slice(
    text.indexOf("async function readRememberedTurnState("),
    text.indexOf("async function rememberTurnIds("),
  );
  assert.match(
    read,
    /async function readRememberedTurnState\(cdp, name, until = deadline\)/u,
  );
  assert.match(read, /const read = await probeRead\(/u);
  assert.match(read, /if \(read\.kind === "error"\) throw read\.error;/u);
  assert.match(read, /return read\.kind === "value" \? read\.value : null;/u);

  const continuity = text.slice(
    text.indexOf("async function assertRememberedTurnContinuity("),
    text.indexOf("async function waitForHarnessPickerReadiness("),
  );
  const deadlineAt = continuity.indexOf("const continuityDeadline =");
  const firstRead = continuity.indexOf("let state = await readState();");
  assert.ok(deadlineAt > 0);
  assert.ok(firstRead > deadlineAt);
  assert.match(continuity, /freshRead\(/u);
  assert.match(continuity, /if \(state === null\) break;/u);
  assert.equal(
    continuity.includes("readRememberedTurnState(cdp, name)"),
    false,
  );
});

test("harness readiness refuses a Ready state observed after its deadline", () => {
  const text = runnerSource();
  const select = text.slice(
    text.indexOf("async function selectHarness("),
    text.indexOf("async function submit("),
  );
  assert.equal(select.includes("readiness = await evaluate("), false);
  assert.match(select, /const fresh = await freshRead\(/u);
  assert.match(
    select,
    /probeState\([\s\S]*?readyDeadline,[\s\S]*?"harness-readiness"/u,
  );
  assert.match(
    select,
    /\{ now: \(\) => Date\.now\(\), until: readyDeadline \}/u,
  );
  assert.match(
    select,
    /if \(fresh\.kind === "late"\) \{\s*readiness = null;\s*break;/u,
  );
});

test("the permission resolution happens inside the accepted turn, before any cancel", () => {
  const text = runnerSource();
  const start = text.indexOf("async function submit(");
  const end = text.indexOf("async function cancelRunningTool(", start);
  assert.ok(start >= 0 && end > start);
  const body = text.slice(start, end);
  const accepted = body.indexOf(
    "await waitForAcceptedTurn(cdp, chat, initialAcceptance, label);",
  );
  const resolve = body.indexOf("await resolvePermission(");
  const cancelCall = body.indexOf(
    "observedRunningTools = await cancelRunningTool(",
  );
  assert.ok(accepted >= 0);
  assert.ok(resolve > accepted);
  assert.ok(cancelCall > resolve);
  assert.match(body, /options\.permission/u);
});

test("the permission wait reads its card through the fail-closed permission module", () => {
  const text = runnerSource();
  assert.match(text, /from "\.\/hr6-permission-evidence\.mjs"/u);
  const start = text.indexOf("async function resolvePermission(");
  const end = text.indexOf("\nasync function ", start + 1);
  assert.ok(start >= 0 && end > start);
  const body = text.slice(start, end);
  assert.match(
    body,
    /const permissionDeadline = Math\.min\(deadline, Date\.now\(\) \+ /u,
  );
  assert.match(body, /permissionStateExpression\(/u);
  assert.match(body, /permissionReadiness\(/u);
  assert.match(body, /resolvePermissionExpression\(/u);
  assert.match(body, /permissionOutcome\(/u);
  assert.match(body, /permissionDeadline\)/u);
  assert.match(body, /permissionIdentity\(readiness\.card\)/u);
  assert.match(body, /expected,?\n/u);
  assert.match(text, /permissionExpected/u);
  assert.match(
    text,
    /const PERMISSION_EXPECTED = \{ command: "Created file" \};/u,
  );
  assert.match(
    text,
    /const DECLINE_EXPECTED = \{ command: "Created file" \};/u,
  );
  assert.equal(text.includes("const DECLINE_EXPECTED = ["), false);
  assert.match(
    text,
    /const LONG_TOOL_EXPECTED = \{ command: LONG_TOOL_COMMAND \};/u,
  );
  assert.match(
    text,
    /const PERMISSION_PENDING_KINDS = \["permission-actionable"\];/u,
  );
});

test("a running-tool cancel honours the window and tolerance it was handed", () => {
  const text = runnerSource();
  const start = text.indexOf("async function cancelRunningTool(");
  const end = text.indexOf("async function waitForSubmissionReadiness(", start);
  assert.ok(start >= 0 && end > start);
  const body = text.slice(start, end);
  assert.match(
    body,
    /const runningDeadline = Math\.min\(deadline, Date\.now\(\) \+ windowMs\);/u,
  );
  assert.match(body, /tolerated\.includes\(readiness\.kind\)/u);
});

test("the diff evidence reads the change summary, the pane and the file on disk", () => {
  const text = runnerSource();
  assert.match(text, /from "\.\/hr6-diff-evidence\.mjs"/u);
  const changeStart = text.indexOf("async function assertFileChange(");
  const changeEnd = text.indexOf("\nasync function ", changeStart + 1);
  assert.ok(changeStart >= 0 && changeEnd > changeStart);
  const change = text.slice(changeStart, changeEnd);
  assert.match(change, /fileChangesExpression\(/u);
  assert.match(change, /fileChangeOutcome\(/u);
  const diffStart = text.indexOf("async function openDiffArtifact(");
  const diffEnd = text.indexOf("\nasync function ", diffStart + 1);
  assert.ok(diffStart >= 0 && diffEnd > diffStart);
  const diff = text.slice(diffStart, diffEnd);
  assert.match(diff, /openDiffExpression\(/u);
  assert.match(diff, /diffSurfaceExpression\(\)/u);
  assert.match(diff, /diffSurfaceOutcome\(/u);
  assert.match(diff, /await shot\(cdp, /u);
  assert.match(
    diff,
    /openDiffExpression\(\s*focusedRememberedChatSurfacePreamble\(/u,
  );
  assert.match(text, /operation: "created",/u);
  assert.match(text, /deletions: 0,/u);
  assert.match(text, /addedLines: \[PERMISSION_LINE\],/u);
  assert.equal((text.match(/additions: 1,/gu) ?? []).length, 2);
});

test("the scratch assertions stay inside the fixture directory the runner was given", () => {
  const text = runnerSource();
  const start = text.indexOf("function scratchPath(");
  const end = text.indexOf("\nasync function ", start);
  assert.ok(start >= 0 && end > start);
  const body = text.slice(start, end);
  assert.match(body, /path\.join\(output, "scratch"\)/u);
  assert.match(body, /README\.md/u);
  assert.match(body, /alpha\.txt/u);
  assert.equal(body.includes(".."), false);
  assert.match(text, /function assertScratchFile\(/u);
  assert.match(text, /function assertScratchAbsent\(/u);
  assert.match(text, /const SCRATCH_BYTE_LIMIT = 4096;/u);
  const fileStart = text.indexOf("function assertScratchFile(");
  const fileEnd = text.indexOf("function assertScratchAbsent(", fileStart);
  assert.ok(fileStart >= 0 && fileEnd > fileStart);
  const file = text.slice(fileStart, fileEnd);
  assert.match(file, /isSymbolicLink\(\)/u);
  assert.match(file, /isFile\(\)/u);
  assert.match(file, /stats\.size > SCRATCH_BYTE_LIMIT/u);
  assert.match(file, /readScratchText\(file, stats\.size\)/u);
  assert.equal(file.includes("readFileSync"), false);
  const reader = text.slice(
    text.indexOf("function readScratchText("),
    text.indexOf("function assertScratchFile("),
  );
  assert.match(reader, /Math\.min\(size, SCRATCH_BYTE_LIMIT\)/u);
  assert.match(reader, /readSync\(handle, buffer, 0, buffer\.length, 0\)/u);
  assert.match(reader, /closeSync\(handle\)/u);
  const absent = text.slice(
    fileEnd,
    text.indexOf("\nasync function ", fileEnd),
  );
  assert.match(absent, /scratchStat\(/u);
  assert.equal(absent.includes("existsSync"), false);
  assert.match(text, /function scratchStat\(file\) \{/u);
  assert.match(text, /lstatSync\(file\)/u);
});

test("every new evidence poll classifies a read the host deadline still owns", () => {
  const text = runnerSource();
  assert.match(
    text,
    /async function freshState\(cdp, chat, value, until, label, stage, turnId\)/u,
  );
  const helper = text.slice(
    text.indexOf("async function freshState("),
    text.indexOf(
      "\nasync function ",
      text.indexOf("async function freshState("),
    ),
  );
  assert.match(helper, /freshRead\(/u);
  assert.match(helper, /probeState\(cdp, value, until, label, stage\)/u);
  assert.match(helper, /fresh\.kind === "value"/u);
  assert.match(
    helper,
    /throw new Error\(`hr6-late-\$\{label\}-\$\{stage\}`\)/u,
  );
  for (const region of [
    "async function resolvePermission(",
    "async function assertFileChange(",
    "async function openDiffArtifact(",
  ]) {
    const start = text.indexOf(region);
    const end = text.indexOf("\nasync function ", start + 1);
    assert.ok(start >= 0 && end > start, region);
    const body = text.slice(start, end);
    assert.equal(
      body.includes("await evaluate(cdp, stateExpression"),
      false,
      region,
    );
    assert.match(body, /await freshState\(/u);
  }
  const permission = text.slice(
    text.indexOf("async function resolvePermission("),
    text.indexOf(
      "\nasync function ",
      text.indexOf("async function resolvePermission(") + 1,
    ),
  );
  assert.equal((permission.match(/await freshState\(/gu) ?? []).length, 2);
});

test("the extended journey drives permission, diff, decline and long-tool cancel after step 08", () => {
  const text = runnerSource();
  const order = [
    '"08-claude-tool-result"',
    '"09-claude-permission-write"',
    '"10-claude-diff"',
    '"11-claude-permission-decline"',
    '"12-claude-long-tool-cancel"',
    "observations.completedAt = new Date().toISOString();",
  ];
  let cursor = -1;
  for (const marker of order) {
    const found = text.indexOf(marker, cursor + 1);
    assert.ok(found > cursor, marker);
    cursor = found;
  }
  assert.match(text, /const permissionTurnId = await submit\(/u);
  assert.match(text, /cancelTolerated: \["tool-awaiting-approval"\]/u);
  assert.match(text, /cancelWindowMs: \d+/u);
  assert.equal(text.includes('"08-claude-tool-cancel"'), false);
  assert.match(
    text,
    /await submit\(cdp, "codex", TOOL_CANCEL_PROMPT, "04-codex-tool-cancel", true\);/u,
  );
  assert.equal((text.match(/TOOL_CANCEL_PROMPT/gu) ?? []).length, 2);
});

test("the long-tool permission is pinned to the exact requested command", () => {
  const text = runnerSource();
  assert.match(text, /const LONG_TOOL_RELEASE_FILE = "hr6-release\.txt";/u);
  assert.ok(text.includes(LONG_TOOL_SOURCE), LONG_TOOL_SOURCE);
  assert.equal(text.includes("LONG_TOOL_MARKER"), false);
  assert.equal(text.includes("Write-Output"), false);
  assert.equal(text.includes("Start-Sleep -Seconds 50"), false);
  assert.equal((text.match(/Start-Sleep/gu) ?? []).length, 1);
  assert.match(
    text,
    /const LONG_TOOL_EXPECTED = \{ command: LONG_TOOL_COMMAND \};/u,
  );
  assert.match(text, /const LONG_TOOL_PROMPT = [^;]*\$\{LONG_TOOL_COMMAND\}/u);
  assert.equal(text.includes("const LONG_TOOL_EXPECTED = ["), false);
  for (const generic of ["shell", "Shell", "bash", "Bash", "Execute", "sleep"])
    assert.equal(
      text.includes(`const LONG_TOOL_EXPECTED = ["${generic}"`),
      false,
    );
});

test("the long-tool prompt asks for a foreground wait on the release file", () => {
  const text = runnerSource();
  const start = text.indexOf("const LONG_TOOL_PROMPT = ");
  const end = text.indexOf("\nconst ", start + 1);
  assert.ok(start >= 0 && end > start);
  const prompt = text.slice(start, end);
  assert.match(prompt, /in the foreground/u);
  assert.match(prompt, /nothing added, removed, or reordered/u);
  assert.match(
    prompt,
    /waits until \$\{LONG_TOOL_RELEASE_FILE\} exists or its 120-second deadline passes/u,
  );
  assert.match(prompt, /Do not run it in the background/u);
  assert.match(prompt, /no other tools/u);
});

test("the long-tool self-bound outlives the cancel window by a real margin", () => {
  const text = runnerSource();
  const bounds = text.match(/AddSeconds\((\d+)\)/gu) ?? [];
  assert.equal(bounds.length, 1);
  const windows = text.match(/cancelWindowMs: ([\d_]+),/gu) ?? [];
  assert.equal(windows.length, 1);
  const boundMs = Number(/AddSeconds\((\d+)\)/u.exec(text)[1]) * 1000;
  const windowMs = Number(
    /cancelWindowMs: ([\d_]+),/u.exec(text)[1].replaceAll("_", ""),
  );
  assert.ok(
    boundMs > windowMs + CANCEL_BOUND_MARGIN_MS,
    `${boundMs} ${windowMs}`,
  );
  assert.equal(windowMs, 90_000);
});

test("the cancelled long tool must report a cancelled card, never a completed one", () => {
  const text = runnerSource();
  const start = text.indexOf("cancelEvidence = cancelledToolOutcome(");
  assert.ok(start >= 0);
  const call = text.slice(start, text.indexOf(");", start));
  assert.match(call, /options\.cancelTerminalStates/u);
  const step = text.indexOf('"12-claude-long-tool-cancel"');
  const end = text.indexOf("assertScratchAbsent(LONG_TOOL_RELEASE_FILE", step);
  assert.ok(step >= 0 && end > step);
  assert.match(text.slice(step, end), /cancelTerminalStates: \["cancelled"\]/u);
  assert.equal((text.match(/cancelTerminalStates: \[/gu) ?? []).length, 1);
});

test("the cancelled long tool leaves no release file behind", () => {
  const text = runnerSource();
  const submitted = text.indexOf('"12-claude-long-tool-cancel"');
  const absent = text.indexOf(
    'assertScratchAbsent(LONG_TOOL_RELEASE_FILE, "12-claude-long-tool-cancel")',
  );
  assert.ok(submitted >= 0);
  assert.ok(absent > submitted);
  assert.ok(absent < text.indexOf("observations.completedAt"));
});

test("the permission outcome is checked against the card that was approved", () => {
  const text = runnerSource();
  const start = text.indexOf("async function resolvePermission(");
  const end = text.indexOf("\nasync function ", start + 1);
  const body = text.slice(start, end);
  assert.match(body, /permissionOutcome\(state, decision, identity\)/u);
});

test("the claude chat switches to the approval permission mode before step 09", () => {
  const text = runnerSource();
  assert.match(text, /from "\.\/hr6-access-mode\.mjs"/u);
  assert.match(text, /APPROVAL_ACCESS_MODE/u);
  const start = text.indexOf("async function selectAccessMode(");
  assert.ok(start >= 0, "runner has no access-mode selection");
  const end = text.indexOf("\nasync function ", start + 1);
  const body = text.slice(start, end);
  assert.match(body, /accessModeStateExpression\(/u);
  assert.match(body, /accessModeSettled\(/u);
  assert.match(body, /openAccessModeExpression\(/u);
  assert.match(body, /accessModeExpandedExpression\(/u);
  assert.match(body, /chooseAccessModeExpression\(/u);
  assert.match(body, /hr6-access-mode-/u);
  const call = text.search(
    /await selectAccessMode\(\s*cdp,\s*"claude",\s*APPROVAL_ACCESS_MODE,\s*"08b-claude-access-mode",?\s*\)/u,
  );
  assert.ok(call >= 0, "runner has no access-mode step");
  const toolResult = text.indexOf('"08-claude-tool-result"');
  const write = text.indexOf('"09-claude-permission-write"');
  assert.ok(toolResult >= 0 && call > toolResult && write > call);
});

test("the access-mode reads are refused when they land past the deadline", () => {
  const text = runnerSource();
  const start = text.indexOf("async function selectAccessMode(");
  const end = text.indexOf("\nasync function ", start + 1);
  const body = text.slice(start, end);
  assert.equal((body.match(/freshState\(/gu) ?? []).length, 2);
  assert.equal(body.includes("await evaluate(cdp, stateExpression"), false);
  assert.equal(body.includes("accessModeReadiness("), false);
});
