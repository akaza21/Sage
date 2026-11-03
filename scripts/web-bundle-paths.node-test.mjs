import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { gzipSync } from "node:zlib";
import test from "node:test";
import {
  WEB_BUNDLE_PATH_ROOTS,
  measureWebBundlePaths,
  verifyWebBundlePathBudgets,
} from "./web-bundle-paths.mjs";

async function fixture() {
  const root = await mkdtemp(path.join(tmpdir(), "sage-bundle-paths-"));
  await mkdir(path.join(root, "dist/assets"), { recursive: true });
  const manifest = {};
  const files = new Map();
  for (const key of new Set(Object.values(WEB_BUNDLE_PATH_ROOTS).flat())) {
    const file = `assets/${files.size}.js`;
    const contents = Buffer.from(`export const value = ${files.size};`);
    files.set(file, contents);
    await writeFile(path.join(root, "dist", file), contents);
    manifest[key] = {
      file,
      imports: key === "index.html" ? [] : ["index.html"],
    };
  }
  manifest["index.html"].dynamicImports = Object.keys(manifest).slice(1);
  return { root, manifest, files };
}

test("every Editor path counts what each Editor view loads eagerly", async () => {
  // Written out, not taken from the roots: the tab strip loads with every
  // Editor view, and registeredSurfaces starts Explorer, which imports the
  // glob matcher, as soon as the Editor registers its surfaces.
  const eager = [
    "src/workbench/TabStrip.tsx",
    "src/features/files/ExplorerSurface.tsx",
    "src/domain/pathGlobMatcher.ts",
  ];
  const registered = await readFile(
    new URL("../src/workbench/registeredSurfaces.tsx", import.meta.url),
    "utf8",
  );
  assert.match(registered, /void loadExplorerSurface\(\)/);
  const explorer = await readFile(
    new URL("../src/features/files/ExplorerSurface.tsx", import.meta.url),
    "utf8",
  );
  assert.match(explorer, /import\("\.\.\/\.\.\/domain\/pathGlobMatcher"\)/);
  for (const name of [
    "editorShell",
    "editorDiff",
    "editorSearch",
    "editorFile",
  ]) {
    for (const root of eager)
      assert.ok(
        WEB_BUNDLE_PATH_ROOTS[name].includes(root),
        `${name} is missing ${root}`,
      );
  }
});

