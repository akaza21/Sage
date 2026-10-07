import assert from "node:assert/strict";
import test from "node:test";
import { JSDOM } from "jsdom";

import { focusedChatSurfacePreamble } from "./hr6-chat-scope.mjs";

function load() {
  return import("./hr6-permission-evidence.mjs");
}

function permissionMarkup(options = {}) {
  const status = options.status ?? "Waiting for approval";
  const disabled = options.disabled === true ? " disabled" : "";
  const identity =
    options.itemId === null
      ? ""
      : ' data-acp-item-id="' + (options.itemId ?? "acp-1") + '"';
  const actions =
    options.awaiting === false
      ? ""
      : '<div class="acp-permission__actions">' +
        '<button type="button" class="acp-permission__action"' +
        disabled +
        ">Approve</button>" +
        '<button type="button" class="acp-permission__action"' +
        disabled +
        ">Deny</button>" +
        "</div>";
  const detail = options.detail ?? "Write hr6-permission.txt";
  const fillers = "<span></span>".repeat(options.fillers ?? 0);
  const summary =
    options.nested === true
      ? "<div><p>" + detail + "</p></div>"
      : ("<p>" + detail + "</p>").repeat(options.summaries ?? 1);
  return (
    '<article class="acp-permission"' +
    identity +
    ' role="group" aria-label="Permission request">' +
    "<header><strong>" +
    (options.label ?? "Permission request") +
    "</strong>" +
    '<span role="status" aria-label="' +
    status +
    '">' +
    status +
    "</span></header>" +
    fillers +
    summary +
    actions +
    "</article>"
  );
}

function buildDom(cards, options = {}) {
  return new JSDOM(
    '<!doctype html><section class="agent-chat-pane agent-chat-pane--focused">' +
      '<section class="chat-surface" aria-label="Agent chat">' +
      '<section class="acp-turn" data-turn-id="earlier">' +
      '<span data-turn-status aria-label="Completed"></span></section>' +
      '<section class="acp-turn" data-turn-id="' +
      (options.turnId ?? "permission") +
      '"><span data-turn-status aria-label="' +
      (options.turnStatus ?? "Waiting for approval") +
      '"></span>' +
      (options.tools ?? "") +
      cards.join("") +
      "</section>" +
      '<form class="composer"><textarea aria-label="Message agent"></textarea>' +
      "</form></section></section>",
  );
}

function evaluate(dom, code) {
  return new Function("document", "return " + code + ";")(dom.window.document);
}

const EXPECTED = ["hr6-permission.txt"];
const LONG_TOOL_MARKER = "hr6-long-tool-marker";
const REFUSAL_OUTPUT =
  "&lt;tool_use_error&gt;Blocked: Start-Sleep 50&lt;/tool_use_error&gt;";

function toolMarkup(options = {}) {
  const output =
    options.output === null
      ? ""
      : '<pre aria-label="Tool output">' +
        (options.output ?? REFUSAL_OUTPUT) +
        "</pre>";
  return (
    '<article class="acp-tool-card" data-acp-item-id="' +
    (options.itemId ?? "tool-1") +
    '" data-activity-state="' +
    (options.state ?? "failed") +
    '"><header><strong>' +
    (options.label ?? "PowerShell") +
    "</strong></header>" +
    output +
    "</article>"
  );
}

function match(overrides = {}) {
  return {
    itemId: "acp-1",
    label: "Permission request",
    detail: "Write hr6-permission.txt",
    expected: EXPECTED,
    ...overrides,
  };
}

function read(dom, turnId = "permission", expected = EXPECTED) {
  return load().then((module) =>
    evaluate(
      dom,
      module.permissionStateExpression(
        focusedChatSurfacePreamble('return {kind:"scope-lost"};'),
        turnId,
        expected,
      ),
    ),
  );
}

function card(overrides = {}) {
  return {
    itemId: "acp-1",
    label: "Permission request",
    status: "Waiting for approval",
    statusExceeded: false,
    detail: "Write hr6-permission.txt",
    summary: 1,
    summaryExceeded: false,
    actions: 2,
    actionsExceeded: false,
    approve: true,
    deny: true,
    matches: true,
    ...overrides,
  };
}

