import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { JSDOM } from "jsdom";

import {
  READ_PROBE,
  SCRATCH_PROBE_TEXT,
  completedToolResults,
  exactOutputMatches,
  exactReplyMatches,
  toolEvidenceOutcome,
  toolResultEvidence,
} from "./hr6-tool-evidence.mjs";
import {
  TERMINAL_LIMITS,
  boundedFirstProjection,
  boundedMatchProjection,
  boundedNodeTextProjection,
  terminalExpression,
  toolCardsProjection,
} from "./hr6-terminal-expression.mjs";
import { focusedChatSurfacePreamble } from "./hr6-chat-scope.mjs";

const settled = {
  composerDisabled: false,
  cancelVisible: false,
  alerts: [],
  turnsExceeded: false,
  alertsExceeded: false,
};

function readCard(overrides) {
  return {
    id: "item-alpha",
    state: "completed",
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

function resultCard(overrides) {
  return readCard({
    outputText: SCRATCH_PROBE_TEXT,
    outputVisible: true,
    ...overrides,
  });
}

function probeTurn(overrides) {
  return {
    id: "tool",
    status: "Completed",
    assistantText: SCRATCH_PROBE_TEXT,
    assistantTextTruncated: false,
    tools: [resultCard()],
    toolsExceeded: false,
    ...overrides,
  };
}

function escapeMarkup(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function fakeCard(card) {
  const resources =
    card.resources > 0
      ? '<ul class="acp-resource-list">' +
        "<li></li>".repeat(card.resources) +
        "</ul>"
      : "";
  const output = card.outputText
    ? '<pre aria-label="Tool output"' +
      (card.outputVisible ? "" : " data-hidden") +
      ">" +
      escapeMarkup(card.outputText) +
      "</pre>"
    : "";
  return (
    '<article class="acp-activity-card acp-tool-card" data-activity-kind="' +
    escapeMarkup(card.kind) +
    '" data-activity-state="' +
    escapeMarkup(card.state) +
    (card.id ? '" data-acp-item-id="' + escapeMarkup(card.id) : "") +
    '"><strong>' +
    escapeMarkup(card.label) +
    "</strong>" +
    output +
    (card.detail ? '<div class="acp-details"><p>detail</p></div>' : "") +
    resources +
    "</article>"
  );
}

function fakeTurn(id, text, cards = []) {
  return (
    '<section class="acp-turn" data-turn-id="' +
    escapeMarkup(id) +
    '"><span data-turn-status aria-label="Completed"></span>' +
    '<article class="acp-message" data-message-role="assistant">' +
    '<div class="acp-markdown"><p>' +
    escapeMarkup(text) +
    "</p></div></article>" +
    cards.map(fakeCard).join("") +
    "</section>"
  );
}

function runTerminal(turns, alerts = []) {
  const dom = new JSDOM(
    '<!doctype html><section class="agent-chat-pane agent-chat-pane--focused">' +
      '<section class="chat-surface" aria-label="Agent chat">' +
      '<textarea aria-label="Message agent"></textarea>' +
      alerts
        .map((value) => '<div role="alert">' + escapeMarkup(value) + "</div>")
        .join("") +
      turns.join("") +
      "</section></section>",
  );
  dom.window.Element.prototype.getClientRects = function getClientRects() {
    return this.hasAttribute("data-hidden") ? [] : [{}];
  };
  return new Function(
    "document",
    "return " +
      terminalExpression(focusedChatSurfacePreamble("return null;")) +
      ";",
  )(dom.window.document);
}

test("only visible rendered output counts, never a resource link or a detail echo", () => {
  assert.equal(toolResultEvidence(resultCard()), "output");
  assert.equal(
    toolResultEvidence(readCard({ resources: 3 })),
    "referencedResource",
  );
  assert.equal(toolResultEvidence(readCard({ detail: true })), "none");
  assert.equal(
    toolResultEvidence(
      readCard({ outputText: SCRATCH_PROBE_TEXT, outputVisible: false }),
    ),
    "none",
  );
  assert.equal(toolResultEvidence(resultCard({ state: "inProgress" })), "none");
  assert.deepEqual(
    completedToolResults({ tools: [readCard({ resources: 3 })] }),
    [],
  );
  assert.deepEqual(completedToolResults({ tools: undefined }), []);
});

test("a completed probe tool that renders only referenced resources is refused", () => {
  assert.deepEqual(
    toolEvidenceOutcome(
      {
        ...settled,
        turns: [probeTurn({ tools: [readCard({ resources: 1 })] })],
      },
      "tool",
    ),
    { kind: "tool-resource-only" },
  );
});

test("output hidden inside a collapsed disclosure is not a visible result", () => {
  assert.deepEqual(
    toolEvidenceOutcome(
      {
        ...settled,
        turns: [
          probeTurn({
            tools: [
              readCard({
                outputText: SCRATCH_PROBE_TEXT,
                outputVisible: false,
              }),
            ],
          }),
        ],
      },
      "tool",
    ),
    { kind: "tool-output-hidden" },
  );
});

test("evidence never counts a tool unrelated to the read-only probe", () => {
  assert.deepEqual(
    toolEvidenceOutcome(
      {
        ...settled,
        turns: [
          probeTurn({
            tools: [resultCard({ kind: "execute", label: "npm install" })],
          }),
        ],
      },
      "tool",
    ),
    { kind: "tool-unrelated" },
  );
});

test("acceptance requires the exact scratch text in the reply and in the card output", () => {
  assert.deepEqual(
    toolEvidenceOutcome({ ...settled, turns: [probeTurn()] }, "tool"),
    {
      kind: "tool-result",
      results: [{ kind: "read", label: "Read alpha.txt", evidence: "output" }],
    },
  );
  assert.equal(
    toolEvidenceOutcome(
      {
        ...settled,
        turns: [
          probeTurn({
            assistantText:
              "I'll read alpha.txt with a read-only command.\n" +
              SCRATCH_PROBE_TEXT,
            tools: [resultCard({ outputText: "1\t" + SCRATCH_PROBE_TEXT })],
          }),
        ],
      },
      "tool",
    )?.kind,
    "tool-result",
  );
  assert.equal(
    toolEvidenceOutcome(
      { ...settled, turns: [probeTurn({ assistantText: "I read the file." })] },
      "tool",
    )?.kind,
    "response-mismatch",
  );
  assert.equal(
    toolEvidenceOutcome(
      {
        ...settled,
        turns: [
          probeTurn({ tools: [resultCard({ outputText: "some other file" })] }),
        ],
      },
      "tool",
    )?.kind,
    "tool-output-mismatch",
  );
  assert.equal(
    exactReplyMatches(
      {
        assistantText: SCRATCH_PROBE_TEXT + " and then some",
        assistantTextTruncated: false,
      },
      READ_PROBE,
    ),
    false,
  );
  assert.equal(exactOutputMatches(resultCard(), READ_PROBE), true);
});

test("truncated text can never prove exactness", () => {
  assert.equal(
    toolEvidenceOutcome(
      { ...settled, turns: [probeTurn({ assistantTextTruncated: true })] },
      "tool",
    )?.kind,
    "response-mismatch",
  );
  assert.equal(
    exactOutputMatches(resultCard({ outputTruncated: true }), READ_PROBE),
    false,
  );
});

test("a settled prompt terminal without any tool card is not a completed tool result", () => {
  assert.deepEqual(
    toolEvidenceOutcome(
      { ...settled, turns: [probeTurn({ tools: [] })] },
      "tool",
    ),
    { kind: "tool-absent" },
  );
});

test("a completed probe tool card that renders no result content is refused", () => {
  assert.deepEqual(
    toolEvidenceOutcome(
      {
        ...settled,
        turns: [probeTurn({ tools: [readCard({ detail: true })] })],
      },
      "tool",
    ),
    { kind: "tool-result-absent" },
  );
});

test("tool evidence keeps waiting while the turn is unsettled and never borrows another turn", () => {
  assert.equal(
    toolEvidenceOutcome(
      {
        composerDisabled: true,
        cancelVisible: true,
        alerts: [],
        turnsExceeded: false,
        alertsExceeded: false,
        turns: [
          { id: "tool", status: "Running", tools: [], toolsExceeded: false },
        ],
      },
      "tool",
    ),
    null,
  );
  assert.equal(
    toolEvidenceOutcome(
      {
        ...settled,
        turns: [
          probeTurn({ id: "earlier" }),
          probeTurn({ id: "tool", tools: [] }),
        ],
      },
      "tool",
    )?.kind,
    "tool-absent",
  );
  assert.equal(toolEvidenceOutcome({ ...settled, turns: [] }, "tool"), null);
});

test("an unsuccessful tool or an unsuccessful turn is never reported as a tool result", () => {
  assert.deepEqual(
    toolEvidenceOutcome(
      {
        ...settled,
        turns: [probeTurn({ tools: [resultCard({ state: "failed" })] })],
      },
      "tool",
    ),
    { kind: "tool-unsuccessful" },
  );
  assert.deepEqual(
    toolEvidenceOutcome(
      { ...settled, turns: [probeTurn({ status: "Cancelled", tools: [] })] },
      "tool",
    ),
    { kind: "turn-not-completed" },
  );
  assert.deepEqual(
    toolEvidenceOutcome(
      {
        composerDisabled: false,
        cancelVisible: false,
        alerts: ["Harness failure"],
        turns: [probeTurn()],
      },
      "tool",
    ),
    { kind: "error" },
  );
});

test("the terminal projection bounds turn count and assistant text before crossing CDP", () => {
  const overflow = TERMINAL_LIMITS.turns + 9;
  const turns = Array.from({ length: overflow }, (_, index) =>
    fakeTurn("turn-" + index, "x".repeat(TERMINAL_LIMITS.text + 50)),
  );
  const result = runTerminal(turns, ["first alert", "second alert"]);
  assert.equal(result.turns.length, TERMINAL_LIMITS.turns);
  assert.equal(result.turnCount, null);
  assert.equal(result.turnsExceeded, true);
  assert.equal(result.turns.at(-1).id, "turn-" + (overflow - 1));
  assert.equal(result.turns[0].assistantTextTruncated, true);
  assert.ok(result.turns[0].assistantText.endsWith(TERMINAL_LIMITS.marker));
  assert.equal(
    result.turns[0].assistantText.length,
    TERMINAL_LIMITS.text + TERMINAL_LIMITS.marker.length,
  );
  assert.ok(result.alerts.length <= TERMINAL_LIMITS.alerts);
});

test("the terminal projection reads visible tool output inside the current turn", () => {
  const result = runTerminal([
    fakeTurn("tool", SCRATCH_PROBE_TEXT, [
      resultCard({ resources: 2, detail: true }),
    ]),
  ]);
  assert.deepEqual(result.turns[0].tools, [
    {
      id: "item-alpha",
      state: "completed",
      kind: "read",
      label: "Read alpha.txt",
      outputText: SCRATCH_PROBE_TEXT,
      outputTruncated: false,
      outputVisible: true,
      resources: 2,
      resourcesExceeded: false,
      detail: true,
    },
  ]);
  assert.equal(result.turns[0].assistantTextTruncated, false);
  assert.equal(toolEvidenceOutcome(result, "tool")?.kind, "tool-result");
  const hidden = runTerminal([
    fakeTurn("tool", SCRATCH_PROBE_TEXT, [
      resultCard({ outputVisible: false }),
    ]),
  ]);
  assert.equal(hidden.turns[0].tools[0].outputVisible, false);
  assert.equal(toolEvidenceOutcome(hidden, "tool")?.kind, "tool-output-hidden");
});

test("a probe card hidden beyond the tool-card cap is refused, never read as unrelated", () => {
  const cards = [resultCard({ id: "item-probe" })];
  for (let index = 0; index < TERMINAL_LIMITS.tools; index += 1)
    cards.push(
      readCard({ id: "item-" + index, kind: "think", label: "Think" }),
    );
  const result = runTerminal([fakeTurn("tool", SCRATCH_PROBE_TEXT, cards)]);
  const projected = result.turns[0];

  assert.equal(projected.tools.length, TERMINAL_LIMITS.tools);
  assert.equal(
    projected.tools.some((tool) => tool.id === "item-probe"),
    false,
  );
  assert.equal(projected.toolsExceeded, true);
  assert.deepEqual(toolEvidenceOutcome(result, "tool"), {
    kind: "scan-exceeded",
  });
});

test("tool evidence fails closed on every non-false scan flag", () => {
  for (const flag of [true, undefined, "false"]) {
    assert.deepEqual(
      toolEvidenceOutcome(
        { ...settled, turns: [probeTurn({ toolsExceeded: flag })] },
        "tool",
      ),
      { kind: "scan-exceeded" },
    );
    assert.deepEqual(
      toolEvidenceOutcome(
        { ...settled, turnsExceeded: flag, turns: [probeTurn()] },
        "tool",
      ),
      { kind: "scan-exceeded" },
    );
  }
  assert.equal(
    toolEvidenceOutcome({ ...settled, turns: [probeTurn()] }, "tool").kind,
    "tool-result",
  );
});

test("a missing or non-boolean truncation flag is never proven as an exact match", () => {
  const { outputTruncated, ...untagged } = resultCard();
  const outputFlags = [undefined, "false", untagged.outputTruncated];

  assert.equal(exactOutputMatches(untagged), false);
  for (const flag of outputFlags) {
    assert.equal(
      exactOutputMatches(resultCard({ outputTruncated: flag })),
      false,
    );
    assert.equal(
      toolEvidenceOutcome(
        {
          ...settled,
          turns: [
            probeTurn({ tools: [resultCard({ outputTruncated: flag })] }),
          ],
        },
        "tool",
      ).kind,
      "tool-output-mismatch",
    );
    assert.equal(
      exactReplyMatches({
        assistantText: SCRATCH_PROBE_TEXT,
        assistantTextTruncated: flag,
      }),
      false,
    );
    assert.equal(
      toolEvidenceOutcome(
        { ...settled, turns: [probeTurn({ assistantTextTruncated: flag })] },
        "tool",
      ).kind,
      "response-mismatch",
    );
  }
  assert.equal(outputTruncated, false);
});

test("a same-id turn or a half scanned alert region never proves tool evidence", () => {
  assert.deepEqual(
    toolEvidenceOutcome(
      {
        ...settled,
        alertsExceeded: false,
        turns: [probeTurn(), probeTurn({ status: null })],
      },
      "tool",
    ),
    { kind: "turn-ambiguous" },
  );
  assert.equal(
    toolEvidenceOutcome(
      {
        ...settled,
        alertsExceeded: false,
        turns: [probeTurn({ status: undefined })],
      },
      "tool",
    ),
    null,
  );
  for (const flag of [true, undefined, "false"]) {
    assert.deepEqual(
      toolEvidenceOutcome(
        { ...settled, alertsExceeded: flag, turns: [probeTurn()] },
        "tool",
      ),
      { kind: "scan-exceeded" },
    );
  }
});

test("a resource list the scan could not finish is never counted as evidence", () => {
  assert.equal(
    toolResultEvidence(readCard({ resources: 1, resourcesExceeded: true })),
    "none",
  );
  assert.equal(
    toolResultEvidence(readCard({ resources: 1, resourcesExceeded: false })),
    "referencedResource",
  );
  const { resourcesExceeded, ...untagged } = readCard({ resources: 1 });
  assert.equal(toolResultEvidence(untagged), "none");
  assert.equal(resourcesExceeded, false);
  const state = runTerminal([
    fakeTurn("tool", SCRATCH_PROBE_TEXT, [
      readCard({ resources: TERMINAL_LIMITS.resources + 1 }),
    ]),
  ]);
  assert.equal(state.turns[0].tools[0].resourcesExceeded, true);
});

test("the runner fails a scan-exceeded tool read instead of polling to its deadline", () => {
  const source = readFileSync(
    new URL("./hr6-live-run.mjs", import.meta.url),
    "utf8",
  );
  assert.match(source, /toolEvidence\.kind !== "tool-result"/u);
  assert.match(
    source,
    /hr6-tool-evidence-\$\{toolEvidence\.kind\}-\$\{label\}/u,
  );
});

test("the terminal projection preserves code text without code-block chrome", () => {
  const dom = new JSDOM(
    `<!doctype html><section class="agent-chat-pane agent-chat-pane--focused"><section class="chat-surface" aria-label="Agent chat"><textarea aria-label="Message agent"></textarea><section class="acp-turn" data-turn-id="tool"><span data-turn-status aria-label="Completed"></span><article class="acp-message" data-message-role="assistant"><div class="acp-markdown"><p>alpha.txt contents:</p><section class="acp-code-block" aria-label="Code code"><header class="acp-code-block__header"><span>Code</span><span class="acp-code-block__actions"><button type="button" aria-label="Copy code">Copy</button></span></header><pre><code>${SCRATCH_PROBE_TEXT}</code></pre></section></div></article></section></section></section>`,
  );
  const result = new Function(
    "document",
    "return " +
      terminalExpression(focusedChatSurfacePreamble("return null;")) +
      ";",
  )(dom.window.document);

  assert.equal(
    result.turns[0].assistantText,
    "alpha.txt contents:\n" + SCRATCH_PROBE_TEXT,
  );
  assert.equal(exactReplyMatches(result.turns[0]), true);
});

test("the journey expands tool disclosures and submits the probe with its expectation", () => {
  const source = readFileSync(
    new URL("./hr6-live-run.mjs", import.meta.url),
    "utf8",
  );
  assert.match(
    source,
    /toolEvidenceOutcome\(\s*terminal,\s*currentTurnId,\s*READ_PROBE,?\s*\)/u,
  );
  assert.match(
    source,
    /toolRollupExpression\(\s*focusedRememberedChatSurfacePreamble\(chat, "return 0;"\),\s*turnId,?\s*\)/u,
  );
  assert.match(
    source,
    /expandToolRollups\(cdp, chat, currentTurnId, turnDeadline\)/u,
  );
  const codexResult = source.indexOf('"04-codex-tool-result"');
  const codexCancel = source.indexOf('"04-codex-tool-cancel"');
  const claudeResult = source.indexOf('"08-claude-tool-result"');
  const claudeCancel = source.indexOf('"12-claude-long-tool-cancel"');
  assert.ok(codexResult > 0);
  assert.ok(codexCancel > codexResult);
  assert.ok(claudeResult > codexCancel);
  assert.ok(claudeCancel > claudeResult);
  assert.equal(source.includes('"08-claude-tool-cancel"'), false);
  for (const label of ['"04-codex-tool-result"', '"08-claude-tool-result"'])
    assert.match(
      source.slice(source.indexOf(label), source.indexOf(label) + 200),
      /undefined,\s*true,/u,
    );
});

test("the scratch fixture writes exactly the probe text acceptance requires", () => {
  const launcher = readFileSync(
    new URL("./hr6-live-run.ps1", import.meta.url),
    "utf8",
  );
  assert.ok(
    launcher.includes('"alpha.txt") -Value "' + SCRATCH_PROBE_TEXT + '"'),
  );
});

test("one bounded tool-card projection serves both the terminal and snapshot reads", () => {
  const source = readFileSync(
    new URL("./hr6-live-run.mjs", import.meta.url),
    "utf8",
  );
  assert.equal(source.includes("data-activity-state"), false);
  assert.ok(source.includes("toolCardsProjection"));
});

test("native process streams are discarded unless diagnostics are opted in", () => {
  const launcher = readFileSync(
    new URL("./hr6-live-run.ps1", import.meta.url),
    "utf8",
  );
  const sink = readFileSync(
    new URL("./hr6-null-sink.ps1", import.meta.url),
    "utf8",
  );
  assert.ok(sink.includes("function Get-NullSinkTargets"));
  assert.ok(launcher.includes("hr6-null-sink.ps1"));
  assert.ok(launcher.includes("$sink = Get-NullSinkTargets"));
  for (const log of ["vite.log", "vite.stderr.log"])
    assert.equal(launcher.includes(log), false);
  for (const log of ["sage.stdout.log", "sage.stderr.log"])
    assert.ok(
      launcher.indexOf("if ($captureRequested) {") < launcher.indexOf(log),
    );
  assert.ok(launcher.includes("$sageOut = $sink.stdout"));
  assert.ok(launcher.includes("$sageErr = $sink.stderr"));
  for (const gone of [
    "Limit-DiagnosticLog",
    "Remove-SageStdout",
    "ReadAllText",
    "$redactions",
  ])
    assert.equal(launcher.includes(gone), false);
  assert.equal(launcher.includes("output = $output"), false);
  assert.equal(launcher.includes("scratch = $scratch"), false);
  assert.ok(launcher.includes("sage = $sageExit"));
  assert.ok(launcher.includes("vite = $viteExit"));
});

function matchRoot(count) {
  const items = Array.from({ length: count }, () => '<p class="m"></p>').join(
    "",
  );
  return new JSDOM(
    `<div id="root">${items}</div>`,
  ).window.document.getElementById("root");
}

function countingRoot(size, counter) {
  const children = [];
  for (let index = 0; index < size; index += 1)
    children.push({
      children: [],
      matches() {
        counter.calls += 1;
        return true;
      },
    });
  return {
    children,
    matches() {
      counter.calls += 1;
      return false;
    },
  };
}

test("a bounded match reports overflow only past its cap, never at it", () => {
  const find = new Function(`return ${boundedMatchProjection(4)};`)();
  const exact = find(matchRoot(4), "p.m", false);
  assert.equal(exact.nodes.length, 4);
  assert.equal(exact.exceeded, false);
  const under = find(matchRoot(3), "p.m", false);
  assert.equal(under.nodes.length, 3);
  assert.equal(under.exceeded, false);
  const over = find(matchRoot(5), "p.m", false);
  assert.equal(over.nodes.length, 4);
  assert.equal(over.exceeded, true);
});

test("the single-node projections still stop at the first match", () => {
  const one = new Function(`return ${boundedFirstProjection()};`)();
  const small = { calls: 0 };
  const huge = { calls: 0 };
  assert.ok(one(countingRoot(2, small), "p.m"));
  assert.ok(one(countingRoot(1024, huge), "p.m"));
  assert.equal(small.calls, 1);
  assert.equal(huge.calls, small.calls);
});

test("an oversized text node is capped before it is ever trimmed", () => {
  const limit = TERMINAL_LIMITS.text;
  const dom = new JSDOM(`<div id="root"></div>`);
  const root = dom.window.document.getElementById("root");
  root.appendChild(dom.window.document.createTextNode("x".repeat(limit * 40)));
  const walk = new Function(`return ${boundedNodeTextProjection()};`)();
  const trim = String.prototype.trim;
  let widest = 0;
  String.prototype.trim = function bounded() {
    widest = Math.max(widest, this.length);
    return trim.call(this);
  };
  let walked;
  try {
    walked = walk(root);
  } finally {
    String.prototype.trim = trim;
  }
  assert.ok(widest > 0);
  assert.ok(widest <= limit, `trimmed ${widest} characters`);
  assert.equal(walked.truncated, true);
  assert.equal(walked.text.length, limit + TERMINAL_LIMITS.marker.length);
  assert.ok(walked.text.endsWith(TERMINAL_LIMITS.marker));
});

test("tool card identity and state never cross CDP unbounded", () => {
  const id = `item-${"z".repeat(TERMINAL_LIMITS.identity)}`;
  const state = "s".repeat(TERMINAL_LIMITS.label * 3);
  const dom = new JSDOM(
    `<div id="surface"><div class="acp-tool-card" data-acp-item-id="${id}" data-activity-state="${state}" data-activity-kind="read"><strong>Read alpha.txt</strong></div></div>`,
  );
  const project = new Function(`return ${toolCardsProjection()};`)();
  const { cards } = project(dom.window.document.getElementById("surface"));
  assert.equal(cards.length, 1);
  assert.equal(cards[0].id, null);
  assert.equal(
    cards[0].state.length,
    TERMINAL_LIMITS.label + TERMINAL_LIMITS.marker.length,
  );
  assert.ok(cards[0].state.endsWith(TERMINAL_LIMITS.marker));
  assert.equal(cards[0].kind, "read");
});
