import assert from "node:assert/strict";
import test from "node:test";

import { undefinedCustomProperties } from "./css-token-policy.mjs";

const css = (relativePath, source) => ({ relativePath, source });

test("flags a var() read that nothing defines, with its line", () => {
  const reads = undefinedCustomProperties(
    [
      css("src/styles/global.css", ":root { --color-border-subtle: #222; }"),
      css(
        "src/styles/panel.css",
        ".a {\n  border-top: 1px solid var(--color-border);\n}",
      ),
    ],
    [],
  );
  assert.deepEqual(reads, [
    { line: 2, name: "--color-border", relativePath: "src/styles/panel.css" },
  ]);
});

test("accepts properties defined in any stylesheet or set from script", () => {
  const reads = undefinedCustomProperties(
    [
      css("src/a.css", ".a { --gap: 4px; gap: var(--gap); }"),
      css("src/b.css", ".b { margin: var( --gap ); width: var(--w); }"),
    ],
    [
      {
        relativePath: "src/B.tsx",
        source: 'style={{ "--w": `${px}px` } as CSSProperties}',
      },
    ],
  );
  assert.deepEqual(reads, []);
});

test("a read with a fallback may stay undefined", () => {
  assert.deepEqual(
    undefinedCustomProperties(
      [css("src/a.css", ".a { color: var(--maybe, red); }")],
      [],
    ),
    [],
  );
});

test("comments and strings neither define nor read properties", () => {
  const reads = undefinedCustomProperties(
    [
      css(
        "src/a.css",
        '/* --ghost: 1px; var(--old) */ .a { content: "var(--quoted)"; padding: var(--ghost); }',
      ),
    ],
    [],
  );
  assert.deepEqual(
    reads.map(({ name }) => name),
    ["--ghost"],
  );
});