function state(cards, overrides = {}) {
  return {
    kind: "permissions",
    turnStatus: "Waiting for approval",
    cards,
    exceeded: false,
    refusals: [],
    refusalsExceeded: false,
    ...overrides,
  };
}

test("the permission read projects the exact awaiting card of the identified turn", async () => {
  const module = await load();
  const dom = buildDom([permissionMarkup()]);
  const value = await read(dom);
  assert.deepEqual(value, {
    kind: "permissions",
    turnStatus: "Waiting for approval",
    cards: [
      {
        itemId: "acp-1",
        label: "Permission request",
        status: "Waiting for approval",
        statusExceeded: false,
        detail: "Write hr6-permission.txt",
        summary: 1,
        summaryExceeded: false,
        actions: 2,
        actionsExceeded: false,
        approve: true,
        deny: true,
        matches: true,
      },
    ],
    exceeded: false,
    refusals: [],
    refusalsExceeded: false,
  });
  assert.equal(module.AWAITING_PERMISSION, "Waiting for approval");
});

test("the permission read reports a lost turn rather than another turn's card", async () => {
  const dom = buildDom([permissionMarkup()]);
  assert.deepEqual(await read(dom, "absent"), { kind: "turn-lost" });
});

test("the permission read reports a lost scope when the chat is not focused", async () => {
  const module = await load();
  const dom = buildDom([permissionMarkup()]);
  dom.window.document
    .querySelector(".agent-chat-pane")
    .classList.remove("agent-chat-pane--focused");
  assert.deepEqual(
    evaluate(
      dom,
      module.permissionStateExpression(
        focusedChatSurfacePreamble('return {kind:"scope-lost"};'),
        "permission",
        EXPECTED,
      ),
    ),
    { kind: "scope-lost" },
  );
});

test("a resolved permission card projects no actions and its accessible status", async () => {
  const dom = buildDom([
    permissionMarkup({
      awaiting: false,
      status: "Permission allowed once",
    }),
  ]);
  const value = await read(dom);
  assert.equal(value.cards.length, 1);
  assert.equal(value.cards[0].status, "Permission allowed once");
  assert.equal(value.cards[0].actions, 0);
  assert.equal(value.cards[0].approve, false);
  assert.equal(value.cards[0].deny, false);
});

test("permission readiness waits while no card has been rendered", async () => {
  const module = await load();
  assert.equal(module.permissionReadiness(state([])), null);
});

test("permission readiness reports the single awaiting card", async () => {
  const module = await load();
  const readiness = module.permissionReadiness(state([card()]));
  assert.equal(readiness.kind, "permission-waiting");
  assert.equal(readiness.card.label, "Permission request");
});

test("created-file permission requires the exact current label and actionable gate", async () => {
  const expected = { command: "Created file" };
  const matching = await read(
    buildDom([permissionMarkup({ detail: "Created file" })]),
    "permission",
    expected,
  );
  assert.equal(
    (await load()).permissionReadiness(matching).kind,
    "permission-waiting",
  );
  const wrong = await read(
    buildDom([permissionMarkup({ detail: "Edited file" })]),
    "permission",
    expected,
  );
  assert.equal(
    (await load()).permissionReadiness(wrong).kind,
    "permission-unexpected",
  );
});

test("permission readiness fails closed on two awaiting cards", async () => {
  const module = await load();
  assert.equal(
    module.permissionReadiness(state([card(), card({ label: "Run sleep" })]))
      .kind,
    "permission-ambiguous",
  );
});

test("permission readiness fails closed when the card scan was capped", async () => {
  const module = await load();
  assert.equal(
    module.permissionReadiness(state([card()], { exceeded: true })).kind,
    "permission-scan-exceeded",
  );
});

test("permission readiness fails closed on an unreadable status", async () => {
  const module = await load();
  assert.equal(
    module.permissionReadiness(state([card({ statusExceeded: true })])).kind,
    "permission-unreadable",
  );
});

