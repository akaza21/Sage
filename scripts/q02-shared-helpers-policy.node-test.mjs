import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import process from "node:process";
import test from "node:test";
import {
  Q02_LIMITS,
  q02SharedHelperPolicyErrors,
} from "./q02-shared-helpers-policy.mjs";

async function withFixture(files, run) {
  const root = await mkdtemp(path.join(tmpdir(), "q02-policy-"));
  try {
    for (const [name, contents] of Object.entries(files)) {
      const target = path.join(root, name);
      await mkdir(path.dirname(target), { recursive: true });
      await writeFile(target, contents);
    }
    return await run(root);
  } finally {
    await rm(root, { recursive: true, force: true, maxRetries: 3 });
  }
}

const scan = (root, limits) => q02SharedHelperPolicyErrors(root, { limits });

test("default production file cap accepts 384 and rejects 385", async () => {
  const files = Array.from({ length: 385 }, (_, index) => [
    `src/f${String(index).padStart(3, "0")}.ts`,
    "",
  ]);
  await withFixture(Object.fromEntries(files.slice(0, 384)), async (root) => {
    assert.deepEqual(await scan(root), []);
  });
  await withFixture(Object.fromEntries(files), async (root) => {
    assert.deepEqual(await scan(root), [
      "Q02 file limit 384 exceeded at src/f384.ts",
    ]);
  });
});

test("clean scope admits exact owners and ignores test/spec/fixture/JS", async () => {
  await withFixture(
    {
      "src/domain/guards.ts": "function isRecord() {}",
      "src/domain/errors.ts":
        "function errorMessage() {} function boundedError() {}",
      "src/domain/text.ts":
        "const encoder = new TextEncoder(); function utf8ByteLength() {}",
      "src/ports/nativeInvoke.ts": "import '@tauri-apps/api/core'",
      "src/x.test.ts": "function isRecord() {}",
      "src/y.spec.tsx": "new TextEncoder()",
      "src/z.fixture.ts": "new TextEncoder()",
      "src/q.js": "new TextEncoder()",
    },
    async (root) => assert.deepEqual(await scan(root), []),
  );
});

test("ownership is per rule, not a blanket canonical-file exemption", async () => {
  await withFixture(
    {
      "src/domain/guards.ts":
        "function isRecord() {} import '@tauri-apps/api/core'",
      "src/ports/nativeInvoke.ts":
        "import '@tauri-apps/api/core'; new TextEncoder()",
      "src/a.ts": "function isRecord() {}",
    },
    async (root) => {
      const errors = await scan(root);
      assert.deepEqual(errors, [
        "Q02 forbidden isRecord at src/a.ts",
        "Q02 forbidden coreImport at src/domain/guards.ts",
        "Q02 forbidden encoder at src/ports/nativeInvoke.ts",
      ]);
    },
  );
});

test("specialized non-array predicate is exact and errors are bounded", async () => {
  await withFixture(
    {
      "src/a.ts": "function isRecord() {}",
      "src/b.ts": "function errorMessage() {}",
      "src/features/chatSearch/chatSearchDomain.ts":
        "function isNonArrayRecord() {}",
    },
    async (root) => {
      const errors = await scan(root, { ...Q02_LIMITS, errors: 1 });
      assert.deepEqual(errors, ["Q02 forbidden isRecord at src/a.ts"]);
    },
  );
});

test("lexical traversal uses stable code-unit order and stops at every bound", async () => {
  await withFixture(
    { "src/a.ts": "", "src/b.ts": "", "src/c.ts": "function isRecord() {}" },
    async (root) => {
      assert.match(
        (await scan(root, { ...Q02_LIMITS, files: 2 }))[0],
        /c\.ts$/u,
      );
      assert.match(
        (await scan(root, { ...Q02_LIMITS, files: 3 }))[0],
        /forbidden isRecord at src\/c/u,
      );
      assert.match(
        (await scan(root, { ...Q02_LIMITS, visitedEntries: 2 }))[0],
        /visited-entry/u,
      );
      assert.match(
        (await scan(root, { ...Q02_LIMITS, visitedEntries: 3 }))[0],
        /forbidden isRecord/u,
      );
    },
  );
  await withFixture({ "src/a.ts": "xxxx" }, async (root) => {
    assert.deepEqual(await scan(root, { ...Q02_LIMITS, fileBytes: 4 }), []);
    assert.match(
      (await scan(root, { ...Q02_LIMITS, fileBytes: 3 }))[0],
      /per-file/u,
    );
  });
  await withFixture({ "src/a.ts": "xx", "src/b.ts": "xx" }, async (root) => {
    assert.deepEqual(
      await scan(root, { ...Q02_LIMITS, aggregateBytes: 4 }),
      [],
    );
    assert.match(
      (await scan(root, { ...Q02_LIMITS, aggregateBytes: 3 }))[0],
      /aggregate/u,
    );
  });
  await withFixture({ "src/a/b/c.ts": "" }, async (root) => {
    assert.deepEqual(await scan(root, { ...Q02_LIMITS, depth: 2 }), []);
    assert.match((await scan(root, { ...Q02_LIMITS, depth: 1 }))[0], /depth/u);
  });
});

