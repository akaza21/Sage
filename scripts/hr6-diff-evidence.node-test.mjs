import assert from "node:assert/strict";
import test from "node:test";
import { JSDOM } from "jsdom";

import { focusedChatSurfacePreamble } from "./hr6-chat-scope.mjs";

function load() {
  return import("./hr6-diff-evidence.mjs");
}

function chipMarkup(options = {}) {
  return (
    '<span class="chat-file-chip">' +
    '<span class="chat-file-chip__name">' +
    (options.name ?? "hr6-permission.txt") +
    "</span>" +
    '<span class="chat-file-chip__root">' +
    (options.root ?? "scratch") +
    "</span>" +
    '<span class="tab__count tab__count--add">' +
    (options.additions ?? "+1") +
    "</span>" +
    '<span class="tab__count tab__count--del">' +
    (options.deletions ?? "-0") +
    "</span></span>"
  );
}

function entryMarkup(options = {}) {
  const chip = chipMarkup(options);
  const disabled = options.disabled === true ? " disabled" : "";
  const action =
    options.openable === false
      ? chip
      : '<button type="button" class="acp-resource-action" aria-label="' +
        (options.actionLabel ?? "Open diff scratch / hr6-permission.txt") +
        '"' +
        disabled +
        ">" +
        chip +
        "</button>";
  return (
    "<li>" +
    action +
    '<span class="acp-change-operation">' +
    (options.operation ?? "created") +
    "</span><span>" +
    (options.summary ?? "1 addition") +
    "</span></li>"
  );
}

function sectionMarkup(entries, options = {}) {
  return sectionBody(entries, options);
}

function countMarkup(sign, value) {
  return "<span>" + sign + "<!-- -->" + value + "</span>";
}

function sectionBody(entries, options) {
  return (
    '<section class="acp-file-changes" aria-label="Files edited in turn ' +
    (options.ordinal ?? 1) +
    '" data-session-attempt="' +
    (options.attempt ?? "attempt-1") +
    '"><button class="acp-file-changes__header" type="button" aria-expanded="true"><strong>' +
    (options.label ?? "Edited 1 file") +
    '</strong><span class="acp-file-changes__totals" aria-label="Line changes">' +
    (options.rawAdditions ?? countMarkup("+", options.additions ?? 1)) +
    (options.rawDeletions ?? countMarkup("-", options.deletions ?? 0)) +
    "</span></button><ul>" +
    entries.join("") +
    "</ul></section>"
  );
}

function buildDom(sections, options = {}) {
  return new JSDOM(
    '<!doctype html><section class="agent-chat-pane agent-chat-pane--focused">' +
      '<section class="chat-surface" aria-label="Agent chat">' +
      '<section class="acp-turn" data-turn-id="earlier">' +
      '<span data-turn-status aria-label="Completed"></span></section>' +
      '<section class="acp-turn" data-turn-id="' +
      (options.turnId ?? "write") +
      '"><span data-turn-status aria-label="' +
      (options.turnStatus ?? "Completed") +
      '"></span>' +
      sections.join("") +
      "</section>" +
      '<form class="composer"><textarea aria-label="Message agent"></textarea>' +
      "</form></section></section>",
  );
}

function evaluate(dom, code) {
  return new Function("document", "return " + code + ";")(dom.window.document);
}

function read(dom, turnId = "write") {
  return load().then((module) =>
    evaluate(
      dom,
      module.fileChangesExpression(
        focusedChatSurfacePreamble('return {kind:"scope-lost"};'),
        turnId,
      ),
    ),
  );
}

function entry(overrides = {}) {
  return {
    action: "Open diff scratch / hr6-permission.txt",
    openable: true,
    name: "hr6-permission.txt",
    root: "scratch",
    additions: "+1",
    deletions: "-0",
    operation: "created",
    ...overrides,
  };
}

function section(entries, overrides = {}) {
  return {
    label: "Edited 1 file",
    additions: "+1",
    deletions: "-0",
    entries,
    entriesExceeded: false,
    ...overrides,
  };
}