test("permission readiness reports an absent card once the turn settled", async () => {
  const module = await load();
  assert.equal(
    module.permissionReadiness(state([], { turnStatus: "Completed" })).kind,
    "permission-absent",
  );
  assert.equal(
    module.permissionReadiness(
      state(
        [card({ status: "Declined", actions: 0, approve: false, deny: false })],
        {
          turnStatus: "Completed",
        },
      ),
    ).kind,
    "permission-unresolved",
  );
});

test("a harness refusal of the tool is named apart from a missing permission", async () => {
  const module = await load();
  const refused = module.permissionReadiness(
    state([], {
      turnStatus: "Completed",
      refusals: [{ id: "tool-1", label: "PowerShell" }],
    }),
  );
  assert.equal(refused.kind, "permission-tool-refused");
  assert.deepEqual(refused.tools, [{ id: "tool-1", label: "PowerShell" }]);
  assert.equal(
    module.permissionReadiness(
      state([], {
        refusals: [{ id: "tool-1", label: "PowerShell" }],
      }),
    ),
    null,
  );
});

test("an unreadable refusal scan fails closed instead of reporting absence", async () => {
  const module = await load();
  for (const overrides of [
    { refusalsExceeded: true },
    { refusals: null },
    { refusals: undefined },
  ])
    assert.equal(
      module.permissionReadiness(
        state([], { turnStatus: "Completed", ...overrides }),
      ).kind,
      "permission-tool-unreadable",
    );
});

test("the permission read projects a refused tool card of the identified turn", async () => {
  const module = await load();
  const dom = buildDom([], {
    turnStatus: "Completed",
    tools: toolMarkup() + toolMarkup({ itemId: "tool-2", state: "completed" }),
  });
  const value = await read(dom);
  assert.deepEqual(value.refusals, [{ id: "tool-1", label: "PowerShell" }]);
  assert.equal(value.refusalsExceeded, false);
  assert.equal(
    module.permissionReadiness(value).kind,
    "permission-tool-refused",
  );
});

test("a failed tool without a harness refusal stays a missing permission", async () => {
  const module = await load();
  const dom = buildDom([], {
    turnStatus: "Completed",
    tools: toolMarkup({ output: "command not found" }),
  });
  const value = await read(dom);
  assert.deepEqual(value.refusals, []);
  assert.equal(value.refusalsExceeded, false);
  assert.equal(module.permissionReadiness(value).kind, "permission-absent");
});

test("a failed tool with truncated output is unreadable, never absent", async () => {
  const module = await load();
  const dom = buildDom([], {
    turnStatus: "Completed",
    tools: toolMarkup({ output: "x".repeat(900) }),
  });
  const value = await read(dom);
  assert.deepEqual(value.refusals, []);
  assert.equal(value.refusalsExceeded, true);
  assert.equal(
    module.permissionReadiness(value).kind,
    "permission-tool-unreadable",
  );
});

test("permission readiness passes a scope or turn failure through", async () => {
  const module = await load();
  assert.equal(
    module.permissionReadiness({ kind: "scope-lost" }).kind,
    "scope-lost",
  );
  assert.equal(
    module.permissionReadiness({ kind: "turn-ambiguous" }).kind,
    "turn-ambiguous",
  );
  assert.equal(module.permissionReadiness(null).kind, "read-invalid");
});

test("the permission outcome waits until the card carries the decided status", async () => {
  const module = await load();
  const pending = card({
    status: "Allowing once",
    actions: 0,
    approve: false,
    deny: false,
  });
  assert.equal(
    module.permissionOutcome(state([pending]), "allowOnce", match()),
    null,
  );
  const allowed = card({
    status: "Permission allowed once",
    actions: 0,
    approve: false,
    deny: false,
  });
  const outcome = module.permissionOutcome(
    state([allowed]),
    "allowOnce",
    match(),
  );
  assert.equal(outcome.kind, "permission-terminal");
  assert.equal(outcome.card.status, "Permission allowed once");
});

