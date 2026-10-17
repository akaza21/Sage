import assert from "node:assert/strict";
import test from "node:test";
import { JSDOM } from "jsdom";

import { focusedChatSurfacePreamble } from "./hr6-chat-scope.mjs";

function load() {
  return import("./hr6-access-mode.mjs");
}

function option(id, label, reason) {
  return (
    '<button type="button" role="menuitemradio" class="searchable-picker__option" data-option-id="' +
    id +
    '"' +
    (reason ? ' aria-disabled="true" disabled' : "") +
    "><span>" +
    label +
    "</span>" +
    (reason
      ? '<span class="searchable-picker__reason">' + reason + "</span>"
      : "") +
    "</button>"
  );
}

function pickerMarkup(options = {}) {
  const selected = options.selectedId ?? "readOnly";
  const label = options.label ?? "Read-only";
  const expanded = options.expanded === true;
  const disabled = options.disabled === true ? " disabled" : "";
  const reason = options.reason
    ? '<span class="composer-access-error" role="alert">' +
      options.reason +
      "</span>"
    : "";
  const list = expanded
    ? '<div id="access-listbox" role="menu">' +
      (
        options.options ?? [
          option("readOnly", "Read-only"),
          option("ask", "Ask each time"),
          option("workspaceWrite", "Workspace edits"),
        ]
      ).join("") +
      "</div>"
    : "";
  return (
    '<label class="composer__controls">' +
    '<div class="searchable-picker"><button type="button" class="composer__access-label" aria-label="Access options" aria-expanded="' +
    String(expanded) +
    '" aria-controls="access-listbox" data-selected-id="' +
    selected +
    '"' +
    disabled +
    '><span class="composer__access-text">' +
    label +
    "</span></button></div>" +
    reason +
    "</label>" +
    list
  );
}

function buildDom(markup) {
  return new JSDOM(
    '<!doctype html><section class="agent-chat-pane agent-chat-pane--focused">' +
      '<section class="chat-surface" aria-label="Agent chat">' +
      '<form class="composer"><textarea aria-label="Message agent"></textarea>' +
      markup +
      "</form></section></section>",
  );
}

function evaluate(dom, code) {
  return new Function("document", "return " + code + ";")(dom.window.document);
}

function preamble() {
  return focusedChatSurfacePreamble('return {kind:"scope-lost"};');
}

function readState(dom) {
  return load().then((module) =>
    evaluate(dom, module.accessModeStateExpression(preamble())),
  );
}

test("the permission mode chip is projected from the composer", async () => {
  const dom = buildDom(pickerMarkup());
  assert.deepEqual(await readState(dom), {
    kind: "access-mode",
    selectedId: "readOnly",
    label: "Read-only",
    expanded: false,
    disabled: false,
    reason: null,
    triggers: 1,
    triggersExceeded: false,
  });
});

test("a missing chip fails closed", async () => {
  const dom = buildDom("");
  const module = await load();
  assert.deepEqual(module.accessModeReadiness(await readState(dom), "ask"), {
    kind: "access-mode-absent",
  });
});

test("a lost chat scope is reported rather than treated as absent", async () => {
  const dom = new JSDOM("<!doctype html><main></main>");
  assert.deepEqual(await readState(dom), { kind: "scope-lost" });
});

test("two chips fail closed as ambiguous", async () => {
  const dom = buildDom(pickerMarkup() + pickerMarkup());
  const module = await load();
  const state = await readState(dom);
  assert.equal(state.triggers, 2);
  assert.deepEqual(module.accessModeReadiness(state, "ask"), {
    kind: "access-mode-ambiguous",
  });
});

test("a disabled chip fails closed", async () => {
  const dom = buildDom(pickerMarkup({ disabled: true }));
  const module = await load();
  assert.deepEqual(module.accessModeReadiness(await readState(dom), "ask"), {
    kind: "access-mode-disabled",
  });
});

test("a chip error is surfaced rather than ignored", async () => {
  const dom = buildDom(pickerMarkup({ reason: "Could not save the mode" }));
  const module = await load();
  assert.deepEqual(module.accessModeReadiness(await readState(dom), "ask"), {
    kind: "access-mode-error",
    reason: "Could not save the mode",
  });
});

test("the wrong mode is a mismatch and the right mode is ready", async () => {
  const module = await load();
  const wrong = await readState(dom0());
  assert.deepEqual(module.accessModeReadiness(wrong, "ask"), {
    kind: "access-mode-mismatch",
    selectedId: "readOnly",
    label: "Read-only",
  });
  const right = await readState(
    buildDom(pickerMarkup({ selectedId: "ask", label: "Ask each time" })),
  );
  assert.equal(module.accessModeReadiness(right, "ask"), null);
});

function dom0() {
  return buildDom(pickerMarkup());
}

test("a selected id without its rendered label fails closed", async () => {
  const module = await load();
  const state = await readState(
    buildDom(pickerMarkup({ selectedId: "ask", label: "Read-only" })),
  );
  assert.deepEqual(module.accessModeReadiness(state, "ask"), {
    kind: "access-mode-mismatch",
    selectedId: "ask",
    label: "Read-only",
  });
});

test("an unsupported mode is rejected before any click", async () => {
  const module = await load();
  assert.throws(() => module.accessModeReadiness(null, "bypass"), {
    message: "hr6-access-mode-unsupported",
  });
  assert.throws(() => module.chooseAccessModeExpression(preamble(), "bypass"), {
    message: "hr6-access-mode-unsupported",
  });
});