function state(sections, overrides = {}) {
  return {
    kind: "file-changes",
    turnStatus: "Completed",
    sections,
    exceeded: false,
    ...overrides,
  };
}

const EXPECTED = {
  relativePath: "hr6-permission.txt",
  rootName: "scratch",
  additions: 1,
  deletions: 0,
  operation: "created",
};

function diffMarkup(options = {}) {
  const path = options.path ?? "hr6-permission.txt";
  const lines = (options.lines ?? ["hr6 permission granted"])
    .map(
      (text, index) =>
        '<div class="diff-line diff-line--added" role="row"><span role="cell"></span><span role="cell">' +
        (index + 1) +
        '</span><div class="diff-line__comment-gutter" role="cell"></div><code role="cell">+' +
        text +
        "</code></div>",
    )
    .join("");
  const notice =
    options.notice === "loading"
      ? '<div class="resource-state resource-state--loading" role="status"><strong class="resource-state__title">Loading diff</strong></div>'
      : options.notice === "error"
        ? '<div class="resource-state resource-state--error" role="alert"><strong class="resource-state__title">Could not load this diff</strong></div>'
        : "";
  const editor =
    options.notice === "loading" || options.notice === "error"
      ? ""
      : '<div class="diff-editor" role="table" aria-label="Unified file diff"><div class="diff-hunk"><div class="diff-hunk__header" role="row">@@ -0,0 +1 @@</div>' +
        (options.context === true
          ? '<div class="diff-line diff-line--context" role="row"><span role="cell">1</span><span role="cell">1</span><div class="diff-line__comment-gutter" role="cell"></div><code role="cell"> existing</code></div>'
          : "") +
        (options.removed ?? [])
          .map(
            (text, index) =>
              '<div class="diff-line diff-line--removed" role="row"><span role="cell">' +
              (index + 1) +
              '</span><span role="cell"></span><div class="diff-line__comment-gutter" role="cell"></div><code role="cell">-' +
              text +
              "</code></div>",
          )
          .join("") +
        lines +
        "</div>" +
        (options.truncated === true
          ? '<p class="diff-limit">Diff capped at 1,200 lines to protect UI responsiveness.</p>'
          : "") +
        "</div>";
  return (
    '<section class="diff-surface resource-document" aria-label="Diff for ' +
    path +
    '"><header class="diff-header"><div class="diff-header__path"><span>' +
    path +
    '</span></div><div class="diff-header__counts">' +
    (options.headerCounts ?? ["+1", "-0"])
      .map((count) => "<span>" + count + "</span>")
      .join("") +
    "</div></header>" +
    '<div class="diff-toolbar"><span>Saved agent change</span></div>' +
    notice +
    editor +
    "</section>"
  );
}

function artifactHeaderMarkup(options = {}) {
  const path = options.path ?? "hr6-permission.txt";
  const counts = options.artifactCounts ?? ["+1", "-0"];
  const additions =
    counts[0] === undefined
      ? ""
      : '<span class="agent-artifact-header__additions">' +
        counts[0] +
        "</span>";
  const deletions =
    counts[1] === undefined
      ? ""
      : '<span class="agent-artifact-header__deletions">' +
        counts[1] +
        "</span>";
  return (
    '<header class="agent-artifact-header"><span class="agent-artifact-header__provenance">Opened by Sage</span>' +
    '<div class="agent-artifact-header__title-row"><div class="agent-artifact-header__identity">' +
    '<span class="agent-artifact-header__kind">Diff</span><strong title="' +
    path +
    '">' +
    path +
    "</strong></div>" +
    '<span class="agent-artifact-header__view-only">VIEW ONLY</span></div>' +
    '<div class="agent-artifact-header__path-row"><code title="scratch/' +
    path +
    '">scratch/' +
    path +
    "</code>" +
    additions +
    (options.duplicateArtifactCounts === true
      ? '<span class="agent-artifact-header__additions">+7</span>'
      : "") +
    deletions +
    '<button class="agent-artifact-header__copy" type="button" aria-label="Copy path"></button></div></header>'
  );
}

