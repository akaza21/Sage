import { expect, it } from "vitest";

import { browserFaviconDataUrl, resolveBrowserAddress } from "./browserAddress";

it("opens web addresses and searches ordinary text", () => {
  expect(resolveBrowserAddress("example.com/docs")).toBe(
    "https://example.com/docs",
  );
  expect(resolveBrowserAddress("http://127.0.0.1:3000/")).toBe(
    "http://127.0.0.1:3000/",
  );
  expect(resolveBrowserAddress("rust ownership")).toBe(
    "https://www.google.com/search?q=rust+ownership",
  );
});

it("refuses dangerous schemes and credentials before navigation", () => {
  for (const input of [
    "file:///C:/secret",
    "data:text/html,hello",
    "javascript:alert(1)",
    "https://user:pass@example.com/",
    "https://user@example.com/",
    "https://example.com/\n",
  ]) {
    expect(() => resolveBrowserAddress(input)).toThrow();
  }
});

it("accepts only bounded PNG image data for browser icons", () => {
  expect(browserFaviconDataUrl("data:image/png;base64,iVBORw0KGgo=")).toBe(
    "data:image/png;base64,iVBORw0KGgo=",
  );
  expect(browserFaviconDataUrl("https://example.com/favicon.png")).toBeNull();
  expect(browserFaviconDataUrl("data:image/png;base64,AAAA")).toBeNull();
  expect(
    browserFaviconDataUrl(`data:image/png;base64,${"A".repeat(44_000)}`),
  ).toBeNull();
});
