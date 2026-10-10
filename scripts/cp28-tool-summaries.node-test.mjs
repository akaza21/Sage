import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import ts from "typescript";

function sourceModule(path) {
  const source = ts.transpileModule(readFileSync(path, "utf8"), {
    compilerOptions: {
      target: ts.ScriptTarget.ES2022,
      module: ts.ModuleKind.ESNext,
    },
  }).outputText;
  const linked = source.replace(
    /from "(\.[^"]+)"/g,
    (_, relative) =>
      `from "${sourceModule(resolve(dirname(path), `${relative}.ts`))}"`,
  );
  return `data:text/javascript;base64,${Buffer.from(linked).toString("base64")}`;
}

const {
  toolRollupLabel,
  toolCommandLine,
  groupPrimaryActivities,
  toolRollupState,
} = await import(
  sourceModule(
    fileURLToPath(
      new URL(
        "../src/features/chat/acp/toolActivityRollup.ts",
        import.meta.url,
      ),
    ),
  )
);

function command(id, state = "completed", inputSummary = null) {
  return {
    id,
    kind: "toolCall",
    toolState: state,
    payload: {
      inputSummary,
      presentation: {
        kind: "execute",
        label: "Run command",
        summary: null,
        activity: "ordinary",
      },
    },
  };
}

test("collapsed ordinary command groups distinguish one, two and three calls", () => {
  assert.equal(toolRollupLabel("execute", [command("one")]), "Ran 1 command");
  assert.equal(
    toolRollupLabel("execute", [command("one"), command("two")]),
    "Ran 2 commands",
  );
  assert.equal(
    toolRollupLabel("execute", [
      command("one"),
      command("two"),
      command("three"),
    ]),
    "Ran 3 commands",
  );
});

test("live groups show the call count rather than a success label", () => {
  assert.equal(
    toolRollupLabel("execute", [command("one", "inProgress"), command("two")]),
    "Running 2 commands",
  );
});

test("mixed groups retain read and edit verbs and count command calls", () => {
  const read = {
    ...command("read"),
    payload: { presentation: { kind: "read", label: "Read", resources: [] } },
  };
  const edit = {
    ...command("edit"),
    payload: { presentation: { kind: "edit", label: "Edit", resources: [] } },
  };
  assert.equal(
    toolRollupLabel("mixed", [read, edit, command("one")]),
    "Read files, edited files, ran 1 command",
  );
  assert.equal(
    toolRollupLabel("mixed", [read, command("one"), command("two")]),
    "Read files, ran 2 commands",
  );
});

test("declared-test counts describe commands, not passed tests", () => {
  assert.equal(
    toolRollupLabel("execute", [command("one")], "declaredTest"),
    "Ran tests \u00b7 1 command",
  );
  assert.equal(
    toolRollupLabel(
      "execute",
      [command("one"), command("two")],
      "declaredTest",
    ),
    "Ran tests \u00b7 2 commands",
  );
});

test("a native null-summary command uses its retained input before expansion", () => {
  const live = command("one", "inProgress", "npm run check");
  assert.equal(toolRollupLabel("execute", [live]), "Running npm run check");
  assert.equal(
    toolRollupLabel("execute", [command("one", "completed", "npm run check")]),
    "Ran npm run check",
  );
});

test("command selection prefers a nonblank summary and falls back to retained input", () => {
  const item = command("one", "inProgress", "npm run check");
  item.payload.presentation.summary = "npm run lint";
  assert.equal(toolCommandLine(item), "npm run lint");
  item.payload.presentation.summary = " \t ";
  assert.equal(toolCommandLine(item), "npm run check");
  item.payload.inputSummary = "Run command";
  assert.equal(toolCommandLine(item), "Run command");
  item.payload.inputSummary = null;
  assert.equal(toolCommandLine(item), null);
  assert.equal(toolRollupLabel("execute", [item]), "Running command");
});

test("Codex-shaped command input remains visible when it equals the native label", () => {
  const item = command("codex-command", "inProgress", "npm run check");
  item.payload.presentation.label = "npm run check";
  assert.equal(toolRollupLabel("execute", [item]), "Running npm run check");
  item.toolState = "completed";
  assert.equal(toolRollupLabel("execute", [item]), "Ran npm run check");
});

test("a late input update refreshes the command without a new tool call", () => {
  const item = command("one", "inProgress");
  assert.equal(toolRollupLabel("execute", [item]), "Running command");
  item.payload.inputSummary = "npm run lint";
  assert.equal(toolRollupLabel("execute", [item]), "Running npm run lint");
});

test("command text is single-line and bounded with a Unicode-safe ellipsis", () => {
  assert.equal(
    toolCommandLine(command("one", "inProgress", " npm\nrun\tcheck ")),
    "npm run check",
  );
  const long = toolCommandLine(
    command("one", "inProgress", "x".repeat(510) + "\u{1f680}".repeat(1_000)),
  );
  assert.equal(long, "x".repeat(510) + "\u2026");
  assert.ok(long.length <= 512);
});

