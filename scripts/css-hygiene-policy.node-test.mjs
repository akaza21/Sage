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

test("reports raw z-index layers and allows only --z-* tokens and offsets", () => {
  const inventory = analyze({
    "src/styles/layers.css": `
      .token { z-index: var(--z-popover); }
      .offset { z-index: calc(var(--z-codemirror-gutters) + 1); }
      .auto { z-index: auto; }
      .raw { z-index: 80; }
      .negative { z-index: -1; }
      .arithmetic { z-index: calc(80 + 1); }
    `,
    "src/styles/global.css": `
      :root { --z-modal: 90; }
      .close-failure { z-index: 1000; }
    `,
  });

  assert.deepEqual(
    inventory.rawZIndexes.map(({ relativePath, line, value }) => ({
      relativePath,
      line,
      value,
    })),
    [
      { relativePath: "src/styles/global.css", line: 3, value: "1000" },
      { relativePath: "src/styles/layers.css", line: 5, value: "80" },
      { relativePath: "src/styles/layers.css", line: 6, value: "-1" },
      { relativePath: "src/styles/layers.css", line: 7, value: "calc(80 + 1)" },
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

test("allows raw colours in custom properties only in the global authority and listed feature owners", () => {
  const inventory = analyze({
    "src/styles/global.css": `:root { --global-fill: #111111; }`,
    "src/features/browser/browser-device.css": `
      .owner { --device-fill: #414141; }
      .owner-rule { color: #414141; }
    `,
    "src/features/other/other.css": `.other { --other-fill: #222222; }`,
  });

  assert.deepEqual(
    inventory.rawColors.map(({ relativePath, line, literal }) => ({
      relativePath,
      line,
      literal,
    })),
    [
      {
        relativePath: "src/features/browser/browser-device.css",
        line: 3,
        literal: "#414141",
      },
      {
        relativePath: "src/features/other/other.css",
        line: 1,
        literal: "#222222",
      },
    ],
  );
});

test("reports rounded per-corner radii and allows only squaring a segment's corner", () => {
  const inventory = analyze({
    "src/styles/a.css": `
      .tab { border-top-left-radius: var(--radius-xl); }
      .split > button { border-start-end-radius: 0; }
      .split > button + button { border-end-start-radius: var(--radius-none); }
      .whole { border-radius: var(--radius-xl); }
    `,
  });

  assert.deepEqual(inventory.cornerRadii, [
    {
      line: 2,
      property: "border-top-left-radius",
      relativePath: "src/styles/a.css",
      value: "var(--radius-xl)",
    },
    {
      line: 3,
      property: "border-start-end-radius",
      relativePath: "src/styles/a.css",
      value: "0",
    },
  ]);
});

test("allows !important only under reduced-motion and forced-colors queries", () => {
  const inventory = analyze({
    "src/styles/a.css": `
      .close:hover { color: var(--color-text-primary) !important; }
      .label::after { content: "!important"; }
      @media (prefers-reduced-motion: reduce) {
        * { animation-duration: 0s !important; }
      }
      @media (forced-colors: active) {
        @supports (display: grid) {
          .chip { color: CanvasText ! important; }
        }
      }
      @media (max-width: 40rem) {
        .row { padding: 0 !important; }
      }
    `,
  });

  assert.deepEqual(
    inventory.importants.map(({ line, property }) => ({ line, property })),
    [
      { line: 2, property: "color" },
      { line: 13, property: "padding" },
    ],
  );
});

test("checks vendor-prefixed and nested declarations for !important", () => {
  const inventory = analyze({
    "src/styles/a.css": `
      .a { -webkit-user-select: none !important; }
      .b {
        @media (min-width: 40rem) { color: inherit !important; }
      }
      .c {
        @media (forced-colors: active) { color: CanvasText !important; }
      }
    `,
  });

  assert.deepEqual(
    inventory.importants.map(({ line, property }) => ({ line, property })),
    [
      { line: 2, property: "-webkit-user-select" },
      { line: 4, property: "color" },
    ],
  );
});

test("allows !important only when every media alternative requires the preference", () => {
  const inventory = analyze({
    "src/styles/a.css": `
      @media (prefers-reduced-motion: reduce), (min-width: 1px) {
        .a { color: inherit !important; }
      }
      @media ( forced-colors : active ) {
        .b { color: CanvasText !important; }
      }
      @media screen and ((prefers-reduced-motion: reduce)) {
        .c { animation: none !important; }
      }
      @media not (forced-colors: active) {
        .d { color: inherit !important; }
      }
      @media (forced-colors: active) or (min-width: 1px) {
        .e { color: inherit !important; }
      }
    `,
  });

  assert.deepEqual(
    inventory.importants.map(({ line }) => line),
    [3, 12, 15],
  );
});

test("reads a declaration's priority apart from its value", () => {
  const inventory = analyze({
    "src/styles/a.css": `
      @media (forced-colors: active) {
        .split > button {
          border-top-left-radius: var(--radius-none) !important;
        }
      }
    `,
  });

  assert.deepEqual(inventory.cornerRadii, []);
  assert.deepEqual(inventory.importants, []);
});

test("splits selectors only at commas outside comments and strings", () => {
  const inventory = analyze({
    "src/styles/a.css": `
      .shared/* note, note */ { color: var(--color-text-primary); }
      [data-label="/*x*/"] { color: var(--color-text-primary); }
    `,
    "src/styles/b.css": `
      .shared { color: var(--color-text-primary); }
      [data-label=" "] { color: var(--color-text-primary); }
    `,
  });

  assert.deepEqual(
    inventory.duplicateSelectors.map(({ selector }) => selector),
    [".shared"],
  );
});

test("holds vendor-prefixed aliases to the standard property's rules", () => {
  const inventory = analyze({
    "src/styles/a.css": `
      .a { -webkit-border-top-left-radius: 8px; }
      .b { -moz-border-radius: 4px; }
      .c { -webkit-border-top-left-radius: var(--radius-none); }
    `,
  });

  assert.deepEqual(
    inventory.cornerRadii.map(({ line, property }) => ({ line, property })),
    [{ line: 2, property: "-webkit-border-top-left-radius" }],
  );
  assert.deepEqual(
    inventory.rawRadii.map(({ line, value }) => ({ line, value })),
    [{ line: 3, value: "4px" }],
  );
});

// The scan is linear: 400 KiB of masked comment takes tens of milliseconds,
// where the earlier per-position rescans took minutes.
test("reads a long media condition in one pass", { timeout: 20_000 }, () => {
  const filler = " ".repeat(400 * 1024);
  const inventory = analyze({
    "src/styles/a.css": `@media screen/*${filler}*/and (forced-colors: active) { .a { color: CanvasText !important; } }`,
  });

  assert.deepEqual(inventory.importants, []);
});
