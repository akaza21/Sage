import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { SourceMap } from "node:module";
import path from "node:path";
import test from "node:test";
import { htmlCompletionSchemaPlugin } from "./htmlCompletionSchema.ts";

const source = readFileSync(
  "node_modules/@codemirror/lang-html/dist/index.js",
  "utf8",
);
const entryId = path.resolve(
  "node_modules/@codemirror/lang-html/dist/index.js",
);

test("maps relocated events, wrapped schema, and unchanged HTML source positions", async () => {
  const result = await htmlCompletionSchemaPlugin().transform.call(
    {},
    source,
    entryId,
  );
  assert.ok(result.map);
  assert.deepEqual(result.map.sources, [entryId.replaceAll("\\", "/")]);
  assert.deepEqual(result.map.sourcesContent, [source]);
  const map = new SourceMap(result.map);
  const position = (text, offset) => {
    const lines = text.slice(0, offset).split("\n");
    return { line: lines.length - 1, column: lines.at(-1).length };
  };
  for (const text of [
    "import { parser, configureNesting }",
    "const Targets =",
    "const eventAttributes =",
    "beforeunload copy cut",
    "for (let a of eventAttributes)",
    "class Schema {",
    "Schema.default = /*@__PURE__*/new Schema;",
    "const htmlLanguage =",
    "export { autoCloseTags",
  ]) {
    const originalOffset = source.indexOf(text);
    const generatedOffset = result.code.indexOf(text);
    assert.ok(originalOffset >= 0 && generatedOffset >= 0, text);
    for (const column of [0, text.length - 1]) {
      const original = position(source, originalOffset + column);
      const generated = position(result.code, generatedOffset + column);
      const mapped = map.findEntry(generated.line, generated.column);
      assert.equal(mapped.originalLine, original.line, text);
      assert.equal(mapped.originalColumn, original.column, text);
    }
  }
});

async function runBuild(plugin, ids) {
  const context = {
    error(message) {
      throw new Error(message);
    },
  };
  await plugin.buildStart?.call(context, {});
  for (const id of ids) {
    await plugin.transform.call(context, source, id);
  }
  await plugin.generateBundle?.call(context, {}, {}, false);
}

test("fails the build when the HTML entry is renamed", async () => {
  await assert.rejects(
    runBuild(htmlCompletionSchemaPlugin(), [
      path.resolve("node_modules/@codemirror/lang-html/dist/renamed.js"),
    ]),
    /HTML completion schema transform must apply exactly once.*got 0/,
  );
});

test("allows builds with exactly one guarded HTML transformation", async () => {
  await assert.doesNotReject(runBuild(htmlCompletionSchemaPlugin(), [entryId]));
});

test("fails the build when the HTML transform applies more than once", async () => {
  await assert.rejects(
    runBuild(htmlCompletionSchemaPlugin(), [entryId, entryId]),
    /HTML completion schema transform must apply exactly once.*got 2/,
  );
});

test("counts transformations independently for successive builds", async () => {
  const plugin = htmlCompletionSchemaPlugin();
  await assert.doesNotReject(runBuild(plugin, [entryId]));
  await assert.doesNotReject(runBuild(plugin, [entryId]));
  await assert.rejects(
    runBuild(plugin, []),
    /HTML completion schema transform must apply exactly once.*got 0/,
  );
});
