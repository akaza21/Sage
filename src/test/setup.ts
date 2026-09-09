import "@testing-library/jest-dom/vitest";

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