function artifactTabsMarkup(options = {}) {
  const path = options.path ?? "hr6-permission.txt";
  return (
    '<div class="agent-artifact-tabs" role="tablist"><button class="tab tab--active" role="tab" type="button" title="scratch/' +
    path +
    '">' +
    '<span class="agent-artifact-tab__kind" aria-hidden="true">Diff</span>' +
    '<span class="tab__path" aria-hidden="true"><span class="tab__dir">scratch</span><span class="tab__basename">' +
    path +
    "</span></span>" +
    '<span class="tab__count tab__count--add">+1</span><span class="tab__count tab__count--del">-0</span></button></div>'
  );
}

function paneDom(options = {}) {
  const body = options.body ?? diffMarkup(options);
  const panes =
    options.collapsed === true
      ? '<aside class="agent-artifact-pane agent-artifact-pane--collapsed" aria-label="Agent artifacts"><button class="agent-artifact-pane__edge-action" type="button" aria-label="Open files, changes, and preview"></button></aside>'
      : '<aside class="agent-artifact-pane agent-artifact-pane--with-header" aria-label="Agent artifacts"><header class="agent-artifact-pane__header">' +
        artifactTabsMarkup(options) +
        "</header>" +
        artifactHeaderMarkup(options) +
        '<div class="agent-artifact-pane__canvas">' +
        body +
        "</div></aside>";
  return new JSDOM(
    "<!doctype html><main>" +
      panes +
      (options.duplicate === true ? panes : "") +
      "</main>",
  );
}

function surfaceRead(dom) {
  return load().then((module) => evaluate(dom, module.diffSurfaceExpression()));
}

test("the file change read projects the exact entry of the identified turn", async () => {
  const dom = buildDom([sectionMarkup([entryMarkup()])]);
  assert.deepEqual(await read(dom), {
    kind: "file-changes",
    turnStatus: "Completed",
    sections: [
      {
        label: "Edited 1 file",
        additions: "+1",
        deletions: "-0",
        entries: [
          {
            action: "Open diff scratch / hr6-permission.txt",
            openable: true,
            name: "hr6-permission.txt",
            root: "scratch",
            additions: "+1",
            deletions: "-0",
            operation: "created",
          },
        ],
        entriesExceeded: false,
      },
    ],
    exceeded: false,
  });
});

test("the file change read reports a lost turn rather than another turn summary", async () => {
  const dom = buildDom([sectionMarkup([entryMarkup()])]);
  assert.deepEqual(await read(dom, "absent"), { kind: "turn-lost" });
});

test("the section totals are read whole when the header splits sign and digits", async () => {
  const dom = buildDom([sectionMarkup([entryMarkup()], { additions: 12 })]);
  const totals = dom.window.document.querySelectorAll(
    ".acp-file-changes__totals span",
  );
  assert.equal(totals.length, 2);
  assert.equal(totals[0].childNodes.length, 3);
  assert.equal(totals[0].textContent, "+12");
  const value = await read(dom);
  assert.equal(value.sections[0].additions, "+12");
  assert.equal(value.sections[0].deletions, "-0");
});

test("a section total that is not a sign and digits is unreadable", async () => {
  const module = await load();
  for (const raw of [
    "<span>+1 lines</span>",
    "<span>++1</span>",
    "<span>1</span>",
    "<span>+</span>",
    "<span>+12345678</span>",
    "<span>+<!-- -->1<!-- -->2</span>",
    "<span>—</span>",
  ]) {
    const value = await read(
      buildDom([sectionMarkup([entryMarkup()], { rawAdditions: raw })]),
    );
    assert.equal(value.sections[0].additions, null, raw);
    assert.equal(
      module.fileChangeOutcome(value, EXPECTED).kind,
      "file-change-unreadable",
      raw,
    );
  }
});

