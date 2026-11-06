import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { Session } from "node:inspector/promises";
import { performance } from "node:perf_hooks";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

import serializeDeveloperResult from "../src-tauri/src/browser/devtools/cdp_port/evaluate.js";

const serializer = serializeDeveloperResult.toString();
const limit = 256 * 1024;
const functions = new WeakMap();
const uninspectable = {
  kind: "object",
  preview: false,
  reason: "not-inspectable-within-budget",
};

test("five million plain-object keys return a bounded marker promptly", async () => {
  await page(async (session) => {
    const { result } = await evaluate(
      session,
      "(() => { const value = {}; for (let i = 0; i < 5000000; i++) value[i] = i; return value; })()",
    );
    const started = performance.now();
    const bounded = await serialize(session, result.objectId);
    const elapsed = performance.now() - started;
    assert.ok(elapsed < 750, `serialization took ${elapsed} ms`);
    assert.deepEqual(JSON.parse(bounded.result.value), {
      result: { type: "object", value: uninspectable },
    });
  });
});

async function page(run) {
  const session = new Session();
  session.connect();
  const post = session.post.bind(session);
  session.post = (method, params = {}) => {
    assert.ok(Buffer.byteLength(JSON.stringify(params)) <= 16_384, method);
    return post(method, params);
  };
  try {
    await run(session);
  } finally {
    session.disconnect();
  }
}

async function evaluate(session, expression, options = {}) {
  return session.post("Runtime.evaluate", {
    expression,
    objectGroup: "bounded-test",
    returnByValue: false,
    ...options,
  });
}

async function serialize(session, objectId) {
  const failure = () => ({
    result: {
      type: "string",
      value:
        '{"result":{"type":"undefined"},"exceptionDetails":{"text":"Result could not be serialized"}}',
    },
  });
  const capacity = () => ({ result: { type: "string", value: "capacity" } });
  const { result: root } = await session.post("Runtime.callFunctionOn", {
    objectId,
    functionDeclaration: "function(){return this;}",
    returnByValue: false,
    throwOnSideEffect: true,
  });
  if (root.subtype === "proxy") return failure();
  const mask = (subtype) => ({ map: 1, set: 2, typedarray: 4 })[subtype] ?? 0;
  let kinds = mask(root.subtype);
  let functionId = functions.get(session);
  if (!functionId) {
    const installed = await session.post("Runtime.evaluate", {
      objectGroup: "bounded-serializer",
      expression: `(${serializer})`,
      returnByValue: false,
      throwOnSideEffect: true,
    });
    functionId = installed.result.objectId;
    assert.ok(functionId);
    functions.set(session, functionId);
  }
  let known = 1;
  let receiver = objectId;
  let resume = false;
  let remaining = 4096;
  for (let pass = 0; pass < 128; pass++) {
    const response = await session.post("Runtime.callFunctionOn", {
      objectId: functionId,
      objectGroup: "bounded-test",
      functionDeclaration:
        "function(root,limit,entries,resume,kinds){return this(root,limit,entries,resume,kinds);}",
      arguments: [
        { objectId: receiver },
        { value: limit },
        { value: remaining },
        { value: resume },
        { value: kinds },
      ],
      returnByValue: false,
      throwOnSideEffect: true,
    });
    if (response.exceptionDetails || response.result.type === "string")
      return response;
    receiver = response.result.objectId;
    const properties = await session.post("Runtime.getProperties", {
      objectId: receiver,
      ownProperties: true,
      generatePreview: false,
    });
    const values = new Map(properties.result.map((p) => [p.name, p.value]));
    remaining -= values.get("0").value;
    for (let i = 1; i < values.get("length").value; i++) {
      const child = values.get(String(i));
      if (child.subtype === "proxy" || !child.objectId) return failure();
      if (known >= 4096) return capacity();
      known++;
      kinds |= mask(child.subtype);
    }
    resume = true;
  }
  return capacity();
}