test("the permission outcome fails closed when the other decision was recorded", async () => {
  const module = await load();
  const declined = card({
    status: "Permission declined",
    actions: 0,
    approve: false,
    deny: false,
  });
  assert.equal(
    module.permissionOutcome(state([declined]), "allowOnce", match()).kind,
    "permission-mismatch",
  );
  assert.equal(
    module.permissionOutcome(state([declined]), "deny", match()).kind,
    "permission-terminal",
  );
});

test("the permission outcome fails closed while the card is still actionable", async () => {
  const module = await load();
  assert.equal(
    module.permissionOutcome(
      state([card({ status: "Permission allowed once" })]),
      "allowOnce",
      match(),
    ).kind,
    "permission-actionable",
  );
  assert.equal(
    module.permissionOutcome(state([]), "allowOnce", match()).kind,
    "permission-absent",
  );
  assert.equal(
    module.permissionOutcome(state([card(), card()]), "allowOnce", match())
      .kind,
    "permission-ambiguous",
  );
  assert.equal(
    module.permissionOutcome(
      state([card()], { kind: "turn-lost" }),
      "allowOnce",
      match(),
    ).kind,
    "turn-lost",
  );
  assert.equal(
    module.permissionOutcome(state([card()]), "maybe").kind,
    "decision-unsupported",
  );
});

test("resolving a permission clicks the exact approve control of the awaiting card", async () => {
  const module = await load();
  const dom = buildDom([permissionMarkup()]);
  const clicks = [];
  for (const button of dom.window.document.querySelectorAll(
    ".acp-permission__action",
  ))
    button.addEventListener("click", () => clicks.push(button.textContent));
  const result = evaluate(
    dom,
    module.resolvePermissionExpression(
      focusedChatSurfacePreamble('return {outcome:"scope-lost"};'),
      "permission",
      "allowOnce",
      match(),
    ),
  );
  assert.deepEqual(result, { outcome: "resolved", action: "Approve" });
  assert.deepEqual(clicks, ["Approve"]);
});

test("resolving a permission clicks the exact deny control of the awaiting card", async () => {
  const module = await load();
  const dom = buildDom([permissionMarkup()]);
  const clicks = [];
  for (const button of dom.window.document.querySelectorAll(
    ".acp-permission__action",
  ))
    button.addEventListener("click", () => clicks.push(button.textContent));
  const result = evaluate(
    dom,
    module.resolvePermissionExpression(
      focusedChatSurfacePreamble('return {outcome:"scope-lost"};'),
      "permission",
      "deny",
      match(),
    ),
  );
  assert.deepEqual(result, { outcome: "resolved", action: "Deny" });
  assert.deepEqual(clicks, ["Deny"]);
});

test("resolving a permission refuses an ambiguous or absent awaiting card", async () => {
  const module = await load();
  const both = buildDom([permissionMarkup(), permissionMarkup()]);
  assert.deepEqual(
    evaluate(
      both,
      module.resolvePermissionExpression(
        focusedChatSurfacePreamble('return {outcome:"scope-lost"};'),
        "permission",
        "allowOnce",
        match(),
      ),
    ),
    { outcome: "permission-ambiguous" },
  );
  const none = buildDom([
    permissionMarkup({ awaiting: false, status: "Permission allowed once" }),
  ]);
  assert.deepEqual(
    evaluate(
      none,
      module.resolvePermissionExpression(
        focusedChatSurfacePreamble('return {outcome:"scope-lost"};'),
        "permission",
        "allowOnce",
        match(),
      ),
    ),
    { outcome: "permission-absent" },
  );
});

test("resolving a permission refuses a disabled control", async () => {
  const module = await load();
  const dom = buildDom([permissionMarkup({ disabled: true })]);
  assert.deepEqual(
    evaluate(
      dom,
      module.resolvePermissionExpression(
        focusedChatSurfacePreamble('return {outcome:"scope-lost"};'),
        "permission",
        "allowOnce",
        match(),
      ),
    ),
    { outcome: "action-unavailable" },
  );
});