test("a section that renders one or three totals is unreadable", async () => {
  const module = await load();
  for (const options of [
    { rawDeletions: "" },
    { rawDeletions: countMarkup("-", 0) + countMarkup("-", 0) },
  ]) {
    const value = await read(
      buildDom([sectionMarkup([entryMarkup()], options)]),
    );
    assert.equal(value.sections[0].additions, null);
    assert.equal(value.sections[0].deletions, null);
    assert.equal(
      module.fileChangeOutcome(value, EXPECTED).kind,
      "file-change-unreadable",
    );
  }
});

test("the file change read reports a lost scope when the chat is not focused", async () => {
  const module = await load();
  const dom = buildDom([sectionMarkup([entryMarkup()])]);
  dom.window.document
    .querySelector(".agent-chat-pane")
    .classList.remove("agent-chat-pane--focused");
  assert.deepEqual(
    evaluate(
      dom,
      module.fileChangesExpression(
        focusedChatSurfacePreamble('return {kind:"scope-lost"};'),
        "write",
      ),
    ),
    { kind: "scope-lost" },
  );
});

test("the file change read marks an entry without an open control unopenable", async () => {
  const dom = buildDom([sectionMarkup([entryMarkup({ openable: false })])]);
  const value = await read(dom);
  assert.equal(value.sections[0].entries[0].openable, false);
  assert.equal(value.sections[0].entries[0].action, null);
  assert.equal(value.sections[0].entries[0].name, "hr6-permission.txt");
});

test("the change location splits the root label from the relative path", async () => {
  const module = await load();
  assert.deepEqual(
    module.changeLocation("Open diff scratch / nested/hr6.txt"),
    {
      root: "scratch",
      relativePath: "nested/hr6.txt",
    },
  );
  assert.equal(module.changeLocation("Collapse resources"), null);
  assert.equal(module.changeLocation(null), null);
});

test("the file change outcome accepts the exact path, root and added count", async () => {
  const module = await load();
  const outcome = module.fileChangeOutcome(
    state([section([entry()])]),
    EXPECTED,
  );
  assert.equal(outcome.kind, "file-change");
  assert.equal(outcome.entry.name, "hr6-permission.txt");
});

test("the file change outcome waits while the turn has not settled", async () => {
  const module = await load();
  assert.equal(
    module.fileChangeOutcome(state([], { turnStatus: "Working" }), EXPECTED),
    null,
  );
  assert.equal(
    module.fileChangeOutcome(state([], { turnStatus: "Completed" }), EXPECTED)
      .kind,
    "file-change-absent",
  );
});

test("the file change outcome fails closed on ambiguous sections or entries", async () => {
  const module = await load();
  assert.equal(
    module.fileChangeOutcome(
      state([section([entry()]), section([entry()])]),
      EXPECTED,
    ).kind,
    "file-change-ambiguous",
  );
  assert.equal(
    module.fileChangeOutcome(state([section([entry(), entry()])]), EXPECTED)
      .kind,
    "file-change-ambiguous",
  );
});

test("the file change outcome fails closed when a scan was capped", async () => {
  const module = await load();
  assert.equal(
    module.fileChangeOutcome(
      state([section([entry()])], { exceeded: true }),
      EXPECTED,
    ).kind,
    "file-change-scan-exceeded",
  );
  assert.equal(
    module.fileChangeOutcome(
      state([section([entry()], { entriesExceeded: true })]),
      EXPECTED,
    ).kind,
    "file-change-unreadable",
  );
  assert.equal(
    module.fileChangeOutcome(
      state([section([entry()], { additions: null })]),
      EXPECTED,
    ).kind,
    "file-change-unreadable",
  );
});

test("the file change outcome fails closed on a mismatched count, root or operation", async () => {
  const module = await load();
  for (const overrides of [
    { additions: "+2" },
    { deletions: "-1" },
    { root: "workspace" },
    { name: "other.txt" },
    { operation: "modified" },
    { action: "Open diff workspace / hr6-permission.txt" },
  ]) {
    assert.equal(
      module.fileChangeOutcome(state([section([entry(overrides)])]), EXPECTED)
        .kind,
      "file-change-mismatch",
      JSON.stringify(overrides),
    );
  }
});

