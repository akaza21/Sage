import "@testing-library/jest-dom/vitest";
import "./markdownCorpus";

import { beforeAll, beforeEach, vi } from "vitest";

import { resetHarnessModelCache } from "../app/harnessModelCache";

// Some modules start a lazy load when they are imported (Explorer starts
// with the workbench). Let those land before the first test, as they do long
// before a person can act in the app, so no test depends on test order.
beforeAll(async () => {
  await vi.dynamicImportSettled();
});

beforeEach(() => {
  resetHarnessModelCache();
});

// jsdom does not implement Range geometry, but CodeMirror schedules layout
// measurements through these APIs after editor updates. Keep the shared test
// environment deterministic so a late animation-frame measurement cannot
// surface as an unhandled exception after an otherwise-passing test.
if (typeof Range !== "undefined") {
  Object.defineProperties(Range.prototype, {
    getBoundingClientRect: {
      configurable: true,
      value: () => new DOMRect(),
    },
    getClientRects: {
      configurable: true,
      value: () => [] as unknown as DOMRectList,
    },
  });
}
