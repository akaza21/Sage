import assert from "node:assert/strict";
import test from "node:test";
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import os from "node:os";
import { fileURLToPath } from "node:url";
import {
  captureEditorFrame,
  seedEditorProject,
} from "./capture-native-editor-capture.mjs";
import {
  parseEditorArgs,
  EDITOR_FILE,
  SEARCH_FILE,
} from "./capture-native-editor-catalog.mjs";
import { editorDom } from "./capture-native-editor-test-support.mjs";
import {
  persistCapture,
  stagingPaths,
} from "./capture-native-chat-evidence.mjs";
import {
  validateEditorProbe,
  admitEditor,
} from "./capture-native-editor-catalog.mjs";
import { captureTiming } from "./capture-native-chat-drive.mjs";

const options = parseEditorArgs([
  "--harness",
  "editor",
  "--state",
  "editor-idle",
  "--width",
  "1888",
]);
function frameRig() {
  const fixture = editorDom();
  const png = Buffer.alloc(24);
  Buffer.from("89504e470d0a1a0a", "hex").copy(png);
  png.write("IHDR", 12);
  png.writeUInt32BE(1888, 16);
  png.writeUInt32BE(1096, 20);
  const size = { width: 1888, height: 1096 };
  let screenshots = 0,
    afterScreenshot = () => {};
  const args = {
    options,
    expression: "editor-observation",
    deadline: Date.now() + 3000,
    identity: {
      nativeBounds: { outer: size, client: size },
      webviewVersion: "154.0.0.1",
      hashes: {
        build: "a".repeat(64),
        fixture: "b".repeat(64),
        source: "c".repeat(40),
        servedAsset: "a".repeat(64),
      },
    },
    cdp: {
      evaluate: async (expression) =>
        expression === "editor-observation" ? fixture.observe() : true,
      screenshot: async () => {
        screenshots++;
        afterScreenshot();
        return png.toString("base64");
      },
    },
  };
  return {
    fixture,
    args,
    screenshots: () => screenshots,
    after: (callback) => {
      afterScreenshot = callback;
    },
  };
}
test("verified frame persists chat-format evidence with native Editor identity", async () => {
  const rig = frameRig(),
    root = mkdtempSync(path.join(os.tmpdir(), "sage-editor-test-"));
  try {
    const captured = await captureEditorFrame(rig.args);
    assert.equal(rig.screenshots(), 1);
    const run = "11111111-1111-4111-8111-111111111111";
    const { directory, ...output } = stagingPaths(
      root,
      run,
      options,
      admitEditor,
    );
    mkdirSync(directory, { recursive: true });
    const result = persistCapture(
      {
        output,
        options,
        run,
        directory: path.relative(root, directory),
        timing: captureTiming(),
        elapsedMs: 20,
        captured,
      },
      writeFileSync,
      validateEditorProbe,
    );
    assert.equal(result.verdict, "captured");
    assert.equal(
      JSON.parse(readFileSync(output.probe, "utf8")).bindingId,
      "native-editor",
    );
    assert.equal(path.basename(output.png), "editor-idle-editor-1888.png");
  } finally {
    rig.fixture.dom.window.close();
    rmSync(root, { recursive: true, force: true });
  }
});
test("mislabelled DOM and DPR mismatch are rejected before screenshot", async () => {
  for (const mutate of [
    (rig) =>
      rig.fixture.document.querySelector('[title="Caret position"]').remove(),
    (rig) =>
      Object.defineProperty(rig.fixture.window, "devicePixelRatio", {
        value: 2,
      }),
  ]) {
    const rig = frameRig();
    try {
      mutate(rig);
      await assert.rejects(
        () => captureEditorFrame(rig.args),
        /editor-state-drift|dpr-mismatch/u,
      );
      assert.equal(rig.screenshots(), 0);
    } finally {
      rig.fixture.dom.window.close();
    }
  }
});
test("state loss during screenshot discards the frame", async () => {
  const rig = frameRig();
  try {
    rig.after(() =>
      rig.fixture.document
        .querySelector('[aria-label="Explorer project tools"]')
        .remove(),
    );
    await assert.rejects(
      () => captureEditorFrame(rig.args),
      /editor-state-drift/u,
    );
  } finally {
    rig.fixture.dom.window.close();
  }
});
test("an invalid evidence envelope is rejected before screenshot", async () => {
  const rig = frameRig();
  try {
    rig.args.identity.webviewVersion = "unknown";
    await assert.rejects(() => captureEditorFrame(rig.args), /probe-identity/u);
    assert.equal(rig.screenshots(), 0);
  } finally {
    rig.fixture.dom.window.close();
  }
});
test("seed uses the real repository file at its original relative path, with a bounded midpoint", () => {
  const root = fileURLToPath(new URL("..", import.meta.url));
  const project = mkdtempSync(path.join(os.tmpdir(), "sage-editor-seed-"));
  try {
    const seed = seedEditorProject(root, project);
    assert.match(seed.sha256, /^[a-f0-9]{64}$/u);
    const source = readFileSync(path.join(root, EDITOR_FILE));
    assert.deepEqual(readFileSync(path.join(project, EDITOR_FILE)), source);
    assert.deepEqual(
      readFileSync(path.join(project, SEARCH_FILE)),
      readFileSync(path.join(root, SEARCH_FILE)),
    );
    assert.ok(
      seed.midLine > 1 &&
        seed.midLine < source.toString("utf8").split("\n").length,
    );
  } finally {
    rmSync(project, { recursive: true, force: true });
  }
});
