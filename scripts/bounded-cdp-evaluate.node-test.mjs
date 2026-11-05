import assert from "node:assert/strict";
import { Session } from "node:inspector/promises";
import test from "node:test";

import serializeDeveloperResult from "../src-tauri/src/browser/devtools/cdp_port/evaluate.js";

const serializer = serializeDeveloperResult.toString();
const limit = 256 * 1024;

async function page(run) {
  const session = new Session();
  session.connect();
  try {
    await run(session);
  } finally {
    session.disconnect();
  }
}

async function evaluate(session, expression) {
  return session.post("Runtime.evaluate", {
    expression,
    objectGroup: "bounded-test",
    returnByValue: false,
  });
}

async function serialize(session, objectId) {
  return session.post("Runtime.callFunctionOn", {
    objectId,
    functionDeclaration: serializer,
    arguments: [{ value: limit }],
    returnByValue: true,
  });
}

test("an object over the cap never transfers its full value", async () => {
  await page(async (session) => {
    const { result } = await evaluate(
      session,
      `({
      large: Array(100000).fill('xxxxxxxxxxxxxxxx'),
      get later() { throw new Error('must not visit beyond the budget'); }
    })`,
    );
    assert.ok(result.objectId);
    assert.equal(result.value, undefined);
    assert.ok(Buffer.byteLength(JSON.stringify(result)) < 1024);
    const bounded = await serialize(session, result.objectId);
    assert.equal(bounded.result.value, "capacity");
    assert.ok(Buffer.byteLength(JSON.stringify(bounded)) < 128);
    await session.post("Runtime.releaseObjectGroup", {
      objectGroup: "bounded-test",
    });
    await assert.rejects(
      session.post("Runtime.getProperties", { objectId: result.objectId }),
    );
  });
});

test("CDP returns strings inline even when returnByValue is false", async () => {
  await page(async (session) => {
    const response = await evaluate(session, "'x'.repeat(300 * 1024)");
    assert.equal(response.result.objectId, undefined);
    assert.ok(response.result.value.length > limit);
    assert.equal((await evaluate(session, "42")).result.value, 42);
  });
});

test("direct evaluation preserves top-level let and const across calls", async () => {
  await page(async (session) => {
    await evaluate(
      session,
      "let sageBoundedLet = 19; const sageBoundedConst = 23;",
    );
    assert.equal(
      (await evaluate(session, "sageBoundedLet + sageBoundedConst")).result
        .value,
      42,
    );
  });
});

test("objects preserve ordinary values and count JSON escapes and UTF-8", async () => {
  await page(async (session) => {
    for (const expression of [
      "({ text: '\\u0000'.repeat(60000) })",
      "({ text: '\\u{1f600}'.repeat(70000) })",
      "({ ['x'.repeat(300000)]: 1 })",
      "({ text: new String('x'.repeat(300000)) })",
    ]) {
      const { result } = await evaluate(session, expression);
      assert.equal(
        (await serialize(session, result.objectId)).result.value,
        "capacity",
      );
    }
    const { result } = await evaluate(
      session,
      "({ answer: 42, text: 'hello' })",
    );
    assert.deepEqual(
      JSON.parse((await serialize(session, result.objectId)).result.value),
      {
        result: { type: "object", value: { answer: 42, text: "hello" } },
      },
    );
  });
});

test("serializer failures remain bounded and the group can still be released", async () => {
  await page(async (session) => {
    const { result } = await evaluate(
      session,
      "({ get fail() { throw 'x'.repeat(300 * 1024); } })",
    );
    const bounded = await serialize(session, result.objectId);
    assert.ok(JSON.parse(bounded.result.value).exceptionDetails);
    assert.ok(Buffer.byteLength(JSON.stringify(bounded)) < 512);
    await session.post("Runtime.releaseObjectGroup", {
      objectGroup: "bounded-test",
    });
    await assert.rejects(
      session.post("Runtime.getProperties", { objectId: result.objectId }),
    );
  });
});