test("the file change outcome fails closed when the entry cannot be opened", async () => {
  const module = await load();
  assert.equal(
    module.fileChangeOutcome(
      state([section([entry({ openable: false })])], {
        turnStatus: "Completed",
      }),
      EXPECTED,
    ).kind,
    "file-change-unopenable",
  );
  assert.equal(
    module.fileChangeOutcome(
      state([section([entry({ openable: false, action: null })])], {
        turnStatus: "Completed",
      }),
      EXPECTED,
    ).kind,
    "file-change-absent",
  );
  assert.equal(
    module.fileChangeOutcome(
      state([section([entry({ openable: false })])]),
      EXPECTED,
    ).kind,
    "file-change-unopenable",
  );
});

test("the file change outcome passes a scope or turn failure through", async () => {
  const module = await load();
  assert.equal(
    module.fileChangeOutcome({ kind: "scope-lost" }, EXPECTED).kind,
    "scope-lost",
  );
  assert.equal(
    module.fileChangeOutcome({ kind: "turn-scan-exceeded" }, EXPECTED).kind,
    "turn-scan-exceeded",
  );
  assert.equal(module.fileChangeOutcome(null, EXPECTED).kind, "read-invalid");
});

test("opening a diff clicks the single matching open control", async () => {
  const module = await load();
  const dom = buildDom([sectionMarkup([entryMarkup()])]);
  const clicks = [];
  for (const button of dom.window.document.querySelectorAll(
    "button.acp-resource-action",
  ))
    button.addEventListener("click", () =>
      clicks.push(button.getAttribute("aria-label")),
    );
  assert.deepEqual(
    evaluate(
      dom,
      module.openDiffExpression(
        focusedChatSurfacePreamble('return {outcome:"scope-lost"};'),
        "write",
        { rootName: "scratch", relativePath: "hr6-permission.txt" },
      ),
    ),
    { outcome: "opened", path: "hr6-permission.txt" },
  );
  assert.deepEqual(clicks, ["Open diff scratch / hr6-permission.txt"]);
});

test("opening a diff refuses an absent, ambiguous or disabled control", async () => {
  const module = await load();
  const expression = module.openDiffExpression(
    focusedChatSurfacePreamble('return {outcome:"scope-lost"};'),
    "write",
    { rootName: "scratch", relativePath: "hr6-permission.txt" },
  );
  assert.deepEqual(
    evaluate(
      buildDom([
        sectionMarkup([
          entryMarkup({
            actionLabel: "Open diff scratch / alpha.txt",
            name: "alpha.txt",
          }),
        ]),
      ]),
      expression,
    ),
    { outcome: "action-absent" },
  );
  assert.deepEqual(
    evaluate(
      buildDom([sectionMarkup([entryMarkup(), entryMarkup()])]),
      expression,
    ),
    { outcome: "action-ambiguous" },
  );
  assert.deepEqual(
    evaluate(
      buildDom([sectionMarkup([entryMarkup({ disabled: true })])]),
      expression,
    ),
    { outcome: "action-unavailable" },
  );
});

test("opening a diff refuses an ambiguous turn", async () => {
  const module = await load();
  const dom = buildDom([sectionMarkup([entryMarkup()])]);
  dom.window.document.querySelector(
    'section.acp-turn[data-turn-id="earlier"]',
  ).dataset.turnId = "write";
  assert.deepEqual(
    evaluate(
      dom,
      module.openDiffExpression(
        focusedChatSurfacePreamble('return {outcome:"scope-lost"};'),
        "write",
        { rootName: "scratch", relativePath: "hr6-permission.txt" },
      ),
    ),
    { outcome: "turn-ambiguous" },
  );
});

