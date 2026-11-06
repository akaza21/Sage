import assert from "node:assert/strict";
import test from "node:test";

import { createDiagnosticSanitizer } from "./lib/diagnostic-redaction.mjs";

const sanitize = createDiagnosticSanitizer([
  ["/home/dev/Sage", "<repo>"],
  [String.raw`D:\a\Sage\Sage`, "<repo>"],
  ["/home/dev", "<home>"],
]);

const CASES = [
  // Known roots keep a readable label, under either separator and any case.
  [
    "Failed to resolve import in /home/dev/Sage/src/app/x.ts:3:1",
    "Failed to resolve import in <repo>/src/app/x.ts:3:1",
  ],
  [
    String.raw`D:\a\Sage\Sage\src\x.ts and d:/A/SAGE/sage/src/y.ts`,
    String.raw`<repo>\src\x.ts and <repo>/src/y.ts`,
  ],
  ["cache at /home/dev/.cache/vite", "cache at <home>/.cache/vite"],
  // A root is matched whole, never as a prefix of a sibling folder.
  ["/home/dev/Sagebrush/x.ts", "<home>/Sagebrush/x.ts"],
  // UNC and device paths.
  [String.raw`\\corp-server\private-share\client\config.json`, "<path>"],
  [String.raw`read \\?\C:\Secret\a.txt`, "read <path>"],
  ["//share-host/team share/file", "<path>"],
  // file: URLs, with or without an authority.
  ["file:///opt/private/client/config.json", "file:<path>"],
  ["at file://host/share/x.js:1:2", "at file:<path>"],
  // Paths with spaces, parentheses and apostrophes are replaced whole.
  [String.raw`C:\External Work\Secret Client\config.json`, "<path>"],
  [
    String.raw`open "C:\Program Files (x86)\O'Brien\a.txt" failed`,
    'open "<path>" failed',
  ],
  ["error at (/Users/some one/work/file.js:1:2)", "error at (<path>"],
  // Vite /@fs/ URLs name an absolute path after the prefix.
  [
    "http://127.0.0.1:1431/@fs/C:/External Work/x.js?v=1",
    "http://127.0.0.1:1431/@fs/<path>",
  ],
  // Served URLs and plain text stay readable.
  [
    "request failed: http://127.0.0.1:1431/src/workbench/Workbench.tsx net::ERR_FAILED",
    "request failed: http://127.0.0.1:1431/src/workbench/Workbench.tsx net::ERR_FAILED",
  ],
  [
    "  ➜  Local:   http://127.0.0.1:1431/",
    "  ➜  Local:   http://127.0.0.1:1431/",
  ],
  ["and/or keep / this", "and/or keep / this"],
  // Each line is redacted on its own.
  [
    "first /var/private/a b\nsecond line stays",
    "first <path>\nsecond line stays",
  ],
];

for (const [input, expected] of CASES)
  test(`redacts ${JSON.stringify(input)}`, () => {
    assert.equal(sanitize(input, 1_000), expected);
  });

test("bounds the text after redaction", () => {
  assert.equal(sanitize("x".repeat(12), 10), `${"x".repeat(10)}…`);
  assert.equal(
    sanitize(`${"y".repeat(8)} C:\\Secret Folder\\a`, 9),
    `${"y".repeat(8)} …`,
  );
});
