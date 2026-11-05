import { expect, it } from "vitest";

import { withStyleNonce } from "./styleNonce";

it("restores ordinary element creation when the callback throws", () => {
  const document = globalThis.document.implementation.createHTMLDocument();
  const seed = document.createElement("style");
  seed.nonce = "test-style-nonce";
  document.head.append(seed);
  const createElement = document.createElement;
  const failure = new Error("Style initialization failed");

  expect(() =>
    withStyleNonce(document, () => {
      const style = document.createElement("style");
      expect(style.nonce).toBe(seed.nonce);
      throw failure;
    }),
  ).toThrow(failure);

  expect(document.createElement).toBe(createElement);
  expect(document.createElement("style").nonce).toBe("");
  const div = document.createElement("div");
  document.body.append(div);
  expect(document.body.firstElementChild).toBe(div);
  expect(div.ownerDocument).toBe(document);
});