test("the diff surface read projects the path and the added lines of the open pane", async () => {
  const value = await surfaceRead(paneDom({}));
  assert.deepEqual(value, {
    kind: "diff-surface",
    path: "Diff for hr6-permission.txt",
    headerPath: "hr6-permission.txt",
    counts: ["+1", "-0"],
    countsAbsent: false,
    surfaceCounts: ["+1", "-0"],
    hunks: 1,
    hunksExceeded: false,
    context: 0,
    contextExceeded: false,
    lines: ["+hr6 permission granted"],
    removed: 0,
    removedExceeded: false,
    linesExceeded: false,
    editor: true,
    loading: false,
    error: false,
    truncated: false,
  });
});

test("the diff surface read reports an absent pane, surface or duplicate pane", async () => {
  assert.deepEqual(await surfaceRead(paneDom({ collapsed: true })), {
    kind: "pane-absent",
  });
  assert.deepEqual(await surfaceRead(paneDom({ body: "" })), {
    kind: "diff-absent",
  });
  assert.deepEqual(await surfaceRead(paneDom({ duplicate: true })), {
    kind: "pane-ambiguous",
  });
  assert.deepEqual(
    await surfaceRead(paneDom({ body: diffMarkup({}) + diffMarkup({}) })),
    { kind: "diff-ambiguous" },
  );
});

test("the diff surface read reports the loading and error notices", async () => {
  const loading = await surfaceRead(paneDom({ notice: "loading" }));
  assert.equal(loading.loading, true);
  assert.equal(loading.editor, false);
  const failed = await surfaceRead(paneDom({ notice: "error" }));
  assert.equal(failed.error, true);
});

test("the diff surface outcome accepts the exact path and ordered added lines", async () => {
  const module = await load();
  const value = await surfaceRead(paneDom({}));
  const outcome = module.diffSurfaceOutcome(value, {
    relativePath: "hr6-permission.txt",
    additions: 1,
    deletions: 0,
    addedLines: ["hr6 permission granted"],
  });
  assert.deepEqual(outcome, {
    kind: "diff-surface",
    lines: ["hr6 permission granted"],
  });
});

test("the diff surface outcome waits while the diff is still loading", async () => {
  const module = await load();
  const value = await surfaceRead(paneDom({ notice: "loading" }));
  assert.equal(
    module.diffSurfaceOutcome(value, {
      relativePath: "hr6-permission.txt",
      additions: 1,
      deletions: 0,
      addedLines: ["hr6 permission granted"],
    }),
    null,
  );
});

test("the diff surface outcome fails closed on an error, truncation or wrong lines", async () => {
  const module = await load();
  const expected = {
    relativePath: "hr6-permission.txt",
    additions: 1,
    deletions: 0,
    addedLines: ["hr6 permission granted"],
  };
  assert.equal(
    module.diffSurfaceOutcome(
      await surfaceRead(paneDom({ notice: "error" })),
      expected,
    ).kind,
    "diff-error",
  );
  assert.equal(
    module.diffSurfaceOutcome(
      await surfaceRead(paneDom({ truncated: true })),
      expected,
    ).kind,
    "diff-truncated",
  );
  assert.equal(
    module.diffSurfaceOutcome(
      await surfaceRead(paneDom({ path: "alpha.txt" })),
      expected,
    ).kind,
    "diff-path-mismatch",
  );
  assert.equal(
    module.diffSurfaceOutcome(
      await surfaceRead(paneDom({ lines: ["hr6 permission denied"] })),
      expected,
    ).kind,
    "diff-lines-mismatch",
  );
  assert.equal(
    module.diffSurfaceOutcome(
      await surfaceRead(paneDom({ collapsed: true })),
      expected,
    ).kind,
    "pane-absent",
  );
  assert.equal(module.diffSurfaceOutcome(null, expected).kind, "read-invalid");
});

test("the file change outcome fails closed when the turn totals disagree", async () => {
  const module = await load();
  assert.equal(
    module.fileChangeOutcome(
      state([section([entry()], { additions: "+99", deletions: "-41" })]),
      EXPECTED,
    ).kind,
    "file-change-mismatch",
  );
  assert.equal(
    module.fileChangeOutcome(
      state([section([entry()], { label: "Edited 2 files" })]),
      EXPECTED,
    ).kind,
    "file-change-mismatch",
  );
});