test("root and child symlinks fail closed without mutating a missing root", async () => {
  await withFixture({}, async (root) => {
    assert.deepEqual(await scan(root), ["Q02 source directory missing: src"]);
    await writeFile(path.join(root, "src"), "not a directory");
    assert.deepEqual(await scan(root), [
      "Q02 source entry is not a directory: src",
    ]);
  });
  const directory = {
    isSymbolicLink: () => false,
    isDirectory: () => true,
    dev: 1,
    ino: 1,
  };
  const symlink = {
    isSymbolicLink: () => true,
    isDirectory: () => false,
    isFile: () => false,
  };
  const nonRegular = {
    isSymbolicLink: () => false,
    isDirectory: () => false,
    isFile: () => false,
  };
  const iterable = {
    async *[Symbol.asyncIterator]() {
      yield { name: "child.ts" };
    },
  };
  const root = await mkdtemp(path.join(tmpdir(), "q02-policy-stat-"));
  try {
    assert.deepEqual(
      await q02SharedHelperPolicyErrors(root, {
        fileSystem: { lstat: async () => symlink },
      }),
      ["Q02 source directory is a symlink: src"],
    );
    assert.deepEqual(
      await q02SharedHelperPolicyErrors(root, {
        fileSystem: {
          lstat: async (target) =>
            target.endsWith("src") ? directory : symlink,
          opendir: async () => iterable,
        },
      }),
      ["Q02 source entry is a symlink: src/child.ts"],
    );
    assert.deepEqual(
      await q02SharedHelperPolicyErrors(root, {
        fileSystem: {
          lstat: async (target) =>
            target.endsWith("src") ? directory : nonRegular,
          opendir: async () => iterable,
        },
      }),
      ["Q02 non-regular source entry: src/child.ts"],
    );
  } finally {
    await rm(root, { recursive: true, force: true, maxRetries: 3 });
  }
});

test("observed source directories reject replacement during enumeration or child processing", async () => {
  const directory = (ino) => ({
    isSymbolicLink: () => false,
    isDirectory: () => true,
    dev: 1,
    ino,
  });
  const file = {
    isSymbolicLink: () => false,
    isDirectory: () => false,
    isFile: () => true,
    dev: 1,
    ino: 3,
    size: 0,
  };
  const empty = { async *[Symbol.asyncIterator]() {} };
  const oneIgnoredFile = {
    async *[Symbol.asyncIterator]() {
      yield { name: "ignored.js" };
    },
  };
  const root = await mkdtemp(path.join(tmpdir(), "q02-policy-replace-"));
  try {
    let enumerationReads = 0;
    assert.deepEqual(
      await q02SharedHelperPolicyErrors(root, {
        fileSystem: {
          lstat: async () =>
            enumerationReads++ === 0 ? directory(1) : directory(2),
          opendir: async () => empty,
        },
      }),
      ["Q02 source directory identity changed: src"],
    );
    let rootReads = 0;
    assert.deepEqual(
      await q02SharedHelperPolicyErrors(root, {
        fileSystem: {
          lstat: async (target) => {
            if (target.endsWith("src")) {
              rootReads += 1;
              return rootReads < 3 ? directory(1) : directory(2);
            }
            return file;
          },
          opendir: async () => oneIgnoredFile,
        },
      }),
      ["Q02 source directory identity changed: src"],
    );
  } finally {
    await rm(root, { recursive: true, force: true, maxRetries: 3 });
  }
  await withFixture({ "src/a.ts": "", "src/b.ts": "" }, async (root) => {
    assert.deepEqual(await scan(root, { ...Q02_LIMITS, files: 2 }), []);
  });
});
test("current repository is clean", async () => {
  assert.deepEqual(await q02SharedHelperPolicyErrors(process.cwd()), []);
});
