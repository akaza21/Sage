import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
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