test("resolving a permission refuses an ambiguous turn", async () => {
  const module = await load();
  const dom = buildDom([permissionMarkup()]);
  const turn = dom.window.document.querySelector(
    'section.acp-turn[data-turn-id="earlier"]',
  );
  turn.dataset.turnId = "permission";
  assert.deepEqual(
    evaluate(
      dom,
      module.resolvePermissionExpression(
        focusedChatSurfacePreamble('return {outcome:"scope-lost"};'),
        "permission",
        "allowOnce",
        match(),
      ),
    ),
    { outcome: "turn-ambiguous" },
  );
});

const SCOPE = 'return {outcome:"scope-lost"};';

function resolve(module, dom, decision, identity) {
  return evaluate(
    dom,
    module.resolvePermissionExpression(
      focusedChatSurfacePreamble(SCOPE),
      "permission",
      decision,
      identity,
    ),
  );
}

function clickLog(dom) {
  const clicks = [];
  for (const button of dom.window.document.querySelectorAll(
    ".acp-permission__action",
  ))
    button.addEventListener("click", () => clicks.push(button.textContent));
  return clicks;
}

test("the permission read projects the card identity and the expected match", async () => {
  const dom = buildDom([permissionMarkup({ itemId: "acp-77" })]);
  const value = await read(dom);
  assert.equal(value.cards[0].itemId, "acp-77");
  assert.equal(value.cards[0].matches, true);
  const other = buildDom([
    permissionMarkup({ label: "Write notes.txt", detail: "notes.txt" }),
  ]);
  assert.equal((await read(other)).cards[0].matches, false);
  const bare = buildDom([permissionMarkup({ itemId: null })]);
  assert.equal((await read(bare)).cards[0].itemId, null);
});

test("the permission expressions refuse an unusable expectation", async () => {
  const module = await load();
  assert.throws(
    () => module.permissionStateExpression("", "permission", []),
    /hr6-permission-expectation-invalid/u,
  );
  assert.throws(
    () => module.permissionStateExpression("", "permission", [""]),
    /hr6-permission-expectation-invalid/u,
  );
  assert.throws(
    () =>
      module.resolvePermissionExpression("", "permission", "allowOnce", {
        itemId: "acp-1",
        label: "Permission request",
        detail: "Write hr6-permission.txt",
        expected: [],
      }),
    /hr6-permission-expectation-invalid/u,
  );
});

test("permission readiness fails closed on a resolved card beside the awaiting one", async () => {
  const module = await load();
  const resolved = card({
    status: "Permission allowed once",
    actions: 0,
    approve: false,
    deny: false,
  });
  assert.equal(
    module.permissionReadiness(state([resolved, card()])).kind,
    "permission-ambiguous",
  );
});

test("permission readiness fails closed when the card does not name the request", async () => {
  const module = await load();
  assert.equal(
    module.permissionReadiness(state([card({ matches: false })])).kind,
    "permission-unexpected",
  );
  assert.equal(
    module.permissionReadiness(state([card({ matches: undefined })])).kind,
    "permission-unexpected",
  );
});

test("the permission outcome fails closed when the terminal card changed request", async () => {
  const module = await load();
  const allowed = card({
    status: "Permission allowed once",
    actions: 0,
    approve: false,
    deny: false,
    matches: false,
  });
  assert.equal(
    module.permissionOutcome(state([allowed]), "allowOnce", match()).kind,
    "permission-unexpected",
  );
});

test("resolving a permission rechecks the card identity before clicking", async () => {
  const module = await load();
  const dom = buildDom([permissionMarkup({ itemId: "acp-9" })]);
  const clicks = clickLog(dom);
  assert.deepEqual(resolve(module, dom, "allowOnce", match()), {
    outcome: "permission-identity-mismatch",
  });
  assert.deepEqual(clicks, []);
  assert.deepEqual(
    resolve(module, dom, "allowOnce", match({ itemId: "acp-9" })),
    { outcome: "resolved", action: "Approve" },
  );
  assert.deepEqual(clicks, ["Approve"]);
});

