import assert from "node:assert/strict";
import test from "node:test";
import {
  validateDimensions,
  validatePng,
} from "./capture-native-chat-contract.mjs";

for (const [width, height] of [
  [1888, 1096],
  [720, 1000],
  [480, 1000],
]) {
  test(`DPR-1 ${width} capture requires the requested client and CSS size`, () => {
    const dimensions = {
      outer: { width: width + 16, height: height + 39 },
      client: { width, height },
      css: { width, height },
    };
    assert.doesNotThrow(() => validateDimensions(dimensions, width, height, 1));
    const old = {
      outer: { width, height },
      client: { width: width - 16, height: height - 39 },
      css: { width: width - 16, height: height - 39 },
    };
    assert.throws(
      () => validateDimensions(old, width, height, 1),
      /resize-clamp/u,
    );
    const clamped = {
      ...dimensions,
      client: { width: width + 8, height },
      css: { width: width + 8, height },
    };
    assert.throws(
      () => validateDimensions(clamped, width, height, 1),
      /resize-clamp/u,
    );
    assert.throws(
      () => validateDimensions(dimensions, width, height, 1.25),
      /dpr-mismatch/u,
    );
    assert.throws(
      () =>
        validateDimensions(
          { ...dimensions, css: { width: width - 1, height } },
          width,
          height,
          1,
        ),
      /viewport-mismatch/u,
    );
    const png = Buffer.alloc(24);
    Buffer.from("89504e470d0a1a0a", "hex").copy(png);
    png.write("IHDR", 12);
    png.writeUInt32BE(width, 16);
    png.writeUInt32BE(height, 20);
    assert.doesNotThrow(() => validatePng(png, { width, height }));
    png.writeUInt32BE(width - 16, 16);
    assert.throws(() => validatePng(png, { width, height }), /png-viewport/u);
    png.writeUInt32BE(width, 16);
    png.writeUInt32BE(height - 39, 20);
    assert.throws(() => validatePng(png, { width, height }), /png-viewport/u);
  });
}
