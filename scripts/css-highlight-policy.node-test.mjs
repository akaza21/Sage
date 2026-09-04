import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import {
  CSS_HIGHLIGHT_MAX_FILE_BYTES,
  CSS_HIGHLIGHT_MAX_FILES,
  cssHighlightPolicyErrors,
  discoverSourceFilesWithCssBound,
  highlightRingErrors,
} from "./css-highlight-policy.mjs";

test("rejects normal and multiline thick outline declarations", () => {
  const errors = highlightRingErrors(
    `.one { outline: 2px solid red; }
     @supports (display: grid) {
       .two {
         outline:
           dotted 3px
           blue;
       }
     }`,
    "src/styles/adversarial.css",
  );

  assert.equal(errors.length, 2);
  assert.match(errors[0], /adversarial\.css:1.*2px outline/u);
  assert.match(errors[1], /adversarial\.css:4.*3px outline/u);
});

test("rejects zero-blur 2px and multiline 3px rings", () => {
  const errors = highlightRingErrors(
    `.one { box-shadow: 0 0 0 2px red; }
     .two {
       box-shadow:
         inset 0
         0 0 3px blue,
         0 0 6px 1px transparent;
     }`,
    "src/styles/adversarial.css",
  );

  assert.equal(errors.length, 2);
  assert.match(errors[0], /2px zero-blur box-shadow ring/u);
  assert.match(errors[1], /3px zero-blur box-shadow ring/u);
});

test("handles color-first zero-blur rings in normal and forced colors", () => {
  const errors = highlightRingErrors(
    `.normal { box-shadow: 0 2px 4px red, blue 0 0 0 2px; }
     @media (forced-colors: active) {
       .kept { box-shadow: 0 1px 2px CanvasText, Highlight 0 0 0 3px; }
     }`,
    "src/styles/adversarial.css",
  );

  assert.equal(errors.length, 1);
  assert.match(errors[0], /adversarial\.css:1.*2px zero-blur/u);
});

test("allows nested forced-colors rings and still checks neighboring blocks", () => {
  const errors = highlightRingErrors(
    `@media screen and (forced-colors: active) {
       @supports (outline: 2px solid Highlight) {
         .kept { outline: 2px solid Highlight; }
         .also-kept { box-shadow: 0 0 0 3px Highlight; }
       }
     }
     @media (forced-colors: none) {
       .rejected { outline: 2px solid red; }
     }
     @media not (forced-colors: active) {
       .also-rejected { box-shadow: 0 0 0 3px red; }
     }
     @media (forced-colors: active), screen {
       .list-rejected { outline: 2px solid red; }
     }`,
    "src/styles/adversarial.css",
  );

  assert.equal(errors.length, 3);
  assert.match(errors[0], /rejected|:8/u);
  assert.match(errors[1], /3px zero-blur box-shadow ring/u);
  assert.match(errors[2], /2px outline/u);
});

test("ignores comments and strings without hiding following declarations", () => {
  const errors = highlightRingErrors(
    `.safe::before {
       content: "outline: 3px solid red; box-shadow: 0 0 0 2px red";
       /* outline: 2px solid red;
          box-shadow: 0 0 0 3px red; */
       outline: 1px solid currentColor;
       box-shadow: 0 0 6px 1px var(--highlight-focus-halo);
     }
     .bad { outline: solid 3px red; }`,
    "src/styles/adversarial.css",
  );

  assert.equal(errors.length, 1);
  assert.match(errors[0], /3px outline/u);
});

test("fails closed when the production CSS scan exceeds its bounds", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-highlight-policy-"));
  await mkdir(path.join(root, "src/styles"), { recursive: true });
  await writeFile(
    path.join(root, "src/styles/oversized.css"),
    "a".repeat(CSS_HIGHLIGHT_MAX_FILE_BYTES + 1),
  );

  const oversized = await cssHighlightPolicyErrors(root, [
    "src/styles/oversized.css",
  ]);
  assert.equal(oversized.length, 1);
  assert.match(oversized[0], /exceeds.*byte scan limit/u);

  const tooMany = await cssHighlightPolicyErrors(
    root,
    Array.from(
      { length: CSS_HIGHLIGHT_MAX_FILES + 1 },
      () => "src/styles/oversized.css",
    ),
  );
  assert.equal(tooMany.length, 1);
  assert.match(tooMany[0], /exceeds.*file scan limit/u);
});

test("stops shared source discovery at the CSS cap plus one", async () => {
  const root = await mkdtemp(path.join(tmpdir(), "sage-highlight-discovery-"));
  const sourceRoot = path.join(root, "src");
  await mkdir(sourceRoot, { recursive: true });
  await Promise.all(
    Array.from({ length: CSS_HIGHLIGHT_MAX_FILES + 2 }, (_, index) =>
      writeFile(
        path.join(sourceRoot, `${String(index).padStart(3, "0")}.css`),
        ".safe { outline: 1px solid currentColor; }",
      ),
    ),
  );
  await writeFile(path.join(sourceRoot, "zzz-after-boundary.ts"), "export {};");

  const inventory = await discoverSourceFilesWithCssBound(root, "src");

  assert.equal(inventory.cssFiles.length, CSS_HIGHLIGHT_MAX_FILES + 1);
  assert.equal(inventory.exceededCssFileLimit, true);
  assert.equal(
    inventory.files.includes(path.join("src", "zzz-after-boundary.ts")),
    false,
  );
});
