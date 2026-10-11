import assert from "node:assert/strict";
import test from "node:test";

import { resolveUiPort } from "./lib/ui-test-port.mjs";

test("UI tests default to port 1431 when SAGE_UI_PORT is unset", () => {
  assert.equal(resolveUiPort({}), 1431);
});

test("UI tests accept a lane port and both non-privileged port boundaries", () => {
  assert.equal(resolveUiPort({ SAGE_UI_PORT: "15431" }), 15431);
  assert.equal(resolveUiPort({ SAGE_UI_PORT: "1024" }), 1024);
  assert.equal(resolveUiPort({ SAGE_UI_PORT: "65535" }), 65535);
});

test("UI test callers read SAGE_UI_PORT from the current process environment", () => {
  const previous = process.env.SAGE_UI_PORT;
  try {
    process.env.SAGE_UI_PORT = "15431";
    assert.equal(resolveUiPort(), 15431);
    process.env.SAGE_UI_PORT = "15432";
    assert.equal(resolveUiPort(), 15432);
    delete process.env.SAGE_UI_PORT;
    assert.equal(resolveUiPort(), 1431);
  } finally {
    if (previous === undefined) delete process.env.SAGE_UI_PORT;
    else process.env.SAGE_UI_PORT = previous;
  }
});

test("UI tests reject malformed, privileged, automatic and out-of-range ports", () => {
  for (const value of [
    "",
    " ",
    " 1431",
    "1431 ",
    "0",
    "1023",
    "65536",
    "-1431",
    "+1431",
    "1431.5",
    "1431.0",
    "1e4",
    "0x599",
    "1431oops",
    "NaN",
    "Infinity",
    "999999999999999999999999999999",
    "1431 --host 0.0.0.0",
  ]) {
    assert.throws(
      () => resolveUiPort({ SAGE_UI_PORT: value }),
      /SAGE_UI_PORT must be a decimal integer between 1024 and 65535/,
      JSON.stringify(value),
    );
  }
});
