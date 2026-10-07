import { expect, it } from "vitest";

import { browserPageIdentity } from "./browserAddress";

it("shows the effective origin and secure transport from a committed URL", () => {
  expect(browserPageIdentity("https://example.com/path?q=1")).toEqual({
    origin: "https://example.com",
    transport: "secure",
    url: "https://example.com/path?q=1",
  });
  expect(browserPageIdentity("http://127.0.0.1:3000/")).toEqual({
    origin: "http://127.0.0.1:3000",
    transport: "not-secure",
    url: "http://127.0.0.1:3000/",
  });
});

it("does not offer a malformed or non-web committed URL for sharing", () => {
  expect(browserPageIdentity("about:blank")).toBeNull();
  expect(browserPageIdentity("https://user:pass@example.com/")).toBeNull();
});