test("path closures include static transitive imports once, not dynamic details", async () => {
  const { root, manifest, files } = await fixture();
  try {
    const workbench = "src/workbench/Workbench.tsx";
    const agent = "src/workbench/AgentWorkbenchBody.tsx";
    manifest[workbench].imports.push("shared");
    manifest[agent].imports.push("shared");
    manifest.shared = { file: "assets/shared.js", imports: [workbench] };
    manifest.sharedAlias = { file: "assets/shared.js", imports: [workbench] };
    manifest[agent].imports.push("sharedAlias");
    manifest[agent].dynamicImports = ["details"];
    manifest.details = { file: "assets/details.js" };
    for (const file of ["assets/shared.js", "assets/details.js"]) {
      files.set(file, Buffer.from(file));
      await writeFile(path.join(root, "dist", file), files.get(file));
    }
    const paths = measureWebBundlePaths(root, manifest);
    const expected = [
      "index.html",
      workbench,
      agent,
      "src/features/chat/ChatSurface.tsx",
      "src/features/chat/acp/AcpChatPresentation.tsx",
    ]
      .map((key) => manifest[key].file)
      .concat("assets/shared.js")
      .sort();
    assert.deepEqual(paths.agentOneChat.files, expected);
    assert.equal(
      paths.agentOneChat.rawBytes,
      expected.reduce((sum, file) => sum + files.get(file).length, 0),
    );
    assert.equal(
      paths.agentOneChat.gzipBytes,
      expected.reduce((sum, file) => sum + gzipSync(files.get(file)).length, 0),
    );
    assert.equal(paths.initial.files.length, 1);
    // index, Workbench, TabStrip, Explorer, its glob matcher, FileSurface,
    // CodeMirror runtime, shared.
    assert.equal(paths.editorFile.files.length, 8);
    assert.ok(!paths.agentOneChat.files.includes("assets/details.js"));
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("each path accepts its exact cap and rejects a one-byte regression independently", async () => {
  const { root, manifest } = await fixture();
  try {
    const paths = measureWebBundlePaths(root, manifest);
    const limits = Object.fromEntries(
      Object.entries(paths).map(([name, value]) => [name, value.gzipBytes]),
    );
    assert.doesNotThrow(() => verifyWebBundlePathBudgets(paths, limits));
    for (const name of Object.keys(paths)) {
      assert.throws(
        () =>
          verifyWebBundlePathBudgets(paths, {
            ...limits,
            [name]: limits[name] - 1,
          }),
        new RegExp(`${name}.*exceeds`),
      );
      for (const invalid of [undefined, null, 0, -1, 1.5, "1000"]) {
        assert.throws(
          () =>
            verifyWebBundlePathBudgets(paths, { ...limits, [name]: invalid }),
          /missing or invalid/,
        );
      }
    }
    assert.throws(
      () => verifyWebBundlePathBudgets(paths, undefined),
      /missing or invalid/,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("path measurement fails closed for missing, unreachable, and escaping roots/imports", async () => {
  const { root, manifest } = await fixture();
  try {
    const workbench = "src/workbench/Workbench.tsx";
    const broken = structuredClone(manifest);
    delete broken[workbench];
    assert.throws(() => measureWebBundlePaths(root, broken), /missing/);
    manifest["index.html"].dynamicImports = [];
    assert.throws(() => measureWebBundlePaths(root, manifest), /unreachable/);
    manifest["index.html"].dynamicImports = Object.keys(manifest).slice(1);
    manifest[workbench].imports = ["missing"];
    assert.throws(() => measureWebBundlePaths(root, manifest), /missing/);
    manifest[workbench].imports = ["index.html"];
    manifest[workbench].file = "../escape.js";
    assert.throws(() => measureWebBundlePaths(root, manifest), /escaping/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("named workbench groups resolve folded facades without counting the shared file twice", async () => {
  const { root, manifest } = await fixture();
  try {
    const workbench = "src/workbench/Workbench.tsx";
    const agent = "src/workbench/AgentWorkbenchBody.tsx";
    const group = "_workbench-deps.js";
    manifest[group] = { ...manifest[workbench], name: "workbench-deps" };
    delete manifest[workbench];
    delete manifest[agent];
    manifest["index.html"].dynamicImports = Object.keys(manifest).slice(1);
    const paths = measureWebBundlePaths(root, manifest);
    assert.equal(paths.agentOneChat.files.length, 4);
    // index, the workbench group, TabStrip, Explorer and its glob matcher.
    assert.equal(paths.editorShell.files.length, 5);
    manifest._ambiguous = { ...manifest[group], file: "assets/other.js" };
    assert.throws(
      () => measureWebBundlePaths(root, manifest),
      /path root is ambiguous/,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("ambiguous Agent groups cannot fall back to the shared Workbench group", async () => {
  const { root, manifest } = await fixture();
  try {
    const workbench = "src/workbench/Workbench.tsx";
    const agent = "src/workbench/AgentWorkbenchBody.tsx";
    manifest._workbench = { ...manifest[workbench], name: "workbench-deps" };
    manifest._agentOne = { ...manifest[agent], name: "agent-workbench" };
    manifest._agentTwo = { ...manifest[agent], name: "agent-workbench" };
    delete manifest[workbench];
    delete manifest[agent];
    manifest["index.html"].dynamicImports = Object.keys(manifest).slice(1);
    assert.throws(
      () => measureWebBundlePaths(root, manifest),
      /path root is ambiguous.*AgentWorkbenchBody/,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("folded chat presentation and editor runtime roots retain their static closures", async () => {
  const { root, manifest } = await fixture();
  try {
    for (const [key, name] of [
      ["src/features/chat/acp/AcpChatPresentation.tsx", "chat-ui"],
      ["src/features/files/CodeMirrorEditorRuntime.ts", "codemirror-deps"],
    ]) {
      manifest[`_${name}`] = { ...manifest[key], name };
      delete manifest[key];
    }
    manifest["index.html"].dynamicImports = Object.keys(manifest).slice(1);
    const paths = measureWebBundlePaths(root, manifest);
    assert.equal(paths.agentOneChat.files.length, 5);
    assert.equal(paths.editorFile.files.length, 7);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("Agent roots folded into one group count its static files once and exclude click-only diffs", async () => {
  const { root, manifest, files } = await fixture();
  try {
    const workbench = "src/workbench/Workbench.tsx";
    const agent = "src/workbench/AgentWorkbenchBody.tsx";
    manifest._agent = {
      ...manifest[agent],
      name: "agent-workbench",
      imports: [workbench],
      dynamicImports: ["snapshot"],
    };
    for (const key of [
      agent,
      "src/features/chat/ChatSurface.tsx",
      "src/features/chat/acp/AcpChatPresentation.tsx",
    ])
      delete manifest[key];
    manifest.snapshot = { file: "assets/snapshot.js" };
    await writeFile(path.join(root, "dist/assets/snapshot.js"), "saved diff");
    manifest["index.html"].dynamicImports = Object.keys(manifest).slice(1);
    const paths = measureWebBundlePaths(root, manifest);
    const expected = [
      manifest["index.html"].file,
      manifest[workbench].file,
      manifest._agent.file,
    ].sort();
    assert.deepEqual(paths.agentOneChat.files, expected);
    assert.equal(
      paths.agentOneChat.rawBytes,
      expected.reduce((sum, file) => sum + files.get(file).length, 0),
    );
    assert.equal(
      paths.agentOneChat.gzipBytes,
      expected.reduce((sum, file) => sum + gzipSync(files.get(file)).length, 0),
    );
    assert.ok(!paths.agentOneChat.files.includes("assets/snapshot.js"));
    manifest._ambiguous = { ...manifest._agent, file: "assets/other.js" };
    assert.throws(
      () => measureWebBundlePaths(root, manifest),
      /path root is ambiguous/,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("manifest traversal accepts bounded deep graphs and rejects the next record or edge", async () => {
  const { root, manifest } = await fixture();
  try {
    const records = 4096;
    const edges = 16384;
    const existingCount = Object.keys(manifest).length;
    manifest["index.html"].imports = ["chain0"];
    for (let index = 0; index < records - existingCount; index++) {
      manifest[`chain${index}`] = {
        file: manifest["index.html"].file,
        imports:
          index < records - existingCount - 1 ? [`chain${index + 1}`] : [],
      };
    }
    assert.doesNotThrow(() => measureWebBundlePaths(root, manifest));
    manifest.overflow = { file: manifest["index.html"].file };
    assert.throws(
      () => measureWebBundlePaths(root, manifest),
      /manifest record limit/,
    );
    delete manifest.overflow;
    for (const record of Object.values(manifest)) record.imports = [];
    manifest["index.html"].imports = Array(edges - existingCount + 1).fill(
      "index.html",
    );
    assert.doesNotThrow(() => measureWebBundlePaths(root, manifest));
    manifest["index.html"].imports.push("index.html");
    assert.throws(
      () => measureWebBundlePaths(root, manifest),
      /manifest import limit/,
    );
    const oversized = Array(edges + 1).fill("index.html");
    Object.defineProperty(oversized, 0, {
      get() {
        throw new Error("Oversized imports must not be inspected");
      },
    });
    manifest["index.html"].imports = oversized;
    assert.throws(
      () => measureWebBundlePaths(root, manifest),
      /manifest import limit/,
    );
    manifest["index.html"].imports = "not-an-array";
    assert.throws(
      () => measureWebBundlePaths(root, manifest),
      /manifest import list is malformed/,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("committed path caps tighten startup and Agent within the original 0.5 percent ceilings", async () => {
  const baseline = {
    initial: 103957,
    agentOneChat: 210654,
    editorShell: 187184,
    editorDiff: 190343,
    editorSearch: 192978,
    editorFile: 289551,
  };
  const budgets = JSON.parse(
    await readFile(
      new URL("../docs/performance/budgets.json", import.meta.url),
      "utf8",
    ),
  );
  assert.deepEqual(budgets.bundle.webPathJsGzipBytes, {
    ...Object.fromEntries(
      Object.entries(baseline).map(([name, bytes]) => [
        name,
        Math.floor(bytes * 1.005),
      ]),
    ),
    initial: 104423,
    agentOneChat: 208434,
  });
  assert.equal(
    budgets.measurement.webBundlePaths.integrationAdjustment,
    undefined,
  );
  assert.equal(budgets.bundle.webTotalJsGzipBytes, 461000);
  // Codex §7 Subagents (w8sub): measured 5,101 B + 10%.
  assert.equal(budgets.bundle.webSubagentLazyJsGzipBytes, 5612);
  // The terminal follows forced-colors palette changes (89,516 B measured
  // with hashed import names normalized; 89,579 B as emitted before).
  assert.equal(budgets.bundle.webTerminalLazyJsGzipBytes, 89516);
});
