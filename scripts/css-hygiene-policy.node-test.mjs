import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import {
  CSS_HYGIENE_MAX_FILE_BYTES,
  CSS_HYGIENE_MAX_FILES,
  analyzeCssInventory,
  cssHygienePolicyErrors,
} from "./css-hygiene-policy.mjs";

function analyze(files) {
  return analyzeCssInventory(
    Object.entries(files).map(([relativePath, source]) => ({
      relativePath,
      source,
    })),
  );
}

test("finds multiline cross-file selector ownership without counting same-file repeats", () => {
  const inventory = analyze({
    "src/styles/a.css": `
      .shared,
      .only-a:hover {
        color: var(--color-text-primary);
      }
      @media (max-width: 40rem) {
        .shared { color: var(--color-text-secondary); }
      }
    `,
    "src/styles/b.css": `
      @supports (display: grid) {
        .shared,
        .only-b { display: grid; }
      }
    `,
  });

  assert.deepEqual(inventory.duplicateSelectors, [
    {
      selector: ".shared",
      owners: [
        { relativePath: "src/styles/a.css", line: 2 },
        { relativePath: "src/styles/b.css", line: 3 },
      ],
    },
  ]);
});

test("does not treat keyframe steps as selectors", () => {
  const inventory = analyze({
    "src/styles/a.css": `
      @keyframes settle {
        from { opacity: 0; }
        50%, to { opacity: 1; }
      }
      from { color: var(--color-text-primary); }
    `,
    "src/styles/b.css": `.safe { color: var(--color-text-primary); }`,
  });

  assert.deepEqual(inventory.duplicateSelectors, []);
  assert.deepEqual(inventory.parseErrors, []);
});

test("normalizes selector comments while preserving quoted selector identity", () => {
  const inventory = analyze({
    "src/styles/a.css": `
      .shared/* comment */ { color: var(--color-text-primary); }
      [data-label="alpha  beta"] { color: var(--color-text-primary); }
    `,
    "src/styles/b.css": `
      .shared { color: var(--color-text-primary); }
      [data-label="alpha beta"] { color: var(--color-text-primary); }
    `,
  });

  assert.deepEqual(
    inventory.duplicateSelectors.map(({ selector }) => selector),
    [".shared"],
  );
});

test("reports multiline raw radii and allows only one radius custom-property token", () => {
  const inventory = analyze({
    "src/styles/radii.css": `
      .token { border-radius: var(--radius-sm); }
      .zero { border-radius: 0; }
      .circle { border-radius: 50%; }
      .length { border-radius:
        7px; }
      .mixed { border-radius: var(--radius-sm) 0; }
    `,
  });

  assert.deepEqual(
    inventory.rawRadii.map(({ relativePath, line, value }) => ({
      relativePath,
      line,
      value,
    })),
    [
      { relativePath: "src/styles/radii.css", line: 3, value: "0" },
      { relativePath: "src/styles/radii.css", line: 4, value: "50%" },
      { relativePath: "src/styles/radii.css", line: 5, value: "7px" },
      {
        relativePath: "src/styles/radii.css",
        line: 7,
        value: "var(--radius-sm) 0",
      },
    ],
  );
});

test("finds hex, rgb, and rgba literals but ignores comments, strings, and custom-property names", () => {
  const inventory = analyze({
    "src/styles/colors.css": `
      .bad {
        --rgb-role: var(--color-surface);
        color: #abc;
        border-color: #aabbccdd;
        background: rgb(
          1 2 3 / 40%
        );
        box-shadow: 0 0 1px rgba(1, 2, 3, 0.4);
        content: "#fff rgb(0 0 0)";
        /* color: #123456; */
      }
    `,
    "src/styles/global.css": `:root {
      --root-role: #fff;
      --root-overlay: rgb(0 0 0 / 0.5);
      --root-shadow: rgba(0, 0, 0, 0.4);
      --radius-none: 0;
    }`,
  });

  assert.deepEqual(
    inventory.rawColors.map(({ line, literal }) => ({ line, literal })),
    [
      { line: 4, literal: "#abc" },
      { line: 5, literal: "#aabbccdd" },
      { line: 6, literal: "rgb(" },
      { line: 9, literal: "rgba(" },
    ],
  );
});

test("reports malformed CSS deterministically", () => {
  const inventory = analyze({
    "src/styles/z.css": `.z { color: var(--color-text-primary);`,
    "src/styles/a.css": `.a { content: "unterminated;`,
  });

  assert.deepEqual(inventory.parseErrors, [
    "src/styles/a.css:1 has an unclosed block.",
    "src/styles/a.css:1 has an unterminated string.",
    "src/styles/z.css:1 has an unclosed block.",
  ]);
});

test("orders selectors, owners, and violations independently of input order", () => {
  const forward = analyze({
    "src/styles/z.css": `.b, .a { color: #fff; border-radius: 4px; }`,
    "src/styles/a.css": `.b, .a { color: rgb(0 0 0); }`,
  });
  const reverse = analyze({
    "src/styles/a.css": `.b, .a { color: rgb(0 0 0); }`,
    "src/styles/z.css": `.b, .a { color: #fff; border-radius: 4px; }`,
  });

  assert.deepEqual(forward, reverse);
  assert.deepEqual(
    forward.duplicateSelectors.map(({ selector }) => selector),
    [".a", ".b"],
  );
  assert.deepEqual(
    forward.rawColors.map(({ relativePath }) => relativePath),
    ["src/styles/a.css", "src/styles/z.css"],
  );
});

test("fails closed at the CSS file and byte caps", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-css-hygiene-"));
  await mkdir(path.join(root, "src/styles"), { recursive: true });
  await writeFile(
    path.join(root, "src/styles/oversized.css"),
    "a".repeat(CSS_HYGIENE_MAX_FILE_BYTES + 1),
  );

  const oversized = await cssHygienePolicyErrors(root, [
    "src/styles/oversized.css",
  ]);
  assert.equal(oversized.length, 1);
  assert.match(oversized[0], /exceeds.*byte scan limit/u);

  const tooMany = await cssHygienePolicyErrors(
    root,
    Array.from(
      { length: CSS_HYGIENE_MAX_FILES + 1 },
      () => "src/styles/oversized.css",
    ),
  );
  assert.equal(tooMany.length, 1);
  assert.match(tooMany[0], /exceeds.*file scan limit/u);
});