test("the file change outcome refuses an expectation without counts or operation", async () => {
  const module = await load();
  const sections = state([section([entry()])]);
  assert.equal(
    module.fileChangeOutcome(sections, {
      relativePath: "hr6-permission.txt",
      rootName: "scratch",
      additions: 1,
      deletions: 0,
    }).kind,
    "file-change-expectation-invalid",
  );
  assert.equal(
    module.fileChangeOutcome(sections, {
      relativePath: "hr6-permission.txt",
      rootName: "scratch",
      additions: 1,
      operation: "created",
    }).kind,
    "file-change-expectation-invalid",
  );
  assert.equal(
    module.fileChangeOutcome(sections, {
      relativePath: "hr6-permission.txt",
      rootName: "scratch",
      additions: 1,
      deletions: 0,
      operation: "modified",
    }).kind,
    "file-change-mismatch",
  );
});

test("the diff surface read projects the removed line count of the open pane", async () => {
  const value = await surfaceRead(
    paneDom({ removed: ["hr6 permission gone"] }),
  );
  assert.equal(value.removed, 1);
  assert.equal(value.removedExceeded, false);
});

test("the diff surface outcome fails closed on any removed line", async () => {
  const module = await load();
  assert.equal(
    module.diffSurfaceOutcome(
      await surfaceRead(paneDom({ removed: ["hr6 permission gone"] })),
      {
        relativePath: "hr6-permission.txt",
        additions: 1,
        deletions: 0,
        addedLines: ["hr6 permission granted"],
      },
    ).kind,
    "diff-removed-lines",
  );
});

test("the diff surface outcome requires the exact artifact header counts", async () => {
  const module = await load();
  const expected = {
    relativePath: "hr6-permission.txt",
    additions: 1,
    deletions: 0,
    addedLines: ["hr6 permission granted"],
  };
  assert.equal(
    module.diffSurfaceOutcome(
      await surfaceRead(paneDom({ artifactCounts: ["+99", "-41"] })),
      expected,
    ).kind,
    "diff-counts-mismatch",
  );
  assert.equal(
    module.diffSurfaceOutcome(
      await surfaceRead(paneDom({ headerCounts: ["+99", "-41"] })),
      expected,
    ).kind,
    "diff-counts-mismatch",
  );
  assert.equal(
    module.diffSurfaceOutcome(
      await surfaceRead(paneDom({ artifactCounts: ["+1"] })),
      expected,
    ).kind,
    "diff-counts-mismatch",
  );
});

test("the diff surface read projects the artifact header counts of the open pane", async () => {
  const value = await surfaceRead(paneDom({ headerCounts: ["", ""] }));
  assert.deepEqual(value.counts, ["+1", "-0"]);
  assert.deepEqual(value.surfaceCounts, ["", ""]);
});

test("the diff surface outcome accepts an empty diff header when the artifact header agrees", async () => {
  const module = await load();
  assert.deepEqual(
    module.diffSurfaceOutcome(
      await surfaceRead(paneDom({ headerCounts: ["", ""] })),
      {
        relativePath: "hr6-permission.txt",
        additions: 1,
        deletions: 0,
        addedLines: ["hr6 permission granted"],
      },
    ),
    { kind: "diff-surface", lines: ["hr6 permission granted"] },
  );
});

test("the diff surface outcome refuses artifact header counts that miss the expectation", async () => {
  const module = await load();
  const expected = {
    relativePath: "hr6-permission.txt",
    additions: 1,
    deletions: 0,
    addedLines: ["hr6 permission granted"],
  };
  for (const options of [
    { artifactCounts: ["+2", "-0"] },
    { artifactCounts: ["+1", "-3"] },
    { artifactCounts: ["+one", "-0"] },
    { artifactCounts: ["+1", ""] },
    { duplicateArtifactCounts: true },
  ]) {
    assert.equal(
      module.diffSurfaceOutcome(await surfaceRead(paneDom(options)), expected)
        .kind,
      "diff-counts-mismatch",
      JSON.stringify(options),
    );
  }
});

