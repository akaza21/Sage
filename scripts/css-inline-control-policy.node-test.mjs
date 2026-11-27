import assert from "node:assert/strict";
import test from "node:test";

import { controlsBesideParent } from "./css-inline-control-policy.mjs";

const scan = (source) =>
  controlsBesideParent([{ relativePath: "src/a.css", source }]);

test("flags an absolute control placed beside its parent's box", () => {
  assert.deepEqual(
    scan(
      ".chip__clear {\n  position: absolute;\n  inset-inline-start: 100%;\n}",
    ),
    [{ line: 3, property: "inset-inline-start", relativePath: "src/a.css" }],
  );
  assert.equal(
    scan(".x { left: calc(100% + 4px); position: absolute; }").length,
    1,
  );
  assert.equal(scan(".x { position: absolute; right: -100%; }").length, 1);
});

test("allows popovers below their trigger and offsets inside the parent", () => {
  assert.deepEqual(
    scan(`
      .menu { position: absolute; top: calc(100% + 6px); left: 0; }
      .badge { position: absolute; inset-inline-end: 4px; }
      .half { position: absolute; left: 50%; }
      .flow { left: 100%; }
      /* .old { position: absolute; left: 100%; } */
    `),
    [],
  );
});