test("command selection preserves literal spaces inside quoted arguments", () => {
  const item = command("one", "inProgress", "printf 'two  spaces'");
  assert.equal(toolCommandLine(item), "printf 'two  spaces'");
  assert.equal(
    toolRollupLabel("execute", [item]),
    "Running printf 'two  spaces'",
  );
});

test("summary-copy has a scoped muted regular override of generic card typography", () => {
  const css = readFileSync(
    new URL("../src/features/chat/acp/acp-activity-cards.css", import.meta.url),
    "utf8",
  );
  assert.match(
    css,
    /\.acp-activity-card > summary > span\.acp-tool-rollup__summary-copy\s*\{[^}]*color:\s*var\(--color-text-muted\);[^}]*font-weight:\s*var\(--font-weight-regular\);/s,
  );
});

test("command labels can shrink beside their icon for one-line ellipsis", () => {
  const css = readFileSync(
    new URL(
      "../src/features/chat/acp/acp-activity-details.css",
      import.meta.url,
    ),
    "utf8",
  );
  assert.match(
    css,
    /\.acp-tool-rollup__label\s*\{[^}]*flex:\s*0 1 auto;[^}]*text-overflow:\s*ellipsis;[^}]*white-space:\s*nowrap;/s,
  );
});

test("empty activity and an input-less non-command do not invent execute rows", () => {
  assert.deepEqual(groupPrimaryActivities([]), []);
  const other = {
    ...command("other"),
    payload: {
      inputSummary: null,
      presentation: { kind: "other", label: "Unknown tool", summary: null },
    },
  };
  assert.equal(toolCommandLine(other), null);
  assert.equal(groupPrimaryActivities([other])[0].rollup.kind, "other");
});

test("failed and cancelled command groups keep their non-success states", () => {
  for (const state of ["failed", "cancelled", "declined"]) {
    const items = [command("one", state), command("two")];
    const grouped = groupPrimaryActivities(items);
    assert.equal(grouped.length, 1);
    assert.equal(toolRollupState(grouped[0].rollup.items), state);
  }
  assert.equal(
    toolRollupState([
      command("failed", "failed"),
      command("live", "inProgress"),
    ]),
    "failed",
  );
});

test("pending, waiting, cancelled and declined inputs do not claim they ran", () => {
  for (const [state, label] of [
    ["pending", "Pending npm run check"],
    ["waitingForApproval", "Awaiting approval \u00b7 npm run check"],
    ["cancelled", "Cancelled npm run check"],
    ["declined", "Declined npm run check"],
    ["failed", "Failed npm run check"],
  ]) {
    const items = [command("one", state, "npm run check")];
    assert.equal(toolRollupLabel("execute", items), label);
    assert.equal(toolRollupState(items), state);
  }
});

test("live and unsettled declared-test groups preserve their classification", () => {
  for (const [state, expected] of [
    ["inProgress", "Running tests \u00b7 2 commands"],
    ["pending", "Test commands pending \u00b7 2 commands"],
    ["waitingForApproval", "Test commands awaiting approval \u00b7 2 commands"],
    ["failed", "Test commands failed \u00b7 2 commands"],
    ["cancelled", "Test commands cancelled \u00b7 2 commands"],
    ["declined", "Test commands declined \u00b7 2 commands"],
  ]) {
    assert.equal(
      toolRollupLabel(
        "execute",
        [command("one", state), command("two", state)],
        "declaredTest",
      ),
      expected,
    );
  }
  assert.equal(
    toolRollupLabel("execute", [command("one", "inProgress")], "declaredTest"),
    "Running tests \u00b7 1 command",
  );
});

test("prose splits command groups and each group retains chronological input order", () => {
  const prose = {
    id: "prose",
    kind: "text",
    payload: { role: "assistant", text: "Next step" },
  };
  const grouped = groupPrimaryActivities([
    command("one"),
    command("two"),
    prose,
    command("three"),
  ]);
  assert.deepEqual(
    grouped.map((row) =>
      row.kind === "item"
        ? row.item.id
        : row.rollup.items.map((item) => item.id),
    ),
    [["one", "two"], "prose", ["three"]],
  );
});

test("65 command calls stay in capped groups of 64 and one", () => {
  const grouped = groupPrimaryActivities(
    Array.from({ length: 65 }, (_, index) => command(`command-${index}`)),
  );
  assert.deepEqual(
    grouped.map((row) => row.rollup.items.length),
    [64, 1],
  );
  assert.equal(
    toolRollupLabel("execute", grouped[0].rollup.items),
    "Ran 64 commands",
  );
  assert.equal(grouped[1].rollup.items[0].id, "command-64");
});