test("opening the picker clicks the trigger once and reports expansion", async () => {
  const dom = buildDom(pickerMarkup());
  const module = await load();
  const trigger = dom.window.document.querySelector(".composer__access-label");
  let clicks = 0;
  trigger.addEventListener("click", () => {
    clicks += 1;
    trigger.setAttribute("aria-expanded", "true");
  });
  assert.equal(
    evaluate(dom, module.openAccessModeExpression(preamble())),
    true,
  );
  assert.equal(clicks, 1);
  assert.equal(
    evaluate(dom, module.accessModeExpandedExpression(preamble())),
    true,
  );
});

test("the target option is clicked exactly once", async () => {
  const dom = buildDom(pickerMarkup({ expanded: true }));
  const module = await load();
  const options = [
    ...dom.window.document.querySelectorAll('[role="menuitemradio"]'),
  ];
  const clicked = [];
  for (const node of options)
    node.addEventListener("click", () =>
      clicked.push(node.getAttribute("data-option-id")),
    );
  assert.deepEqual(
    evaluate(dom, module.chooseAccessModeExpression(preamble(), "ask")),
    { outcome: "selected", id: "ask", label: "Ask each time" },
  );
  assert.deepEqual(clicked, ["ask"]);
});

test("a disabled option is never clicked", async () => {
  const dom = buildDom(
    pickerMarkup({
      expanded: true,
      options: [
        option("readOnly", "Read-only"),
        option(
          "ask",
          "Ask each time",
          "This harness has not confirmed support for permission prompts.",
        ),
      ],
    }),
  );
  const module = await load();
  let clicks = 0;
  for (const node of dom.window.document.querySelectorAll(
    '[role="menuitemradio"]',
  ))
    node.addEventListener("click", () => {
      clicks += 1;
    });
  assert.deepEqual(
    evaluate(dom, module.chooseAccessModeExpression(preamble(), "ask")),
    {
      outcome: "option-unavailable",
      reason: "This harness has not confirmed support for permission prompts.",
    },
  );
  assert.equal(clicks, 0);
});

test("a label that no longer matches the mode id is never clicked", async () => {
  const dom = buildDom(
    pickerMarkup({
      expanded: true,
      options: [option("ask", "Ask sometimes")],
    }),
  );
  const module = await load();
  let clicks = 0;
  for (const node of dom.window.document.querySelectorAll(
    '[role="menuitemradio"]',
  ))
    node.addEventListener("click", () => {
      clicks += 1;
    });
  assert.deepEqual(
    evaluate(dom, module.chooseAccessModeExpression(preamble(), "ask")),
    { outcome: "option-unexpected", label: "Ask sometimes" },
  );
  assert.equal(clicks, 0);
});

test("a duplicated option fails closed without clicking", async () => {
  const dom = buildDom(
    pickerMarkup({
      expanded: true,
      options: [option("ask", "Ask each time"), option("ask", "Ask each time")],
    }),
  );
  const module = await load();
  let clicks = 0;
  for (const node of dom.window.document.querySelectorAll(
    '[role="menuitemradio"]',
  ))
    node.addEventListener("click", () => {
      clicks += 1;
    });
  assert.deepEqual(
    evaluate(dom, module.chooseAccessModeExpression(preamble(), "ask")),
    { outcome: "option-ambiguous", matches: 2 },
  );
  assert.equal(clicks, 0);
});

test("a missing option is reported rather than silently ignored", async () => {
  const dom = buildDom(
    pickerMarkup({
      expanded: true,
      options: [option("readOnly", "Read-only")],
    }),
  );
  const module = await load();
  assert.deepEqual(
    evaluate(dom, module.chooseAccessModeExpression(preamble(), "ask")),
    { outcome: "option-absent" },
  );
});

test("the approval surfacing mode is ask each time", async () => {
  const module = await load();
  assert.equal(module.APPROVAL_ACCESS_MODE, "ask");
  assert.deepEqual(module.accessModeLabels(), {
    readOnly: "Read-only",
    ask: "Ask each time",
    workspaceWrite: "Workspace edits",
  });
});

test("a ready mode read that lands past the deadline is refused", async () => {
  const module = await load();
  const right = await readState(
    buildDom(pickerMarkup({ selectedId: "ask", label: "Ask each time" })),
  );
  assert.equal(module.accessModeSettled(right, "ask", 1000, 2000), null);
  assert.deepEqual(module.accessModeSettled(right, "ask", 2000, 2000), {
    kind: "access-mode-late",
  });
  assert.deepEqual(module.accessModeSettled(right, "ask", 2001, 2000), {
    kind: "access-mode-late",
  });
});

test("a late read is refused before any mismatch is reported", async () => {
  const module = await load();
  const wrong = await readState(dom0());
  assert.deepEqual(module.accessModeSettled(wrong, "ask", 5000, 2000), {
    kind: "access-mode-late",
  });
  assert.deepEqual(module.accessModeSettled(wrong, "ask", 1000, 2000), {
    kind: "access-mode-mismatch",
    selectedId: "readOnly",
    label: "Read-only",
  });
});

test("an unusable clock is treated as late rather than ready", async () => {
  const module = await load();
  const right = await readState(
    buildDom(pickerMarkup({ selectedId: "ask", label: "Ask each time" })),
  );
  for (const clock of [
    [Number.NaN, 2000],
    [1000, Number.NaN],
    [1000, Number.POSITIVE_INFINITY],
  ])
    assert.deepEqual(
      module.accessModeSettled(right, "ask", clock[0], clock[1]),
      {
        kind: "access-mode-late",
      },
    );
});