test("resolving a permission falls back to the label and detail without an id", async () => {
  const module = await load();
  const dom = buildDom([permissionMarkup({ itemId: null })]);
  const clicks = clickLog(dom);
  assert.deepEqual(
    resolve(
      module,
      dom,
      "allowOnce",
      match({ itemId: null, label: "Write notes.txt" }),
    ),
    { outcome: "permission-identity-mismatch" },
  );
  assert.deepEqual(
    resolve(module, dom, "allowOnce", match({ itemId: null, detail: "other" })),
    { outcome: "permission-identity-mismatch" },
  );
  assert.deepEqual(clicks, []);
  assert.deepEqual(resolve(module, dom, "allowOnce", match({ itemId: null })), {
    outcome: "resolved",
    action: "Approve",
  });
  assert.deepEqual(clicks, ["Approve"]);
});

test("resolving a permission refuses a card that stopped naming the request", async () => {
  const module = await load();
  const dom = buildDom([
    permissionMarkup({ label: "Write notes.txt", detail: "notes.txt" }),
  ]);
  const clicks = clickLog(dom);
  assert.deepEqual(
    resolve(
      module,
      dom,
      "allowOnce",
      match({ label: "Write notes.txt", detail: "notes.txt" }),
    ),
    { outcome: "permission-unexpected" },
  );
  assert.deepEqual(clicks, []);
});

test("resolving a permission refuses a second card in the same turn", async () => {
  const module = await load();
  const dom = buildDom([
    permissionMarkup({
      itemId: "acp-0",
      awaiting: false,
      status: "Permission allowed once",
    }),
    permissionMarkup(),
  ]);
  const clicks = clickLog(dom);
  assert.deepEqual(resolve(module, dom, "allowOnce", match()), {
    outcome: "permission-ambiguous",
  });
  assert.deepEqual(clicks, []);
});

test("a generic expectation fragment is rejected before any card is read", async () => {
  const module = await load();
  for (const fragment of ["command", "Execute", "shell", "sleep", "bash"])
    assert.throws(() => module.permissionCardsProjection([fragment]), {
      message: "hr6-permission-expectation-generic",
    });
  assert.throws(
    () => module.permissionCardsProjection([LONG_TOOL_MARKER, "echo"]),
    { message: "hr6-permission-expectation-generic" },
  );
});

test("every expected fragment must appear in the card, not just one", async () => {
  const dom = buildDom([
    permissionMarkup({
      label: "Execute command",
      detail: "Start-Sleep -Seconds 50; Write-Output " + LONG_TOOL_MARKER,
    }),
  ]);
  const both = await read(dom, "permission", [
    LONG_TOOL_MARKER,
    "hr6-permission.txt",
  ]);
  assert.equal(both.cards[0].matches, false);
  const single = await read(dom, "permission", [LONG_TOOL_MARKER]);
  assert.equal(single.cards[0].matches, true);
});

test("the terminal card must be the card that was approved", async () => {
  const module = await load();
  const allowed = card({
    status: "Permission allowed once",
    actions: 0,
    approve: false,
    deny: false,
  });
  assert.equal(
    module.permissionOutcome(state([allowed]), "allowOnce", match()).kind,
    "permission-terminal",
  );
  assert.equal(
    module.permissionOutcome(
      state([allowed]),
      "allowOnce",
      match({ itemId: "acp-2" }),
    ).kind,
    "permission-identity-mismatch",
  );
  assert.equal(
    module.permissionOutcome(
      state([{ ...allowed, itemId: null }]),
      "allowOnce",
      match({ itemId: null, label: "Write other.txt" }),
    ).kind,
    "permission-identity-mismatch",
  );
});

test("an outcome without an approved identity fails closed", async () => {
  const module = await load();
  assert.throws(() => module.permissionOutcome(state([card()]), "allowOnce"), {
    message: "hr6-permission-identity-invalid",
  });
});

const LONG_TOOL_COMMAND =
  "Start-Sleep -Seconds 50; Write-Output " + LONG_TOOL_MARKER;