test("the diff surface outcome waits for artifact header counts that have not rendered", async () => {
  const module = await load();
  assert.equal(
    module.diffSurfaceOutcome(
      await surfaceRead(paneDom({ artifactCounts: [] })),
      {
        relativePath: "hr6-permission.txt",
        additions: 1,
        deletions: 0,
        addedLines: ["hr6 permission granted"],
      },
    ).kind,
    "diff-counts-pending",
  );
});

test("the diff surface outcome refuses a diff header that contradicts the artifact header", async () => {
  const module = await load();
  const expected = {
    relativePath: "hr6-permission.txt",
    additions: 1,
    deletions: 0,
    addedLines: ["hr6 permission granted"],
  };
  assert.equal(
    module.diffSurfaceOutcome(
      await surfaceRead(paneDom({ headerCounts: ["+9", "-9"] })),
      expected,
    ).kind,
    "diff-counts-mismatch",
  );
  assert.equal(
    module.diffSurfaceOutcome(
      await surfaceRead(paneDom({ headerCounts: ["+1"] })),
      expected,
    ).kind,
    "diff-counts-unreadable",
  );
});

test("the diff surface outcome refuses an expectation without counts", async () => {
  const module = await load();
  assert.equal(
    module.diffSurfaceOutcome(await surfaceRead(paneDom({})), {
      relativePath: "hr6-permission.txt",
      addedLines: ["hr6 permission granted"],
    }).kind,
    "diff-expectation-invalid",
  );
});

test("opening a diff requires the exact full open-diff label", async () => {
  const module = await load();
  const expression = module.openDiffExpression(
    focusedChatSurfacePreamble('return {outcome:"scope-lost"};'),
    "write",
    { rootName: "scratch", relativePath: "hr6-permission.txt" },
  );
  assert.deepEqual(
    evaluate(
      buildDom([
        sectionMarkup([
          entryMarkup({
            actionLabel: "Open diff other / scratch / hr6-permission.txt",
          }),
        ]),
      ]),
      expression,
    ),
    { outcome: "action-absent" },
  );
  assert.deepEqual(
    evaluate(
      buildDom([
        sectionMarkup([
          entryMarkup({ actionLabel: "Open diff notes / hr6-permission.txt" }),
        ]),
      ]),
      expression,
    ),
    { outcome: "action-absent" },
  );
});

test("a created file must show no context rows", async () => {
  const module = await load();
  const value = await surfaceRead(paneDom({ context: true }));
  assert.equal(value.context, 1);
  assert.deepEqual(
    module.diffSurfaceOutcome(value, {
      relativePath: "hr6-permission.txt",
      additions: 1,
      deletions: 0,
      addedLines: ["hr6 permission granted"],
    }),
    { kind: "diff-context-lines", context: 1 },
  );
});

test("a created file must show exactly one hunk", async () => {
  const module = await load();
  const value = await surfaceRead(paneDom({}));
  assert.deepEqual(
    module.diffSurfaceOutcome(
      { ...value, hunks: 2 },
      {
        relativePath: "hr6-permission.txt",
        additions: 1,
        deletions: 0,
        addedLines: ["hr6 permission granted"],
      },
    ),
    { kind: "diff-hunks-mismatch", hunks: 2 },
  );
});

test("a hunk or context overflow fails closed rather than passing", async () => {
  const module = await load();
  const value = await surfaceRead(paneDom({}));
  const expected = {
    relativePath: "hr6-permission.txt",
    additions: 1,
    deletions: 0,
    addedLines: ["hr6 permission granted"],
  };
  for (const overflow of ["hunksExceeded", "contextExceeded"])
    assert.deepEqual(
      module.diffSurfaceOutcome({ ...value, [overflow]: true }, expected),
      { kind: "diff-scan-exceeded" },
    );
});
