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
    const entries = Object.entries(files);
    const folders = new Set(entries.map(([name]) => path.dirname(name)));
    for (const folder of folders)
      await mkdir(path.join(root, folder), { recursive: true });
    // Written in bounded batches so wide fixtures stay quick without
    // exhausting file handles.
    for (let start = 0; start < entries.length; start += 64) {
      await Promise.all(
        entries
          .slice(start, start + 64)
          .map(([name, contents]) =>
            writeFile(path.join(root, name), contents),
          ),
      );
    }
    return await run(root);
  } finally {
    await rm(root, { recursive: true, force: true, maxRetries: 3 });
  }
}

const scan = (root, limits) => q02SharedHelperPolicyErrors(root, { limits });

for (const { label, cap, count } of [
  { label: "3 MiB", cap: 3_145_728, count: 96 },
  { label: "2.5 MiB", cap: 2_621_440, count: 80 },
]) {
  test(`configured ${label} aggregate source cap accepts exact and fails closed just over`, async () => {
    const limits = { aggregateBytes: cap };
    const last = `src/f${String(count - 1).padStart(3, "0")}.ts`;
    const files = Object.fromEntries(
      Array.from({ length: count }, (_, index) => [
        `src/f${String(index).padStart(3, "0")}.ts`,
        " ".repeat(index === count - 1 ? 32_767 : 32_768),
      ]),
    );
    await withFixture(files, async (root) => {
      assert.deepEqual(await scan(root, limits), []);
      await writeFile(path.join(root, last), " ".repeat(32_768));
      assert.deepEqual(await scan(root, limits), []);
      await writeFile(path.join(root, last), " ".repeat(32_769));
      assert.deepEqual(await scan(root, limits), [
        `Q02 aggregate limit ${cap} exceeded at ${last}`,
      ]);
    });
  });
}

test("a tree past the scanner's former totals is still checked in full", async () => {
  // 1,100 modules, 1,300 entries and over 3 MiB, all above the totals that
  // used to stop the scan, with a violation in the last module read.
  const files = Object.fromEntries(
    Array.from({ length: 1_100 }, (_, index) => [
      `src/m${index % 2}/f${String(index).padStart(4, "0")}.ts`,
      index === 1_099 ? "function isRecord() {}" : " ".repeat(3_000),
    ]),
  );
  for (let index = 0; index < 200; index += 1)
    files[`src/m0/f${String(index).padStart(4, "0")}.test.ts`] = "";
  await withFixture(files, async (root) => {
    assert.deepEqual(await scan(root), [
      "Q02 forbidden isRecord at src/m1/f1099.ts",
    ]);
  });
});

test("one directory's listing is bounded and fails closed past its cap", async () => {
  const files = (count) =>
    Object.fromEntries(
      Array.from({ length: count }, (_, index) => [
        `src/f${String(index).padStart(4, "0")}.ts`,
        "",
      ]),
    );
  await withFixture(files(1_024), async (root) => {
    assert.deepEqual(await scan(root), []);
  });
  await withFixture(files(1_025), async (root) => {
    assert.match(
      (await scan(root))[0],
      /^Q02 directory-entry limit 1024 exceeded at src\//u,
    );
  });
});

test("wide fan-out of files the policy ignores still exhausts the entry budget", async () => {
  // Ten directories, each under its listing cap, of tests and scripts that are
  // never read as modules, so no module or byte total grows. Listing them is
  // still work, and the scan stops once the entry budget is spent.
  const files = {};
  for (let folder = 0; folder < 10; folder += 1)
    for (let index = 0; index < 1_000; index += 1)
      files[
        `src/d${folder}/f${String(index).padStart(4, "0")}.${index % 2 ? "test.ts" : "js"}`
      ] = "";
  await withFixture(files, async (root) => {
    // src lists 10 entries and d0 to d8 list 9,000 more, so the budget
    // runs out partway through d9, in whatever order the filesystem lists it.
    const errors = await scan(root);
    assert.equal(errors.length, 1);
    assert.match(
      errors[0],
      new RegExp(
        `^Q02 entry limit ${Q02_LIMITS.entries} exceeded at src/d9/`,
        "u",
      ),
    );
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
        // The budget runs out while src is still being listed, in the
        // filesystem's order, so any of its entries may be the one named.
        (await scan(root, { ...Q02_LIMITS, entries: 2 }))[0],
        /^Q02 entry limit 2 exceeded at src\/[abc]\.ts$/u,
      );
      assert.match(
        (await scan(root, { ...Q02_LIMITS, entries: 3 }))[0],
        /forbidden isRecord at src\/c/u,
      );
      assert.match(
        (await scan(root, { ...Q02_LIMITS, directoryEntries: 2 }))[0],
        /directory-entry/u,
      );
      assert.match(
        (await scan(root, { ...Q02_LIMITS, directoryEntries: 3 }))[0],
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
    assert.deepEqual(await scan(root, { ...Q02_LIMITS, entries: 2 }), []);
  });
});
test("current repository is clean", async () => {
  assert.deepEqual(await q02SharedHelperPolicyErrors(process.cwd()), []);
});