test("huge sized collections fail closed without transferring their contents", async (t) => {
  // The heap fits the existing inputs, but not an eager numeric-key list.
  // Keep this budget when the enclosing test runner has no heap limit.
  if (!process.execArgv.includes("--max-old-space-size=128")) {
    const { stdout } = await promisify(execFile)(
      process.execPath,
      [
        "--max-old-space-size=128",
        "--test",
        "--experimental-test-isolation=none",
        "--test-concurrency=1",
        `--test-name-pattern=^${t.name}$`,
        fileURLToPath(import.meta.url),
      ],
      {
        timeout: 60_000,
        maxBuffer: 64 * 1024,
        env: { ...process.env, NODE_TEST_CONTEXT: undefined },
      },
    );
    assert.match(stdout, /# tests 1\r?\n/);
    assert.match(stdout, /# pass 1\r?\n/);
    return;
  }
  await page(async (session) => {
    for (const expression of [
      "Array(10000000).fill(42)",
      "new Uint8Array(10000000)",
      `(() => {
        const value = new Map();
        for (let i = 0; i < 1000000; i++) value.set(i, i);
        return value;
      })()`,
      `(() => {
        const value = new Set();
        for (let i = 0; i < 1000000; i++) value.add(i);
        return value;
      })()`,
    ]) {
      const { result } = await evaluate(session, expression);
      try {
        assert.ok(result.objectId);
        assert.equal(
          (await serialize(session, result.objectId)).result.value,
          "capacity",
          expression,
        );
      } finally {
        await session.post("Runtime.releaseObjectGroup", {
          objectGroup: "bounded-test",
        });
        await session.post("HeapProfiler.collectGarbage");
      }
    }
  });
});

test("admitted collections preserve indexed values and iterator order", async () => {
  await page(async (session) => {
    for (const [expression, expected] of [
      ["[42, , undefined]", [42, null, null]],
      ["new Uint8Array([42, 7])", { 0: 42, 1: 7 }],
      [
        "new Map([[42, {answer: true}], ['next', [7]]])",
        [
          [42, uninspectable],
          ["next", [7]],
        ],
      ],
      ["new Set([42, 'next', null])", [42, "next", null]],
      ["new Map()", []],
      ["new Set()", []],
      ["new DataView(new ArrayBuffer(8))", uninspectable],
      [
        "[new Map([[{key: 42}, new Set([7, 9])]]), new Uint8Array([3])]",
        [[[uninspectable, [7, 9]]], { 0: 3 }],
      ],
    ]) {
      const { result } = await evaluate(session, expression);
      const bounded = await serialize(session, result.objectId);
      assert.equal(bounded.exceptionDetails, undefined, expression);
      assert.deepEqual(JSON.parse(bounded.result.value).result.value, expected);
    }
  });
});

test("sized collections admit the entry limit and reject one more", async () => {
  await page(async (session) => {
    for (const count of [4096, 4097]) {
      for (const expression of [
        `Array(${count}).fill(42)`,
        `new Uint8Array(${count}).fill(42)`,
        `new Map(Array.from({length: ${count}}, (_, i) => [i, 42]))`,
        `new Set(Array.from({length: ${count}}, (_, i) => i))`,
      ]) {
        const { result } = await evaluate(session, expression);
        const bounded = await serialize(session, result.objectId);
        if (count === 4097) {
          assert.equal(bounded.result.value, "capacity", expression);
        } else {
          const value = JSON.parse(bounded.result.value).result.value;
          assert.equal(Object.keys(value).length, 4096, expression);
          assert.deepEqual(
            value[0],
            expression.startsWith("new Map")
              ? [0, 42]
              : expression.startsWith("new Set")
                ? 0
                : 42,
          );
          assert.deepEqual(
            value[4095],
            expression.startsWith("new Map")
              ? [4095, 42]
              : expression.startsWith("new Set")
                ? 4095
                : 42,
          );
        }
      }
    }
  });
});

test("Map entry pairs count toward the serialized nesting limit", async () => {
  await page(async (session) => {
    for (const position of [0, 1]) {
      for (const count of [32, 33]) {
        const { result } = await evaluate(
          session,
          `(() => {
            let value = 42;
            for (let i = 0; i < ${count}; i++)
              value = new Map([${position === 0 ? "[value, 0]" : "[0, value]"}]);
            return value;
          })()`,
        );
        const bounded = await serialize(session, result.objectId);
        if (count === 33) {
          assert.equal(bounded.result.value, "capacity");
        } else {
          let value = JSON.parse(bounded.result.value).result.value;
          for (let i = 0; i < count; i++) value = value[0][position];
          assert.equal(value, 42);
        }
      }
    }
  });
});

test("collection sizes cannot be hidden by page accessors or forged by records", async () => {
  await page(async (session) => {
    for (const expression of [
      "new Uint8Array(4097)",
      "new Map(Array.from({length: 4097}, (_, i) => [i, i]))",
      "new Set(Array.from({length: 4097}, (_, i) => i))",
    ]) {
      const { result } = await evaluate(
        session,
        `(() => {
          globalThis.sageCollectionAccessors = 0;
          const value = ${expression};
          for (const name of ['length', 'size', 'entries', 'values', Symbol.toStringTag]) {
            Object.defineProperty(value, name, {
              get() { globalThis.sageCollectionAccessors++; return 0; }
            });
          }
          return value;
        })()`,
      );
      assert.equal(
        (await serialize(session, result.objectId)).result.value,
        "capacity",
      );
      assert.equal(
        (await evaluate(session, "sageCollectionAccessors")).result.value,
        0,
      );
    }
    const { result } = await evaluate(
      session,
      "({length: 10000000, size: 1000000, answer: 42})",
    );
    assert.deepEqual(
      JSON.parse((await serialize(session, result.objectId)).result.value),
      {
        result: {
          type: "object",
          value: uninspectable,
        },
      },
    );
  });
});

test("an object over the cap never transfers its full value", async () => {
  await page(async (session) => {
    const { result } = await evaluate(
      session,
      `Object.defineProperty([Array(100000).fill('xxxxxxxxxxxxxxxx')], 1, {
        get() { throw new Error('must not visit beyond the budget'); }
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

test("plain objects omit all properties without running accessors", async () => {
  await page(async (session) => {
    for (const value of ["undefined", "function() {}", "Symbol('omitted')"]) {
      for (const count of [4096, 4097]) {
        const { result } = await evaluate(
          session,
          `Object.fromEntries(Array.from({length: ${count}}, (_, i) => [i, ${value}]))`,
        );
        const bounded = await serialize(session, result.objectId);
        assert.equal(
          bounded.result.value,
          JSON.stringify({ result: { type: "object", value: uninspectable } }),
        );
      }
    }
    const { result } = await evaluate(
      session,
      `(() => {
        globalThis.sageGetterCalls = 0;
        const value = {};
        for (let i = 0; i < 100000; i++) {
          Object.defineProperty(value, i, {
            enumerable: true,
            get() { globalThis.sageGetterCalls++; }
          });
        }
        return value;
      })()`,
    );
    assert.equal(
      (await serialize(session, result.objectId)).result.value,
      JSON.stringify({ result: { type: "object", value: uninspectable } }),
    );
    assert.equal((await evaluate(session, "sageGetterCalls")).result.value, 0);
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

test("indexed values count JSON escapes and UTF-8", async () => {
  await page(async (session) => {
    for (const expression of [
      "['\\u0000'.repeat(60000)]",
      "['\\u{1f600}'.repeat(70000)]",
      "['x'.repeat(300000)]",
      "[new String('x'.repeat(300000))]",
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
        result: { type: "object", value: uninspectable },
      },
    );
  });
});

test("serializer failures remain bounded and the group can still be released", async () => {
  await page(async (session) => {
    const { result } = await evaluate(
      session,
      "(() => { const value = []; value[0] = value; return value; })()",
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

test("indexed data keeps primitives and holes while records fail closed", async () => {
  await page(async (session) => {
    const { result } = await evaluate(
      session,
      `[42, 'hello', null, true,
        Object.assign(Object.create({inherited: 'not a result'}), {
          missing: undefined, fn() {}, symbol: Symbol('omit'), kept: false,
          ['__proto__']: {ordinary: 'data'}
        }),
        ['text', , undefined, function() {}, Symbol('null'), null, NaN, Infinity]]`,
    );
    assert.deepEqual(
      JSON.parse((await serialize(session, result.objectId)).result.value),
      {
        result: {
          type: "object",
          subtype: "array",
          value: [
            42,
            "hello",
            null,
            true,
            uninspectable,
            ["text", null, null, null, null, null, null, null],
          ],
        },
      },
    );
  });
});

test("side-effect-free evaluation never runs getters or toJSON during serialization", async () => {
  await page(async (session) => {
    await evaluate(
      session,
      `globalThis.sageSerializationState = 0;
       globalThis.sageSerializationValue = {
         answer: 42,
         get change() { globalThis.sageSerializationState++; return 'changed'; },
         toJSON() { globalThis.sageSerializationState++; return 'replaced'; }
       };
       Object.defineProperty(sageSerializationValue, Symbol.toStringTag, {
         get() { globalThis.sageSerializationState++; return 'Custom'; }
       })`,
    );
    const { result } = await evaluate(session, "sageSerializationValue", {
      throwOnSideEffect: true,
    });
    assert.deepEqual(
      JSON.parse((await serialize(session, result.objectId)).result.value),
      { result: { type: "object", value: uninspectable } },
    );
    assert.equal(
      (await evaluate(session, "sageSerializationState")).result.value,
      0,
    );
  });
});

test("boxed primitives keep their values without calling conversion overrides", async () => {
  await page(async (session) => {
    const { result } = await evaluate(
      session,
      `(() => {
        globalThis.sageBoxConversions = 0;
        const values = [new Number(42), new Boolean(false), new String('hello')];
        for (const value of values) {
          Object.defineProperty(value, 'valueOf', {get() { globalThis.sageBoxConversions++; }});
          value.toJSON = () => { globalThis.sageBoxConversions++; return 'changed'; };
        }
        return values;
      })()`,
    );
    assert.deepEqual(
      JSON.parse((await serialize(session, result.objectId)).result.value),
      {
        result: {
          type: "object",
          subtype: "array",
          value: [42, false, "hello"],
        },
      },
    );
    assert.equal(
      (await evaluate(session, "sageBoxConversions")).result.value,
      0,
    );
  });
});

test("a wide result bounds each record across native classification", async () => {
  await page(async (session) => {
    const { result } = await evaluate(
      session,
      "Array.from({length: 1000}, () => ({answer: 42}))",
    );
    const bounded = JSON.parse(
      (await serialize(session, result.objectId)).result.value,
    );
    assert.equal(bounded.result.value.length, 1000);
    assert.deepEqual(bounded.result.value, Array(1000).fill(uninspectable));
  });
});

test("a thousand typed arrays fit the native request cap", async () => {
  await page(async (session) => {
    const { result } = await evaluate(
      session,
      "Array.from({length:1000}, () => new Uint8Array([42]))",
    );
    const bounded = JSON.parse(
      (await serialize(session, result.objectId)).result.value,
    );
    assert.deepEqual(
      bounded.result.value,
      Array.from({ length: 1000 }, () => ({ 0: 42 })),
    );
  });
});

test("proxy traps cannot run during serialization, including nested proxies", async () => {
  await page(async (session) => {
    await evaluate(
      session,
      `globalThis.sageProxyTraps = 0;
       globalThis.sageSerializationProxy = new Proxy({answer: 42}, {
         ownKeys() { globalThis.sageProxyTraps++; return ['answer']; },
         getOwnPropertyDescriptor() { globalThis.sageProxyTraps++; return {value: 42, enumerable: true, configurable: true}; },
         getPrototypeOf() { globalThis.sageProxyTraps++; return null; },
         get() { globalThis.sageProxyTraps++; return undefined; }
       })`,
    );
    for (const expression of [
      "sageSerializationProxy",
      "[sageSerializationProxy]",
      "Object.create(sageSerializationProxy)",
      "new Proxy({answer: 42}, {ownKeys() { return ['answer']; }})",
      "[new Proxy({answer: 42}, {ownKeys() { return ['answer']; }})]",
      "Object.create(new Proxy({}, {ownKeys() { return []; }}))",
      "Object.create(new Proxy(function() {}, {ownKeys() { return []; }}))",
      "new Proxy([42], {getOwnPropertyDescriptor() { return {value: 1}; }})",
    ]) {
      const { result } = await evaluate(session, expression);
      const bounded = await serialize(session, result.objectId);
      assert.ok(
        bounded.exceptionDetails ||
          JSON.parse(bounded.result.value).exceptionDetails,
      );
      assert.equal((await evaluate(session, "sageProxyTraps")).result.value, 0);
    }
  });
});