function commandDom(detail) {
  return buildDom([permissionMarkup({ label: "Execute command", detail })]);
}

function commandIdentity(detail) {
  return {
    itemId: "acp-1",
    label: "Execute command",
    detail,
    expected: { command: LONG_TOOL_COMMAND },
  };
}

test("an exact command expectation accepts only that command", async () => {
  const exact = await read(commandDom(LONG_TOOL_COMMAND), "permission", {
    command: LONG_TOOL_COMMAND,
  });
  assert.equal(exact.cards[0].matches, true);
  const spaced = await read(
    commandDom(
      "  Start-Sleep -Seconds 50;\n\tWrite-Output " + LONG_TOOL_MARKER,
    ),
    "permission",
    { command: LONG_TOOL_COMMAND },
  );
  assert.equal(spaced.cards[0].matches, true);
});

test("an injected extra statement is refused for an exact command", async () => {
  const injections = [
    "Write-Output WRONG; " + LONG_TOOL_COMMAND,
    LONG_TOOL_COMMAND + "; Remove-Item alpha.txt",
    LONG_TOOL_COMMAND + " -Force",
    "Start-Sleep -Seconds 5; Write-Output " + LONG_TOOL_MARKER,
    LONG_TOOL_MARKER,
  ];
  for (const detail of injections) {
    const value = await read(commandDom(detail), "permission", {
      command: LONG_TOOL_COMMAND,
    });
    assert.equal(value.cards[0].matches, false, detail);
  }
});

test("an injected command is never approved by the resolve click", async () => {
  const module = await load();
  const detail = "Write-Output WRONG; " + LONG_TOOL_COMMAND;
  const dom = commandDom(detail);
  const clicks = clickLog(dom);
  assert.deepEqual(resolve(module, dom, "allowOnce", commandIdentity(detail)), {
    outcome: "permission-unexpected",
  });
  assert.deepEqual(clicks, []);
  const good = commandDom(LONG_TOOL_COMMAND);
  const goodClicks = clickLog(good);
  assert.deepEqual(
    resolve(module, good, "allowOnce", commandIdentity(LONG_TOOL_COMMAND)),
    { outcome: "resolved", action: "Approve" },
  );
  assert.deepEqual(goodClicks, ["Approve"]);
});

test("a command too long to read whole fails closed", async () => {
  const long = "hr6 " + "a".repeat(600);
  const value = await read(commandDom(long), "permission", { command: long });
  assert.equal(value.cards[0].matches, false);
});

test("an unusable exact command expectation is rejected", async () => {
  const module = await load();
  for (const command of ["echo", "sleep", "Execute", "Get-Date", "pwsh -c ls"])
    assert.throws(() => module.permissionCardsProjection({ command }), {
      message: "hr6-permission-expectation-generic",
    });
  for (const expected of [{ command: 12 }, { command: "" }, {}, "hr6-x"])
    assert.throws(() => module.permissionCardsProjection(expected), {
      message: "hr6-permission-expectation-invalid",
    });
});

test("an exact command expectation needs length, not the hr6 anchor", async () => {
  const module = await load();
  const command = "Start-Sleep -Seconds 50";
  const dom = buildDom([permissionMarkup({ detail: command })]);
  const value = await read(dom, "permission", { command });
  assert.equal(value.cards[0].matches, true);
  const near = await read(
    buildDom([permissionMarkup({ detail: "Start-Sleep -Seconds 5" })]),
    "permission",
    { command },
  );
  assert.equal(near.cards[0].matches, false);
  assert.throws(() => module.permissionCardsProjection([command]), {
    message: "hr6-permission-expectation-generic",
  });
});

