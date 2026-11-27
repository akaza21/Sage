import assert from "node:assert/strict";
import test from "node:test";

import { offScaleDeclarations } from "./css-scale-policy.mjs";

const scan = (relativePath, source) =>
  offScaleDeclarations([{ relativePath, source }]).map(
    ({ line, message }) => `${line} ${message}`,
  );

test("flags px font sizes outside the type scale authority", () => {
  assert.deepEqual(scan("src/a.css", ".a {\n  font-size: 12px;\n}"), [
    "2 font-size 12px; use a var(--font-size-*) token",
  ]);
  assert.deepEqual(
    scan("src/a.css", ".a { font-size: var(--font-size-xs); font: inherit; }"),
    [],
  );
  assert.deepEqual(
    scan("src/styles/global.css", ":root { --font-size-xs: 12px; }"),
    [],
  );
});

test("flags @media widths off the shared breakpoints, not container queries", () => {
  assert.deepEqual(
    scan(
      "src/a.css",
      `@media (max-width: 760px) and (max-height: 320px) {}
       @media (max-width: 920px) {}
       @container chat (max-width: 520px) {}
       /* @media (max-width: 1px) {} */`,
    ),
    ["2 @media width 920px; use 640, 760, 960px or a container query"],
  );
});
