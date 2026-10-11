import assert from "node:assert/strict";
import test from "node:test";
import { JSDOM } from "jsdom";
import { richContentReadiness } from "./capture-native-chat-rich-content.mjs";

test("capture readiness requires a coloured token in this turn, not an unstyled span or another turn", () => {
  const dom = new JSDOM(
    '<section id="old"><div class="acp-code-block"><code><span style="color: purple">export</span></code></div></section><section id="current"><div class="acp-code-block"><code><span>export</span></code></div></section>',
  );
  const turn = dom.window.document.querySelector("#current");
  const observe = () =>
    richContentReadiness(
      turn,
      (element) => !element.hidden,
      dom.window.getComputedStyle,
    );
  assert.equal(observe().codeHighlighted, false);
  const token = turn.querySelector("span");
  token.style.color = "purple";
  assert.equal(observe().codeHighlighted, true);
  token.hidden = true;
  assert.equal(observe().codeHighlighted, false);
  token.hidden = false;
  token.textContent = " ";
  assert.equal(observe().codeHighlighted, false);
  assert.deepEqual(
    richContentReadiness(null, () => true, dom.window.getComputedStyle),
    { codeHighlighted: false, imageLoaded: false },
  );
  dom.window.close();
});

test("capture readiness requires a visible image with loaded witness dimensions", () => {
  const dom = new JSDOM('<section><img class="acp-image__src"></section>');
  const turn = dom.window.document.querySelector("section");
  const image = turn.querySelector("img");
  const observe = () =>
    richContentReadiness(
      turn,
      (element) => !element.hidden,
      dom.window.getComputedStyle,
    );
  const loaded = (complete, width, height) => {
    Object.defineProperties(image, {
      complete: { configurable: true, value: complete },
      naturalWidth: { configurable: true, value: width },
      naturalHeight: { configurable: true, value: height },
    });
  };
  loaded(false, 320, 180);
  assert.equal(observe().imageLoaded, false);
  loaded(true, 1, 1);
  assert.equal(observe().imageLoaded, false);
  loaded(true, 320, 180);
  assert.equal(observe().imageLoaded, true);
  image.hidden = true;
  assert.equal(observe().imageLoaded, false);
  dom.window.close();
});

for (const [name, content, limit, wrap] of [
  [
    "code blocks",
    '<div class="acp-code-block"><code></code></div>',
    64,
    (value) => value,
  ],
  [
    "code tokens",
    "<span>token</span>",
    512,
    (value) => `<div class="acp-code-block"><code>${value}</code></div>`,
  ],
  ["images", '<img class="acp-image__src">', 64, (value) => value],
]) {
  test(`rich-content readiness admits ${limit} ${name} and rejects overflow before processing`, () => {
    const dom = new JSDOM(`<section>${wrap(content.repeat(limit))}</section>`);
    const turn = dom.window.document.querySelector("section");
    assert.doesNotThrow(() =>
      richContentReadiness(turn, () => true, dom.window.getComputedStyle),
    );
    turn.innerHTML = wrap(content.repeat(limit + 1));
    const visible = () => {
      throw new Error("processed overflow");
    };
    assert.throws(
      () => richContentReadiness(turn, visible, dom.window.getComputedStyle),
      /probe-rich-content-bound/u,
    );
    dom.window.close();
  });
}