test("a bounded condition wait is accepted exactly and its near misses are not", async () => {
  const release = "hr6-release.txt";
  const command =
    "$hr6Deadline=(Get-Date).AddSeconds(90); while (-not (Test-Path -LiteralPath " +
    release +
    ") -and (Get-Date) -lt $hr6Deadline) { Start-Sleep -Seconds 2 }";
  const exact = await read(commandDom(command), "permission", { command });
  assert.equal(exact.cards[0].matches, true);
  const spaced = await read(
    commandDom(command.replace("; while", ";\n   while")),
    "permission",
    { command },
  );
  assert.equal(spaced.cards[0].matches, true);
  for (const detail of [
    command.replace("AddSeconds(90)", "AddSeconds(9)"),
    command.replace(" -LiteralPath", ""),
    command.replace("Start-Sleep -Seconds 2", "Start-Sleep -Seconds 50"),
    command + "; Write-Output done",
    "Start-Job { " + command + " }",
    command.replace("-and (Get-Date) -lt $hr6Deadline", ""),
  ]) {
    const miss = await read(commandDom(detail), "permission", { command });
    assert.equal(miss.cards[0].matches, false, detail);
  }
});

test("the rendered write title is accepted exactly and near misses are not", async () => {
  const title = "Write hr6-permission.txt";
  const exact = await read(buildDom([permissionMarkup()]), "permission", {
    command: title,
  });
  assert.equal(exact.cards[0].matches, true);
  for (const detail of [
    title + ".bak",
    title + " -Force",
    "Write other.txt; " + title,
  ]) {
    const near = await read(
      buildDom([permissionMarkup({ detail })]),
      "permission",
      {
        command: title,
      },
    );
    assert.equal(near.cards[0].matches, false, detail);
    const loose = await read(buildDom([permissionMarkup({ detail })]));
    assert.equal(loose.cards[0].matches, true, detail);
  }
});

test("the permission read takes the summary from the direct child paragraph", async () => {
  const value = await read(buildDom([permissionMarkup()]));
  assert.equal(value.cards[0].detail, "Write hr6-permission.txt");
  assert.equal(value.cards[0].label, "Permission request");
  assert.equal(value.cards[0].summary, 1);
  assert.equal(value.cards[0].summaryExceeded, false);
  assert.equal(value.cards[0].matches, true);
  const nested = (await read(buildDom([permissionMarkup({ nested: true })])))
    .cards[0];
  assert.equal(nested.detail, null);
  assert.equal(nested.summary, 0);
  assert.equal(nested.matches, false);
});

test("a card without exactly one summary paragraph is unreadable", async () => {
  const module = await load();
  for (const summaries of [0, 2]) {
    const value = await read(buildDom([permissionMarkup({ summaries })]));
    assert.equal(value.cards[0].summary, summaries);
    assert.equal(value.cards[0].detail, null);
    assert.equal(value.cards[0].matches, false);
    assert.equal(
      module.permissionReadiness(value).kind,
      "permission-unreadable",
    );
    assert.equal(
      module.permissionOutcome(value, "allowOnce", match()).kind,
      "permission-unreadable",
    );
  }
});

test("a card with more children than the scan bound is unreadable", async () => {
  const module = await load();
  const value = await read(buildDom([permissionMarkup({ fillers: 16 })]));
  assert.equal(value.cards[0].summaryExceeded, true);
  assert.equal(value.cards[0].detail, null);
  assert.equal(module.permissionReadiness(value).kind, "permission-unreadable");
});

test("a summary too long to read whole is unreadable", async () => {
  const module = await load();
  const value = await read(
    buildDom([permissionMarkup({ detail: "hr6-" + "b".repeat(600) })]),
  );
  assert.equal(value.cards[0].summaryExceeded, true);
  assert.equal(value.cards[0].matches, false);
  assert.equal(module.permissionReadiness(value).kind, "permission-unreadable");
});

test("resolving a permission refuses an unreadable summary", async () => {
  const module = await load();
  for (const options of [
    { summaries: 0 },
    { summaries: 2 },
    { nested: true },
    { fillers: 16 },
    { detail: "hr6-" + "b".repeat(600) },
  ]) {
    const dom = buildDom([permissionMarkup(options)]);
    const clicks = clickLog(dom);
    assert.deepEqual(
      resolve(module, dom, "allowOnce", match()),
      { outcome: "permission-unreadable" },
      JSON.stringify(options),
    );
    assert.deepEqual(clicks, []);
  }
});
