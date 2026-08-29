import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawn, spawnSync } from "node:child_process";
import { EventEmitter } from "node:events";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  renameSync,
  readdirSync,
  readFileSync,
  truncateSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { PassThrough } from "node:stream";

import {
  admitCdpTarget,
  assertEvidenceRoot,
  attachPlaywright,
  JOURNEY_STEPS,
  createJourneyReceipt,
  decodeBoundedScreenshot,
  hashExecutable,
  parseJourneyArguments,
  processIdentityHelper,
  revalidateControlledCdpTarget,
  runBoundedTool,
  validateExecutableBinding,
  validateCdpTarget,
  writePendingJourneyReceipt,
} from "./source-functional-native-ide-journey.mjs";

test("native IDE journey requires an explicit bounded CDP launch contract", () => {
  assert.throws(
    () => parseJourneyArguments(["--port", "9229"]),
    /journey-arguments-invalid/,
  );
  const parsed = parseJourneyArguments([
    "--port",
    "9229",
    "--output",
    "C:\\evidence",
    "--source-commit",
    "a".repeat(40),
    "--source-tree",
    "b".repeat(40),
    "--executable",
    "C:\\candidate\\sage.exe",
    "--fixture",
    "C:\\fixture",
    "--source-root",
    "C:\\source",
    "--playwright-cli",
    "C:\\tools\\playwright-cli.cmd",
    "--executable-bytes",
    "123",
    "--executable-sha256",
    "c".repeat(64),
    "--expected-origin",
    "http://127.0.0.1:1420",
    "--runtime-mode",
    "tauri-dev-split",
    "--frontend-ready",
    "true",
    "--receipt-name",
    "machine-receipt.pending.json",
  ]);
  assert.equal(parsed.port, 9229);
  assert.equal(parsed.output, "C:\\evidence");
  assert.equal(parsed.deadlineMs, 90_000);
  assert.equal(parsed.expectedOrigin, "http://127.0.0.1:1420/");
  assert.equal(parsed.runtimeMode, "tauri-dev-split");
  assert.equal(parsed.frontendReady, true);
  assert.equal(parsed.receiptName, "machine-receipt.pending.json");
  assert.deepEqual(parsed.steps, JOURNEY_STEPS);
});

test("native IDE receipt binds a completed ordered journey and artifact hashes", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-ide-journey-"));
  try {
    const screenshot = path.join(root, "journey.png");
    const log = path.join(root, "cdp.json");
    writeFileSync(screenshot, "screenshot");
    writeFileSync(log, "cdp log");
    const receipt = createJourneyReceipt({
      source: { commit: "a".repeat(40), tree: "b".repeat(40) },
      executable: {
        path: "C:\\candidate\\sage.exe",
        bytes: 123,
        sha256: "c".repeat(64),
      },
      fixture: { path: "C:\\fixture", sha256: "d".repeat(64) },
      cdp: {
        port: 9229,
        targetId: "page-1",
        origin: "http://127.0.0.1:1420/",
      },
      expectedOrigin: "http://127.0.0.1:1420/",
      runtime: runtimeReceipt(),
      observations: observed(),
      milestones: JOURNEY_STEPS.map((step, index) => ({
        step,
        ordinal: index + 1,
      })),
      artifacts: [screenshot, log],
    });
    assert.equal(receipt.completed, true);
    assert.deepEqual(
      receipt.milestones.map(({ step }) => step),
      JOURNEY_STEPS,
    );
    assert.equal(receipt.artifacts.length, 2);
    assert.match(receipt.artifacts[0].sha256, /^[a-f0-9]{64}$/u);
    assert.equal("path" in receipt.artifacts[0], false);
    assert.equal(receipt.sourceBinding, "collection-only");
    assert.deepEqual(receipt.runtime, runtimeReceipt());
    assert.deepEqual(Object.keys(receipt.runtime).sort(), [
      "frontendReady",
      "mode",
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("native IDE receipt refuses reordered milestones and evidence outside its root", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-ide-journey-"));
  try {
    const artifact = path.join(root, "journey.png");
    writeFileSync(artifact, "screenshot");
    const base = {
      source: { commit: "a".repeat(40), tree: "b".repeat(40) },
      executable: {
        path: "C:\\candidate\\sage.exe",
        bytes: 123,
        sha256: "c".repeat(64),
      },
      fixture: { path: "C:\\fixture", sha256: "d".repeat(64) },
      cdp: {
        port: 9229,
        targetId: "page-1",
        origin: "http://127.0.0.1:1420/",
      },
      expectedOrigin: "http://127.0.0.1:1420/",
      runtime: runtimeReceipt(),
      artifacts: [artifact],
      evidenceRoot: root,
      observations: observed(),
    };
    assert.throws(
      () =>
        createJourneyReceipt({
          ...base,
          milestones: JOURNEY_STEPS.map((step, index) => ({
            step: index === 0 ? JOURNEY_STEPS[1] : step,
            ordinal: index + 1,
          })),
        }),
      /journey-milestones-invalid/,
    );
    assert.throws(
      () =>
        createJourneyReceipt({
          ...base,
          artifacts: [path.join(root, "..", "outside.png")],
          milestones: JOURNEY_STEPS.map((step, index) => ({
            step,
            ordinal: index + 1,
          })),
        }),
      /journey-artifact-invalid/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("native IDE receipt refuses unobserved save, diagnostic, and Diff claims", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-ide-journey-"));
  try {
    const artifact = path.join(root, "journey.png");
    const log = path.join(root, "cdp.json");
    writeFileSync(artifact, "screenshot");
    writeFileSync(log, "log");
    assert.throws(
      () =>
        createJourneyReceipt({
          source: { commit: "a".repeat(40), tree: "b".repeat(40) },
          executable: {
            path: "C:\\candidate\\sage.exe",
            bytes: 1,
            sha256: "c".repeat(64),
          },
          fixture: { path: "C:\\fixture", sha256: "d".repeat(64) },
          cdp: {
            port: 9229,
            targetId: "page-1",
            origin: "http://127.0.0.1:1420/",
          },
          expectedOrigin: "http://127.0.0.1:1420/",
          runtime: runtimeReceipt(),
          milestones: JOURNEY_STEPS.map((step, index) => ({
            step,
            ordinal: index + 1,
          })),
          artifacts: [artifact, log],
          evidenceRoot: root,
          observations: {
            ...observed(),
            save: { ...observed().save, saveDisabled: false },
          },
        }),
      /journey-observations-invalid/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("native IDE screenshot decoding refuses encoded payloads beyond the byte bound", () => {
  assert.throws(
    () => decodeBoundedScreenshot("A".repeat(12 * 1024 * 1024)),
    /journey-screenshot-oversize/,
  );
});

test("native IDE journey admits only the selected loopback CDP websocket", () => {
  validateCdpTarget(
    {
      id: "page-1",
      webSocketDebuggerUrl: "ws://127.0.0.1:9229/devtools/page/page-1",
    },
    9229,
  );
  validateCdpTarget(
    {
      id: "page/encoded",
      webSocketDebuggerUrl: "ws://127.0.0.1:9229/devtools/page/page%2Fencoded",
    },
    9229,
  );
  for (const [id, url] of [
    ["page-1", "ws://localhost:9229/devtools/page/page-1"],
    ["page-1", "ws://127.1:9229/devtools/page/page-1"],
    ["page-1", "wss://127.0.0.1:9229/devtools/page/page-1"],
    ["page-1", "ws://127.0.0.1:9228/devtools/page/page-1"],
    ["page-1", "ws://127.0.0.1:9229/devtools/browser/page-1"],
    ["page-1", "ws://127.0.0.1:9229/devtools/page/other"],
    ["page-1", "ws://127.0.0.1:9229/devtools/page/page-1/"],
    ["page-1", "ws://127.0.0.1:9229/devtools/page/page-1?query=1"],
    ["page-1", "ws://127.0.0.1:9229/devtools/page/page-1#fragment"],
  ]) {
    assert.throws(
      () => validateCdpTarget({ id, webSocketDebuggerUrl: url }, 9229),
      /journey-cdp-target-invalid/,
    );
  }
});

test("native IDE journey admits exactly one page at the derived development origin", () => {
  const page = (id, url) => ({
    id,
    type: "page",
    title: "Sage",
    url,
    webSocketDebuggerUrl: `ws://127.0.0.1:9229/devtools/page/${id}`,
  });
  for (const [expectedOrigin, targetUrl] of [
    ["http://127.0.0.1:1420", "http://127.0.0.1:1420/"],
    ["http://127.0.0.1:1420/", "http://127.0.0.1:1420"],
  ]) {
    assert.equal(
      admitCdpTarget([page("one", targetUrl)], 9229, expectedOrigin).id,
      "one",
    );
  }
  for (const wrongOrigin of [
    "http://localhost:1420/",
    "http://127.0.0.1:1421/",
    "http://127.0.0.1:1420/path",
    "http://127.0.0.1:1420/?query=1",
    "http://tauri.localhost/",
  ]) {
    assert.throws(
      () =>
        admitCdpTarget(
          [page("wrong", wrongOrigin)],
          9229,
          "http://127.0.0.1:1420/",
        ),
      /journey-cdp-page-count-0/,
    );
  }
  for (const wrongPage of [
    { ...page("wrong-type", "http://127.0.0.1:1420/"), type: "other" },
    { ...page("wrong-title", "http://127.0.0.1:1420/"), title: "sage" },
  ]) {
    assert.throws(
      () => admitCdpTarget([wrongPage], 9229, "http://127.0.0.1:1420/"),
      /journey-cdp-page-count-0/,
    );
  }
  assert.throws(
    () =>
      admitCdpTarget(
        [
          page("one", "http://127.0.0.1:1420/"),
          page("two", "http://127.0.0.1:1420/"),
        ],
        9229,
        "http://127.0.0.1:1420/",
      ),
    /journey-cdp-page-count-2/,
  );
});

test("native IDE journey binds the Playwright-controlled page to the stable admitted target", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-ide-playwright-"));
  const output = path.join(root, "local-evidence", "journey");
  mkdirSync(output, { recursive: true });
  const calls = [];
  const controlled = {
    id: "page-1",
    type: "page",
    title: "Sage",
    url: "http://127.0.0.1:1420/",
  };
  const screenshotBytes = Buffer.from("native-ide-screenshot");
  const page = {
    context: () => ({
      newCDPSession: async (actualPage) => {
        assert.equal(actualPage, page);
        return {
          send: async (method) => {
            assert.equal(method, "Target.getTargetInfo");
            return {
              targetInfo: {
                targetId: controlled.id,
                type: controlled.type,
                title: controlled.title,
                url: controlled.url,
              },
            };
          },
          detach: async () => {},
        };
      },
    }),
    screenshot: async () => screenshotBytes,
  };
  try {
    const playwright = await attachPlaywright(
      {
        playwrightCli: "C:\\tools\\playwright-cli.cmd",
        output,
        sourceRoot: root,
        port: 9229,
      },
      Date.now() + 5000,
      {
        runTool: async (_tool, args) => {
          calls.push(args);
          if (!args.includes("run-code")) return "";
          const payload = args.at(-1);
          assert.match(payload, /^async page =>/u);
          const callback = Function(`"use strict"; return (${payload});`)();
          assert.equal(typeof callback, "function");
          return JSON.stringify(await callback(page));
        },
      },
    );
    assert.deepEqual(await playwright.inspectTarget(), controlled);
    const screenshot = await playwright.screenshot("command-contract.png");
    assert.deepEqual(readFileSync(screenshot), screenshotBytes);
    await playwright.close();
    const runCodeCalls = calls.filter((args) => args.includes("run-code"));
    assert.equal(runCodeCalls.length, 2);
    assert.match(runCodeCalls[0].at(-1), /newCDPSession\(page\)/u);
    assert.match(runCodeCalls[0].at(-1), /Target\.getTargetInfo/u);
    assert.match(runCodeCalls[1].at(-1), /page\.screenshot\(\)/u);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }

  const admitted = {
    ...controlled,
    webSocketDebuggerUrl: "ws://127.0.0.1:9229/devtools/page/page-1",
  };
  assert.equal(
    revalidateControlledCdpTarget(
      admitted,
      [admitted],
      controlled,
      9229,
      "http://127.0.0.1:1420/",
    ).id,
    "page-1",
  );
  assert.equal(
    revalidateControlledCdpTarget(
      admitted,
      [admitted],
      { ...controlled, url: "http://127.0.0.1:1420" },
      9229,
      "http://127.0.0.1:1420/",
    ).url,
    "http://127.0.0.1:1420/",
  );
  for (const mismatch of [
    { ...controlled, id: "page-2" },
    { ...controlled, title: "Other" },
    { ...controlled, url: "http://127.0.0.1:1421/" },
  ]) {
    assert.throws(
      () =>
        revalidateControlledCdpTarget(
          admitted,
          [admitted],
          mismatch,
          9229,
          "http://127.0.0.1:1420/",
        ),
      /journey-cdp-controlled-target-mismatch/,
    );
  }
  const replacement = {
    ...admitted,
    id: "page-2",
    webSocketDebuggerUrl: "ws://127.0.0.1:9229/devtools/page/page-2",
  };
  assert.throws(
    () =>
      revalidateControlledCdpTarget(
        admitted,
        [replacement],
        { ...controlled, id: "page-2" },
        9229,
        "http://127.0.0.1:1420/",
      ),
    /journey-cdp-target-churn/,
  );
});

test("native IDE evidence is limited to a plain local-evidence descendant", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-ide-root-"));
  try {
    const evidence = path.join(root, "local-evidence", "native");
    mkdirSync(evidence, { recursive: true });
    assert.doesNotThrow(() => assertEvidenceRoot(root, evidence));
    assert.throws(
      () => assertEvidenceRoot(root, path.join(root, "outside")),
      /journey-evidence-root-invalid/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("native IDE executable hashing streams a real-size Sage binary", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-executable-"));
  try {
    const executable = path.join(root, "sage.exe");
    const bytes = 30 * 1024 * 1024;
    writeFileSync(executable, "");
    truncateSync(executable, bytes);
    const expected = createHash("sha256");
    const block = Buffer.alloc(1024 * 1024);
    for (let index = 0; index < 30; index += 1) expected.update(block);
    assert.deepEqual(hashExecutable(executable), {
      bytes,
      sha256: expected.digest("hex"),
    });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("executable binding rejects in-flight replacement, mutation, and prelaunch mismatch", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-executable-identity-"));
  try {
    for (const mode of ["mutation", "replacement"]) {
      const executable = path.join(root, `${mode}.exe`);
      writeFileSync(executable, Buffer.alloc(2 * 1024 * 1024, 1));
      let changed = false;
      assert.throws(
        () =>
          hashExecutable(executable, {
            afterRead: () => {
              if (changed) return;
              changed = true;
              if (mode === "mutation") writeFileSync(executable, "mutated");
              else {
                const replacement = path.join(root, "replacement.tmp");
                const displaced = path.join(root, "displaced.tmp");
                writeFileSync(replacement, "replacement");
                renameSync(executable, displaced);
                renameSync(replacement, executable);
              }
            },
          }),
        /journey-executable-identity-mutated/,
      );
    }
    const stable = path.join(root, "stable.exe");
    writeFileSync(stable, "stable executable");
    const binding = hashExecutable(stable);
    assert.throws(
      () =>
        validateExecutableBinding(stable, {
          ...binding,
          sha256: "f".repeat(64),
        }),
      /journey-executable-binding-mismatch/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("bounded Playwright helper settles overflow and timeout when child never closes", async () => {
  for (const mode of ["overflow", "timeout"]) {
    const child = new EventEmitter();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.pid = 42;
    const cleanupCalls = [];
    const promise = runBoundedTool(
      "tool",
      [],
      Date.now() + (mode === "timeout" ? 5 : 5000),
      process.cwd(),
      {
        spawn: () => child,
        captureIdentity: (pid) => ({
          processId: pid,
          creationDate: "exact",
          descendants: [43],
        }),
        cleanupIdentity: (identity) => {
          cleanupCalls.push(identity);
          return { identityLost: false, remaining: [] };
        },
      },
    );
    if (mode === "overflow") child.stdout.write(Buffer.alloc(64 * 1024 + 1));
    await assert.rejects(
      promise,
      new RegExp(
        `playwright-cli-${mode === "overflow" ? "output-oversize" : "timeout"}`,
      ),
    );
    assert.deepEqual(cleanupCalls, [
      { processId: 42, creationDate: "exact", descendants: [43] },
    ]);
  }
});

test("bounded Playwright helper fails and quarantines when descendant cleanup is unproven", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-tool-quarantine-"));
  try {
    const child = new EventEmitter();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.pid = 52;
    const promise = runBoundedTool("tool", [], Date.now() + 5000, root, {
      spawn: () => child,
      captureIdentity: () => ({
        processId: 52,
        creationDate: "exact",
        remembered: [{ processId: 53 }],
      }),
      cleanupIdentity: () => ({ identityLost: false, remaining: [53] }),
    });
    child.stdout.write(Buffer.alloc(64 * 1024 + 1));
    await assert.rejects(promise, /playwright-cli-cleanup-unproven/);
    assert.equal(
      readdirSync(root).filter((name) =>
        name.startsWith("tool-cleanup-quarantine-"),
      ).length,
      1,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows ProcessIdentity bridge cleans an authenticated disposable root and descendant to zero", async () => {
  const powershell = path.join(
    process.env.SystemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const source = `const {spawn}=require('node:child_process');spawn(${JSON.stringify(powershell)},['-NoProfile','-Command','Start-Sleep -Seconds 30']);setTimeout(()=>{},30000);`;
  const owned = spawn(process.execPath, ["-e", source], {
    windowsHide: true,
    stdio: "ignore",
  });
  try {
    await new Promise((resolve) => setTimeout(resolve, 400));
    const identity = processIdentityHelper("capture", owned.pid);
    assert.ok(identity.remembered.length >= 2);
    const cleanup = processIdentityHelper("cleanup", identity);
    assert.equal(cleanup.identityLost, false);
    assert.deepEqual(cleanup.remaining, []);
  } finally {
    if (!owned.killed) owned.kill();
  }
});

test("Windows PowerShell 5.1 normal wrapper accepts canonical SourceRoot and rejects relative or traversal before launch", () => {
  const powershell = path.join(
    process.env.SystemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const wrapper = path.resolve(
    "scripts",
    "run-source-functional-native-ide-journey.ps1",
  );
  const sourceRoot = path.resolve(".");
  const evidence = path.join(
    sourceRoot,
    "local-evidence",
    `path-validation-${process.pid}-${Date.now()}`,
  );
  const processSnapshot = () => {
    const result = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "@(Get-Process | Where-Object { $_.ProcessName -match '^(sage|playwright)' } | ForEach-Object { \"$($_.ProcessName):$($_.Id)\" } | Sort-Object) -join ','",
      ],
      { encoding: "utf8", windowsHide: true, timeout: 5000 },
    );
    assert.equal(result.status, 0, result.stderr);
    return result.stdout.trim();
  };
  const invoke = (candidate) =>
    spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-File",
        wrapper,
        "-SageExecutable",
        wrapper,
        "-SourceRoot",
        candidate,
        "-EvidenceRoot",
        evidence,
      ],
      {
        encoding: "utf8",
        env: { ...process.env, SAGE_PLAYWRIGHT_CLI: "" },
        windowsHide: true,
        timeout: 5000,
      },
    );
  const before = processSnapshot();
  const accepted = invoke(sourceRoot);
  assert.notEqual(accepted.status, 0);
  assert.match(
    `${accepted.stdout}\n${accepted.stderr}`,
    /SAGE_PLAYWRIGHT_CLI must be an absolute canonical path/,
  );
  for (const rejected of [
    "relative\\source",
    `${sourceRoot}\\safe\\..\\escape`,
    ".\\local-source",
  ]) {
    const result = invoke(rejected);
    assert.notEqual(result.status, 0);
    assert.match(
      `${result.stdout}\n${result.stderr}`,
      /SourceRoot must be an absolute canonical path/,
    );
  }
  assert.equal(processSnapshot(), before);
  assert.equal(existsSync(evidence), false);
});

test("Windows PowerShell Invoke-Git preserves clean empty stdout, dirty status, and command failure", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-invoke-git-"));
  const powershell = path.join(
    process.env.SystemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const wrapper = path.resolve(
    "scripts",
    "run-source-functional-native-ide-journey.ps1",
  );
  try {
    assert.equal(
      spawnSync("git", ["init", "--quiet", root], { windowsHide: true }).status,
      0,
    );
    const quote = (value) => `'${value.replaceAll("'", "''")}'`;
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @('Invoke-GitAt','Invoke-Git')){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};$resolvedSource=${quote(root)};$clean=Invoke-Git @('status','--porcelain=v1','--untracked-files=all');Set-Content -LiteralPath (Join-Path $resolvedSource 'dirty.txt') -Value 'dirty';$dirty=Invoke-Git @('status','--porcelain=v1','--untracked-files=all');$failed=$false;try{[void](Invoke-Git @('not-a-real-command'))}catch{$failed=$true};[ordered]@{clean=$clean;dirty=$dirty;failed=$failed}|ConvertTo-Json -Compress`;
    const encoded = Buffer.from(source, "utf16le").toString("base64");
    const result = spawnSync(
      powershell,
      ["-NoProfile", "-NonInteractive", "-EncodedCommand", encoded],
      { encoding: "utf8", windowsHide: true, timeout: 10000 },
    );
    assert.equal(result.status, 0, result.stderr);
    const observed = JSON.parse(result.stdout.trim());
    assert.equal(observed.clean, "");
    assert.match(observed.dirty, /^\?\? dirty\.txt$/u);
    assert.equal(observed.failed, true);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows PowerShell fixture creation resolves the exact quoted tree revision", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-fixture-git-"));
  const powershell = path.join(
    process.env.SystemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const wrapper = path.resolve(
    "scripts",
    "run-source-functional-native-ide-journey.ps1",
  );
  try {
    const quote = (value) => `'${value.replaceAll("'", "''")}'`;
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @('Invoke-GitAt','Initialize-JourneyFixture')){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};$fixture=${quote(root)};$identity=Initialize-JourneyFixture -Fixture $fixture;$actualCommit=(& git -C $fixture rev-parse 'HEAD').Trim();$actualTree=(& git -C $fixture rev-parse 'HEAD^{tree}').Trim();[ordered]@{commit=$identity.commit;tree=$identity.tree;actualCommit=$actualCommit;actualTree=$actualTree;journeyExists=(Test-Path -LiteralPath (Join-Path $fixture 'journey.ts') -PathType Leaf)}|ConvertTo-Json -Compress`;
    const encoded = Buffer.from(source, "utf16le").toString("base64");
    const result = spawnSync(
      powershell,
      ["-NoProfile", "-NonInteractive", "-EncodedCommand", encoded],
      { encoding: "utf8", windowsHide: true, timeout: 10000 },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    const observed = JSON.parse(result.stdout.trim());
    assert.match(observed.commit, /^[a-f0-9]{40}$/u);
    assert.match(observed.tree, /^[a-f0-9]{40}$/u);
    assert.equal(observed.commit, observed.actualCommit);
    assert.equal(observed.tree, observed.actualTree);
    assert.equal(observed.journeyExists, true);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows disposable profile helper creates the marker admitted by launch_profile", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-native-profile-"));
  const powershell = path.join(
    process.env.SystemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const modulePath = path.resolve(
    "scripts",
    "footprint",
    "DisposableProfile.psm1",
  );
  try {
    const quote = (value) => `'${value.replaceAll("'", "''")}'`;
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;Import-Module ${quote(modulePath)} -Force;$profile=New-C0BDisposableProfile ${quote(root)} 'native-ide-profile';$entries=@(Get-ChildItem -LiteralPath $profile.root -Force);$marker=$entries|Select-Object -First 1;$bytes=[IO.File]::ReadAllBytes($marker.FullName);[ordered]@{profileId=$profile.profileId;identitySha256=$profile.identitySha256;entryCount=$entries.Count;markerName=$marker.Name;markerText=[Text.Encoding]::ASCII.GetString($bytes);markerReparse=[bool]($marker.Attributes -band [IO.FileAttributes]::ReparsePoint);profileReparse=[bool]((Get-Item -LiteralPath $profile.root -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)}|ConvertTo-Json -Compress`;
    const encoded = Buffer.from(source, "utf16le").toString("base64");
    const result = spawnSync(
      powershell,
      ["-NoProfile", "-NonInteractive", "-EncodedCommand", encoded],
      { encoding: "utf8", windowsHide: true, timeout: 10000 },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    const observed = JSON.parse(result.stdout.trim());
    assert.equal(observed.profileId, "native-ide-profile");
    assert.match(observed.identitySha256, /^[a-f0-9]{64}$/u);
    assert.equal(observed.entryCount, 1);
    assert.equal(observed.markerName, ".sage-disposable-profile-v1");
    assert.equal(observed.markerText, "sage-disposable-profile-v1\n");
    assert.equal(observed.markerReparse, false);
    assert.equal(observed.profileReparse, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows wrapper resolves the repository dev port and rejects Tauri/Vite mismatch", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-port-contract-"));
  const powershell = path.join(
    process.env.SystemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const wrapper = path.resolve(
    "scripts",
    "run-source-functional-native-ide-journey.ps1",
  );
  try {
    mkdirSync(path.join(root, "src-tauri"));
    writeFileSync(
      path.join(root, "src-tauri", "tauri.conf.json"),
      JSON.stringify({ build: { devUrl: "http://127.0.0.1:1420" } }),
    );
    writeFileSync(
      path.join(root, "vite.config.ts"),
      "export default { server: { port: 1421, strictPort: true, } };\n",
    );
    const quote = (value) => `'${value.replaceAll("'", "''")}'`;
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Resolve-FrontendPortContract'},$true);Invoke-Expression $fn.Extent.Text;$actual=Resolve-FrontendPortContract ${quote(path.resolve("."))};$mismatch=$false;try{[void](Resolve-FrontendPortContract ${quote(root)})}catch{$mismatch=$true};[ordered]@{port=$actual.port;origin=$actual.origin;mismatch=$mismatch}|ConvertTo-Json -Compress`;
    const result = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-EncodedCommand",
        Buffer.from(source, "utf16le").toString("base64"),
      ],
      { encoding: "utf8", windowsHide: true, timeout: 10000 },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.deepEqual(JSON.parse(result.stdout.trim()), {
      port: 1420,
      origin: "http://127.0.0.1:1420/",
      mismatch: true,
    });
    assert.match(
      readFileSync(wrapper, "utf8"),
      /"--expected-origin",\s*\$frontendContract\.origin/u,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows pending receipt is promoted only for two terminal-zero cleanups", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-pending-receipt-"));
  const powershell = path.join(
    process.env.SystemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const wrapper = path.resolve(
    "scripts",
    "run-source-functional-native-ide-journey.ps1",
  );
  try {
    const evidence = path.join(root, "local-evidence", "pending-test");
    mkdirSync(evidence, { recursive: true });
    const quote = (value) => `'${value.replaceAll("'", "''")}'`;
    const pending = writePendingJourneyReceipt(
      root,
      evidence,
      "machine-receipt.pending.json",
      { completed: true },
    );
    const final = path.join(evidence, "machine-receipt.json");
    assert.equal(existsSync(final), false);
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Publish-PendingJourneyReceipt'},$true);Invoke-Expression $fn.Extent.Text;$pending=${quote(pending)};$final=${quote(final)};$zero=[ordered]@{identityLost=$false;remainingCount=0};$bad=[ordered]@{identityLost=$true;remainingCount=0};$rejected=$false;try{Publish-PendingJourneyReceipt $pending $final $zero $bad}catch{$rejected=$true};$pendingBefore=Test-Path -LiteralPath $pending -PathType Leaf;$finalBefore=Test-Path -LiteralPath $final;Publish-PendingJourneyReceipt $pending $final $zero $zero;[ordered]@{rejected=$rejected;pendingBefore=$pendingBefore;finalBefore=$finalBefore;pendingAfter=(Test-Path -LiteralPath $pending);finalAfter=(Test-Path -LiteralPath $final -PathType Leaf)}|ConvertTo-Json -Compress`;
    const result = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-EncodedCommand",
        Buffer.from(source, "utf16le").toString("base64"),
      ],
      { encoding: "utf8", windowsHide: true, timeout: 10000 },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.deepEqual(JSON.parse(result.stdout.trim()), {
      rejected: true,
      pendingBefore: true,
      finalBefore: false,
      pendingAfter: false,
      finalAfter: true,
    });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows launch identity capture retries once then admits the exact process without quarantine", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-launch-capture-"));
  const powershell = path.join(
    process.env.SystemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const wrapper = path.resolve(
    "scripts",
    "run-source-functional-native-ide-journey.ps1",
  );
  const identityModule = path.resolve(
    "scripts",
    "footprint",
    "ProcessIdentity.psm1",
  );
  try {
    const quote = (value) => `'${value.replaceAll("'", "''")}'`;
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;Import-Module ${quote(identityModule)} -Force;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @('New-OwnedRuntime','Write-LaunchIdentityQuarantine','Stop-UnadmittedOwnedRuntime','Write-RuntimeCleanupQuarantine','Capture-OwnedRuntimeOrQuarantine')){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};$state=[ordered]@{attempts=0;elapsed=0;delays=0;cleanups=0};$resolve={param($candidate,$expected)$state.attempts++;if($state.attempts -eq 1){throw 'injected unavailable'};Get-FootprintLaunchIdentity $candidate $expected};$delay={param($milliseconds)$state.delays++;$state.elapsed+=$milliseconds};$elapsed={return [long]$state.elapsed};$cleanup={param($candidate,$expected,$role,$evidence)$state.cleanups++;return [ordered]@{terminalZero=$true}};$process=Start-Process -FilePath ${quote(powershell)} -ArgumentList @('-NoProfile','-Command','Wait-Event -Timeout 30') -WindowStyle Hidden -PassThru;try{$runtime=$null;$captureError=$null;try{$runtime=Capture-OwnedRuntimeOrQuarantine $process ${quote(powershell)} 'sage' ${quote(root)} 500 $resolve $delay $elapsed $cleanup}catch{$captureError=$_.Exception.Message};$processId=$null;$identityPath=$null;$creationDate=$null;if($null -ne $runtime){$processId=$runtime.identity.processId;$identityPath=$runtime.identity.executablePath;$creationDate=$runtime.identity.creationDate};$quarantine=Test-Path -LiteralPath (Join-Path ${quote(root)} 'runtime-launch-quarantine-sage.json');[ordered]@{captureError=$captureError;attempts=$state.attempts;elapsed=$state.elapsed;delays=$state.delays;cleanups=$state.cleanups;quarantine=$quarantine;processId=$processId;identityPath=$identityPath;creationDate=$creationDate}|ConvertTo-Json -Compress}finally{$process.Refresh();if(-not $process.HasExited){$process.Kill()};[void]$process.WaitForExit(5000)}`;
    const result = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-EncodedCommand",
        Buffer.from(source, "utf16le").toString("base64"),
      ],
      { encoding: "utf8", windowsHide: true, timeout: 15000 },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    const observed = JSON.parse(result.stdout.trim());
    assert.equal(observed.captureError, null, JSON.stringify(observed));
    assert.equal(observed.attempts, 2);
    assert.equal(observed.elapsed, 40);
    assert.equal(observed.delays, 1);
    assert.equal(observed.cleanups, 0);
    assert.equal(observed.quarantine, false);
    assert.ok(Number.isInteger(observed.processId));
    assert.equal(observed.identityPath.toLowerCase(), powershell.toLowerCase());
    assert.match(observed.creationDate, /^\d{4}-\d{2}-\d{2}T/u);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows launch identity exhaustion cleans a real tree and leaves an ambient sentinel untouched", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-launch-exhaustion-"));
  const powershell = path.join(
    process.env.SystemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const wrapper = path.resolve(
    "scripts",
    "run-source-functional-native-ide-journey.ps1",
  );
  const identityModule = path.resolve(
    "scripts",
    "footprint",
    "ProcessIdentity.psm1",
  );
  try {
    const fixture = path.join(root, "owned-root.mjs");
    const childPidFile = path.join(root, "owned-child.pid");
    writeFileSync(
      fixture,
      `import {spawn} from "node:child_process";import {writeFileSync} from "node:fs";const [pidFile,powershell]=process.argv.slice(2);const child=spawn(powershell,["-NoProfile","-Command","Start-Sleep -Seconds 30"],{detached:true,windowsHide:true,stdio:"ignore"});writeFileSync(pidFile,String(child.pid));setInterval(()=>{},30000);`,
    );
    const quote = (value) => `'${value.replaceAll("'", "''")}'`;
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;Import-Module ${quote(identityModule)} -Force;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @('New-OwnedRuntime','Write-LaunchIdentityQuarantine','Stop-UnadmittedOwnedRuntime','Write-RuntimeCleanupQuarantine','Capture-OwnedRuntimeOrQuarantine')){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};$state=[ordered]@{attempts=0;now=70;delays=0};$resolve={param($candidate,$expected)$state.attempts++;throw 'injected unavailable'};$delay={param($milliseconds)$state.delays++;$state.now+=$milliseconds};$clock={return [long]$state.now};$ambient=Start-Process -FilePath ${quote(powershell)} -ArgumentList @('-NoProfile','-Command','Wait-Event -Timeout 30') -WindowStyle Hidden -PassThru;$owned=Start-Process -FilePath ${quote(process.execPath)} -ArgumentList @(${quote(fixture)},${quote(childPidFile)},${quote(powershell)}) -WindowStyle Hidden -PassThru;$childPid=$null;try{$ready=[Diagnostics.Stopwatch]::StartNew();while(-not (Test-Path -LiteralPath ${quote(childPidFile)}) -and $ready.ElapsedMilliseconds -lt 5000){Start-Sleep -Milliseconds 25};if(-not (Test-Path -LiteralPath ${quote(childPidFile)})){throw 'owned child did not start'};$childPid=[int](Get-Content -LiteralPath ${quote(childPidFile)} -Raw);[void](New-Item -ItemType Directory -Path (Join-Path ${quote(root)} 'runtime-launch-quarantine-sage.json'));$captureError=$null;try{[void](Capture-OwnedRuntimeOrQuarantine $owned ${quote(process.execPath)} 'sage' ${quote(root)} 85 $resolve $delay $clock)}catch{$captureError=$_.Exception.Message};$owned.Refresh();$ambient.Refresh();[ordered]@{captureError=$captureError;attempts=$state.attempts;now=$state.now;delays=$state.delays;ownedAlive=(-not $owned.HasExited);childAlive=($null -ne (Get-Process -Id $childPid -ErrorAction SilentlyContinue));ambientAlive=(-not $ambient.HasExited);launchQuarantineIsDirectory=(Test-Path -LiteralPath (Join-Path ${quote(root)} 'runtime-launch-quarantine-sage.json') -PathType Container);cleanupQuarantine=(Test-Path -LiteralPath (Join-Path ${quote(root)} 'runtime-cleanup-quarantine-sage.json'))}|ConvertTo-Json -Compress}finally{foreach($pidValue in @($childPid,$owned.Id,$ambient.Id)){if($pidValue){Stop-Process -Id $pidValue -Force -ErrorAction SilentlyContinue}}}`;
    const result = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-EncodedCommand",
        Buffer.from(source, "utf16le").toString("base64"),
      ],
      { encoding: "utf8", windowsHide: true, timeout: 15000 },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    const observed = JSON.parse(result.stdout.trim());
    assert.match(
      observed.captureError,
      /launch quarantine could not be retained/,
    );
    assert.equal(observed.attempts, 1);
    assert.equal(observed.now, 85);
    assert.equal(observed.delays, 1);
    assert.equal(observed.ownedAlive, false);
    assert.equal(observed.childAlive, false);
    assert.equal(observed.ambientAlive, true);
    assert.equal(observed.launchQuarantineIsDirectory, true);
    assert.equal(observed.cleanupQuarantine, false, JSON.stringify(observed));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows ProcessIdentity bridge cleans a remembered child after the root exits", () => {
  const powershell = path.join(
    process.env.SystemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const identityModule = path.resolve(
    "scripts",
    "footprint",
    "ProcessIdentity.psm1",
  );
  const quote = (value) => `'${value.replaceAll("'", "''")}'`;
  const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;Import-Module ${quote(identityModule)} -Force;$root=Start-Process -FilePath ${quote(powershell)} -ArgumentList @('-NoProfile','-NonInteractive','-Command','Start-Sleep -Milliseconds 700') -WindowStyle Hidden -PassThru;$child=Start-Process -FilePath ${quote(powershell)} -ArgumentList @('-NoProfile','-NonInteractive','-Command','Wait-Event -Timeout 30') -WindowStyle Hidden -PassThru;$ambient=Start-Process -FilePath ${quote(powershell)} -ArgumentList @('-NoProfile','-NonInteractive','-Command','Wait-Event -Timeout 30') -WindowStyle Hidden -PassThru;try{$known=New-FootprintIdentityStore;$rootIdentity=Get-FootprintLaunchIdentity $root ${quote(powershell)};$childIdentity=Get-FootprintLaunchIdentity $child ${quote(powershell)};[void](Add-FootprintRememberedIdentity $known $rootIdentity);[void](Add-FootprintRememberedIdentity $known $childIdentity);if(-not $root.WaitForExit(5000)){throw 'remembered root did not exit naturally'};$child.Refresh();$ambient.Refresh();$childAliveBeforeCleanup=-not $child.HasExited;$ambientAliveBeforeCleanup=-not $ambient.HasExited;$cleanup=Stop-FootprintLaunchedProcessObject $root ${quote(powershell)} $known $null 0 5000;$child.Refresh();$ambient.Refresh();[ordered]@{rootExited=$root.HasExited;childAliveBeforeCleanup=$childAliveBeforeCleanup;ambientAliveBeforeCleanup=$ambientAliveBeforeCleanup;childAliveAfterCleanup=(-not $child.HasExited);ambientAliveAfterCleanup=(-not $ambient.HasExited);cleanupExited=$cleanup.exited;treeBoundaryVerified=$cleanup.treeBoundaryVerified}|ConvertTo-Json -Compress}finally{foreach($candidate in @($root,$child,$ambient)){if($null -ne $candidate){try{$candidate.Refresh();if(-not $candidate.HasExited){$candidate.Kill()};[void]$candidate.WaitForExit(5000)}catch{}}}}`;
  const result = spawnSync(
    powershell,
    [
      "-NoProfile",
      "-NonInteractive",
      "-EncodedCommand",
      Buffer.from(source, "utf16le").toString("base64"),
    ],
    { encoding: "utf8", windowsHide: true, timeout: 15000 },
  );
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
  const observed = JSON.parse(result.stdout.trim());
  assert.equal(observed.rootExited, true, JSON.stringify(observed));
  assert.equal(
    observed.childAliveBeforeCleanup,
    true,
    JSON.stringify(observed),
  );
  assert.equal(
    observed.ambientAliveBeforeCleanup,
    true,
    JSON.stringify(observed),
  );
  assert.equal(
    observed.childAliveAfterCleanup,
    false,
    JSON.stringify(observed),
  );
  assert.equal(
    observed.ambientAliveAfterCleanup,
    true,
    JSON.stringify(observed),
  );
  assert.equal(observed.cleanupExited, true, JSON.stringify(observed));
  assert.equal(observed.treeBoundaryVerified, true, JSON.stringify(observed));
});

test("Windows launch identity exits and path mismatches fail immediately without retry delay", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-launch-immediate-"));
  const powershell = path.join(
    process.env.SystemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const wrapper = path.resolve(
    "scripts",
    "run-source-functional-native-ide-journey.ps1",
  );
  const identityModule = path.resolve(
    "scripts",
    "footprint",
    "ProcessIdentity.psm1",
  );
  try {
    const quote = (value) => `'${value.replaceAll("'", "''")}'`;
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;Import-Module ${quote(identityModule)} -Force;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @('New-OwnedRuntime','Write-LaunchIdentityQuarantine','Stop-UnadmittedOwnedRuntime','Write-RuntimeCleanupQuarantine','Capture-OwnedRuntimeOrQuarantine')){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};$state=[ordered]@{resolves=0;delays=0;cleanups=0};$resolve={param($candidate,$expected)$state.resolves++;throw 'must not run'};$delay={param($milliseconds)$state.delays++};$elapsed={return [long]0};$cleanup={param($candidate,$expected,$role,$evidence)$state.cleanups++;$candidate.Refresh();if(-not $candidate.HasExited){$candidate.Kill()};[void]$candidate.WaitForExit(5000);return [ordered]@{terminalZero=$true}};$mismatch=Start-Process -FilePath ${quote(powershell)} -ArgumentList @('-NoProfile','-Command','Wait-Event -Timeout 30') -WindowStyle Hidden -PassThru;try{[void](Capture-OwnedRuntimeOrQuarantine $mismatch ${quote(wrapper)} 'sage' ${quote(root)} 500 $resolve $delay $elapsed $cleanup)}catch{};$mismatchRecord=Get-Content -LiteralPath (Join-Path ${quote(root)} 'runtime-launch-quarantine-sage.json') -Raw|ConvertFrom-Json;Remove-Item -LiteralPath (Join-Path ${quote(root)} 'runtime-launch-quarantine-sage.json');$exited=Start-Process -FilePath ${quote(powershell)} -ArgumentList @('-NoProfile','-Command','exit 0') -WindowStyle Hidden -PassThru;[void]$exited.WaitForExit(5000);try{[void](Capture-OwnedRuntimeOrQuarantine $exited ${quote(powershell)} 'sage' ${quote(root)} 500 $resolve $delay $elapsed $cleanup)}catch{};$exitRecord=Get-Content -LiteralPath (Join-Path ${quote(root)} 'runtime-launch-quarantine-sage.json') -Raw|ConvertFrom-Json;[ordered]@{resolves=$state.resolves;delays=$state.delays;cleanups=$state.cleanups;mismatch=$mismatchRecord.failureCategory;exited=$exitRecord.failureCategory;mismatchAlive=(-not $mismatch.HasExited);exitedAlive=(-not $exited.HasExited)}|ConvertTo-Json -Compress`;
    const result = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-EncodedCommand",
        Buffer.from(source, "utf16le").toString("base64"),
      ],
      { encoding: "utf8", windowsHide: true, timeout: 15000 },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.deepEqual(JSON.parse(result.stdout.trim()), {
      resolves: 0,
      delays: 0,
      cleanups: 2,
      mismatch: "path-mismatch",
      exited: "process-exited",
      mismatchAlive: false,
      exitedAlive: false,
    });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows resolver PID and creation mismatches fail deterministically without retries", () => {
  const root = mkdtempSync(
    path.join(tmpdir(), "sage-launch-resolver-mismatch-"),
  );
  const powershell = path.join(
    process.env.SystemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const wrapper = path.resolve(
    "scripts",
    "run-source-functional-native-ide-journey.ps1",
  );
  const identityModule = path.resolve(
    "scripts",
    "footprint",
    "ProcessIdentity.psm1",
  );
  try {
    const quote = (value) => `'${value.replaceAll("'", "''")}'`;
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;Import-Module ${quote(identityModule)} -Force;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @('New-OwnedRuntime','Write-LaunchIdentityQuarantine','Stop-UnadmittedOwnedRuntime','Write-RuntimeCleanupQuarantine','Capture-OwnedRuntimeOrQuarantine')){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};$state=[ordered]@{resolves=0;now=0;delays=0;cleanups=0};$mode='pid';$resolve={param($candidate,$expected)$state.resolves++;$identity=Get-FootprintLaunchIdentity $candidate $expected;if($mode -eq 'pid'){$identity.processId=[int]$identity.processId+1}else{$identity.creationDate='2000-01-01T00:00:00.000Z'};return $identity};$delay={param($milliseconds)$state.delays++;$state.now+=$milliseconds};$clock={return [long]$state.now};$cleanup={param($candidate,$expected,$role,$evidence)$state.cleanups++;$candidate.Refresh();if(-not $candidate.HasExited){$candidate.Kill()};[void]$candidate.WaitForExit(5000);return [ordered]@{terminalZero=$true}};$categories=@();foreach($modeValue in @('pid','creation')){$mode=$modeValue;$candidate=Start-Process -FilePath ${quote(powershell)} -ArgumentList @('-NoProfile','-Command','Wait-Event -Timeout 30') -WindowStyle Hidden -PassThru;try{try{[void](Capture-OwnedRuntimeOrQuarantine $candidate ${quote(powershell)} 'sage' ${quote(root)} 500 $resolve $delay $clock $cleanup)}catch{};$record=Get-Content -LiteralPath (Join-Path ${quote(root)} 'runtime-launch-quarantine-sage.json') -Raw|ConvertFrom-Json;$categories+=$record.failureCategory;Remove-Item -LiteralPath (Join-Path ${quote(root)} 'runtime-launch-quarantine-sage.json')}finally{$candidate.Refresh();if(-not $candidate.HasExited){$candidate.Kill()};[void]$candidate.WaitForExit(5000)}};[ordered]@{resolves=$state.resolves;delays=$state.delays;cleanups=$state.cleanups;categories=$categories}|ConvertTo-Json -Compress`;
    const result = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-EncodedCommand",
        Buffer.from(source, "utf16le").toString("base64"),
      ],
      { encoding: "utf8", windowsHide: true, timeout: 15000 },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.deepEqual(JSON.parse(result.stdout.trim()), {
      resolves: 2,
      delays: 0,
      cleanups: 2,
      categories: ["resolver-pid-mismatch", "resolver-creation-mismatch"],
    });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows owned Vite fixture becomes ready and reaches terminal-zero cleanup", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-vite-runtime-"));
  const powershell = path.join(
    process.env.SystemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const wrapper = path.resolve(
    "scripts",
    "run-source-functional-native-ide-journey.ps1",
  );
  const identityModule = path.resolve(
    "scripts",
    "footprint",
    "ProcessIdentity.psm1",
  );
  try {
    writeFileSync(
      path.join(root, "package.json"),
      JSON.stringify({ private: true, scripts: { dev: "node server.mjs" } }),
    );
    writeFileSync(
      path.join(root, "server.mjs"),
      `import http from "node:http";const args=process.argv.slice(2);const at=args.indexOf("--port");const port=Number(args[at+1]);const server=http.createServer((_request,response)=>{response.writeHead(200,{"content-type":"text/html"});response.end("ready")});server.listen(port,"127.0.0.1");`,
    );
    const quote = (value) => `'${value.replaceAll("'", "''")}'`;
    const functions = [
      "Test-FullyQualifiedPath",
      "Require-AbsoluteFile",
      "Resolve-FrontendPortContract",
      "Assert-FrontendPortAvailable",
      "New-OwnedRuntime",
      "Write-LaunchIdentityQuarantine",
      "Stop-UnadmittedOwnedRuntime",
      "Capture-OwnedRuntimeOrQuarantine",
      "Update-OwnedRuntimeTree",
      "Get-FrontendListenerIdentity",
      "Test-OwnedRuntimeProcessIdentity",
      "Test-SameProcessIdentity",
      "Test-FrontendHttpReady",
      "Wait-OwnedFrontendReady",
      "Start-OwnedVite",
      "Write-RuntimeCleanupQuarantine",
      "Stop-OwnedRuntimeTree",
    ];
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;Import-Module ${quote(identityModule)} -Force;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @(${functions.map(quote).join(",")})){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};$contract=Resolve-FrontendPortContract ${quote(path.resolve("."))};$runtime=$null;$cleanup=$null;$startupDeadline=[DateTime]::UtcNow.AddSeconds(12);$clock=[Diagnostics.Stopwatch]::StartNew();$monotonic={return [long]$clock.ElapsedMilliseconds}.GetNewClosure();$deadlineMilliseconds=[long](&$monotonic)+12000;try{$runtime=Start-OwnedVite ${quote(root)} $contract ${quote(root)} $deadlineMilliseconds $monotonic;$ready=Wait-OwnedFrontendReady $runtime $contract $startupDeadline;$ownedIds=@($runtime.store.Values|ForEach-Object {[int]$_.processId}|Sort-Object -Unique);$cleanup=Stop-OwnedRuntimeTree 'vite' $runtime ${quote(root)};Start-Sleep -Milliseconds 200;$alive=@($ownedIds|Where-Object {Get-Process -Id $_ -ErrorAction SilentlyContinue}).Count;[ordered]@{port=$contract.port;mode=$ready.mode;frontendReady=$ready.frontendReady;identityCount=$ownedIds.Count;identityLost=$cleanup.identityLost;remainingCount=$cleanup.remainingCount;alive=$alive}|ConvertTo-Json -Compress}finally{if($null -ne $runtime -and $null -eq $cleanup){try{[void](Stop-OwnedRuntimeTree 'vite' $runtime ${quote(root)})}catch{}}}`;
    const encoded = Buffer.from(source, "utf16le").toString("base64");
    const result = spawnSync(
      powershell,
      ["-NoProfile", "-NonInteractive", "-EncodedCommand", encoded],
      { encoding: "utf8", windowsHide: true, timeout: 25000 },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    const observed = JSON.parse(result.stdout.trim());
    assert.equal(observed.port, 1420);
    assert.equal(observed.mode, "tauri-dev-split");
    assert.equal(observed.frontendReady, true);
    assert.ok(observed.identityCount >= 2);
    assert.equal(observed.identityLost, false);
    assert.equal(observed.remainingCount, 0);
    assert.equal(observed.alive, 0);
    assertWindowsPortFree(1420);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows readiness requires one unchanged injected listener identity", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-listener-identity-"));
  const powershell = path.join(
    process.env.SystemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const wrapper = path.resolve(
    "scripts",
    "run-source-functional-native-ide-journey.ps1",
  );
  const identityModule = path.resolve(
    "scripts",
    "footprint",
    "ProcessIdentity.psm1",
  );
  try {
    for (const mode of ["same", "pid", "path", "creation"]) {
      const quote = (value) => `'${value.replaceAll("'", "''")}'`;
      const names = [
        "New-OwnedRuntime",
        "Update-OwnedRuntimeTree",
        "Test-OwnedRuntimeProcessIdentity",
        "Test-SameProcessIdentity",
        "Wait-OwnedFrontendReady",
        "Write-RuntimeCleanupQuarantine",
        "Stop-OwnedRuntimeTree",
      ];
      const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;Import-Module ${quote(identityModule)} -Force;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @(${names.map(quote).join(",")})){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};$process=Start-Process -FilePath ${quote(powershell)} -ArgumentList @('-NoProfile','-Command','Start-Sleep -Seconds 30') -WindowStyle Hidden -PassThru;$identity=Get-FootprintLaunchIdentity $process ${quote(powershell)};$runtime=New-OwnedRuntime $process ${quote(powershell)} $identity;try{$before=$runtime.identity;$after=[ordered]@{processId=$(if('${mode}'-eq'pid'){2147483000}else{$before.processId});executablePath=$(if('${mode}'-eq'path'){${quote(wrapper)}}else{$before.executablePath});creationDate=$(if('${mode}'-eq'creation'){'1900-01-01T00:00:00.000Z'}else{$before.creationDate})};$script:probe=0;$listener={param($ignored)$script:probe++;if($script:probe-eq1){$before}else{$after}};$http={param($ignored,$timeout)$true};$outcome='';$ready=$null;try{$ready=Wait-OwnedFrontendReady $runtime ([ordered]@{port=1420;origin='http://127.0.0.1:1420/'}) ([DateTime]::UtcNow.AddSeconds(5)) $listener $http;$outcome='accepted'}catch{$outcome=$_.Exception.Message};$cleanup=Stop-OwnedRuntimeTree 'vite' $runtime ${quote(root)};[ordered]@{mode='${mode}';outcome=$outcome;readyMode=$(if($null-ne$ready){$ready.mode}else{$null});frontendReady=$(if($null-ne$ready){$ready.frontendReady}else{$false});probeCount=$script:probe;identityLost=$cleanup.identityLost;remainingCount=$cleanup.remainingCount}|ConvertTo-Json -Compress}finally{Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue};exit 0`;
      const result = spawnSync(
        powershell,
        [
          "-NoProfile",
          "-NonInteractive",
          "-EncodedCommand",
          Buffer.from(source, "utf16le").toString("base64"),
        ],
        { encoding: "utf8", windowsHide: true, timeout: 15000 },
      );
      assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
      const observed = JSON.parse(result.stdout.trim());
      assert.equal(observed.mode, mode);
      assert.equal(observed.probeCount, 2);
      assert.equal(observed.identityLost, false);
      assert.equal(observed.remainingCount, 0);
      if (mode === "same") {
        assert.equal(observed.outcome, "accepted");
        assert.equal(observed.readyMode, "tauri-dev-split");
        assert.equal(observed.frontendReady, true);
      } else {
        assert.match(
          observed.outcome,
          /listener identity changed during readiness/,
        );
        assert.equal(observed.readyMode, null);
        assert.equal(observed.frontendReady, false);
      }
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows Vite port preoccupation is rejected before ownership", async () => {
  const powershell = path.join(
    process.env.SystemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const wrapper = path.resolve(
    "scripts",
    "run-source-functional-native-ide-journey.ps1",
  );
  const occupied = spawn(
    process.execPath,
    [
      "-e",
      `require("node:http").createServer((_q,r)=>r.end("ambient")).listen(1420,"127.0.0.1",()=>process.stdout.write("ready\\n"))`,
    ],
    { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] },
  );
  let occupiedIdentity;
  try {
    await waitForText(occupied.stdout, "ready", 5000);
    occupiedIdentity = processIdentityHelper("capture", occupied.pid);
    const quote = (value) => `'${value.replaceAll("'", "''")}'`;
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @('Resolve-FrontendPortContract','Assert-FrontendPortAvailable')){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};$contract=Resolve-FrontendPortContract ${quote(path.resolve("."))};Assert-FrontendPortAvailable $contract`;
    const encoded = Buffer.from(source, "utf16le").toString("base64");
    const result = spawnSync(
      powershell,
      ["-NoProfile", "-NonInteractive", "-EncodedCommand", encoded],
      { encoding: "utf8", windowsHide: true, timeout: 10000 },
    );
    assert.notEqual(result.status, 0);
    assert.match(
      `${result.stdout}\n${result.stderr}`,
      /required frontend port is already occupied/,
    );
  } finally {
    const failures = [];
    try {
      await waitForChild(occupied, occupiedIdentity);
    } catch (error) {
      failures.push(error);
    }
    try {
      assertWindowsPortFree(1420);
    } catch (error) {
      failures.push(error);
    }
    if (failures.length)
      throw new AggregateError(failures, "fixture cleanup failed");
  }
});

test("Windows frontend readiness rejects an ambient listener race outside the owned tree", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-vite-ambient-race-"));
  const powershell = path.join(
    process.env.SystemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const wrapper = path.resolve(
    "scripts",
    "run-source-functional-native-ide-journey.ps1",
  );
  const identityModule = path.resolve(
    "scripts",
    "footprint",
    "ProcessIdentity.psm1",
  );
  const ambient = spawn(
    process.execPath,
    [
      "-e",
      `require("node:http").createServer((_q,r)=>r.end("ambient")).listen(1420,"127.0.0.1",()=>process.stdout.write("ready\\n"))`,
    ],
    { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] },
  );
  let ambientIdentity;
  try {
    await waitForText(ambient.stdout, "ready", 5000);
    ambientIdentity = processIdentityHelper("capture", ambient.pid);
    const quote = (value) => `'${value.replaceAll("'", "''")}'`;
    const names = [
      "Resolve-FrontendPortContract",
      "New-OwnedRuntime",
      "Update-OwnedRuntimeTree",
      "Get-FrontendListenerIdentity",
      "Test-OwnedRuntimeProcessIdentity",
      "Test-SameProcessIdentity",
      "Test-FrontendHttpReady",
      "Wait-OwnedFrontendReady",
      "Write-RuntimeCleanupQuarantine",
      "Stop-OwnedRuntimeTree",
    ];
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;Import-Module ${quote(identityModule)} -Force;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @(${names.map(quote).join(",")})){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};$contract=Resolve-FrontendPortContract ${quote(path.resolve("."))};$process=Start-Process -FilePath ${quote(powershell)} -ArgumentList @('-NoProfile','-Command','Start-Sleep -Seconds 30') -WindowStyle Hidden -PassThru;$identity=Get-FootprintLaunchIdentity $process ${quote(powershell)};$runtime=New-OwnedRuntime $process ${quote(powershell)} $identity;try{$rejected=$false;try{[void](Wait-OwnedFrontendReady $runtime $contract ([DateTime]::UtcNow.AddSeconds(5)))}catch{$rejected=$_.Exception.Message -match 'not owned'};$cleanup=Stop-OwnedRuntimeTree 'vite' $runtime ${quote(root)};[ordered]@{rejected=$rejected;identityLost=$cleanup.identityLost;remainingCount=$cleanup.remainingCount}|ConvertTo-Json -Compress}finally{if($null -ne (Get-Process -Id $process.Id -ErrorAction SilentlyContinue)){Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue}}`;
    const result = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-EncodedCommand",
        Buffer.from(source, "utf16le").toString("base64"),
      ],
      { encoding: "utf8", windowsHide: true, timeout: 15000 },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.deepEqual(JSON.parse(result.stdout.trim()), {
      rejected: true,
      identityLost: false,
      remainingCount: 0,
    });
  } finally {
    const failures = [];
    try {
      await waitForChild(ambient, ambientIdentity);
    } catch (error) {
      failures.push(error);
    }
    try {
      assertWindowsPortFree(1420);
    } catch (error) {
      failures.push(error);
    }
    rmSync(root, { recursive: true, force: true });
    if (failures.length)
      throw new AggregateError(failures, "fixture cleanup failed");
  }
});

test("Windows Vite cleanup quarantines identity loss without killing the mismatched PID", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-vite-identity-loss-"));
  const powershell = path.join(
    process.env.SystemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const wrapper = path.resolve(
    "scripts",
    "run-source-functional-native-ide-journey.ps1",
  );
  const identityModule = path.resolve(
    "scripts",
    "footprint",
    "ProcessIdentity.psm1",
  );
  try {
    const quote = (value) => `'${value.replaceAll("'", "''")}'`;
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;Import-Module ${quote(identityModule)} -Force;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @('Update-OwnedRuntimeTree','Write-RuntimeCleanupQuarantine','Stop-OwnedRuntimeTree')){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};$process=Start-Process -FilePath ${quote(powershell)} -ArgumentList @('-NoProfile','-Command','Start-Sleep -Seconds 30') -WindowStyle Hidden -PassThru;try{$actual=Get-FootprintLaunchIdentity $process ${quote(powershell)};$forged=[ordered]@{processId=$actual.processId;executablePath=$actual.executablePath;creationDate='1900-01-01T00:00:00.000Z'};$runtime=[ordered]@{process=$process;identity=$forged;store=(New-FootprintIdentityStore)};$failed=$false;try{[void](Stop-OwnedRuntimeTree 'vite' $runtime ${quote(root)})}catch{$failed=$true};$alive=$null -ne (Get-Process -Id $process.Id -ErrorAction SilentlyContinue);$record=Get-Content -LiteralPath (Join-Path ${quote(root)} 'runtime-cleanup-quarantine-vite.json') -Raw|ConvertFrom-Json;[ordered]@{failed=$failed;alive=$alive;record=$record}|ConvertTo-Json -Depth 4 -Compress}finally{Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue;[void]$process.WaitForExit(5000)}`;
    const encoded = Buffer.from(source, "utf16le").toString("base64");
    const result = spawnSync(
      powershell,
      ["-NoProfile", "-NonInteractive", "-EncodedCommand", encoded],
      { encoding: "utf8", windowsHide: true, timeout: 15000 },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    const observed = JSON.parse(result.stdout.trim());
    assert.equal(observed.failed, true);
    assert.equal(observed.alive, true);
    assert.deepEqual(observed.record, {
      schemaVersion: 1,
      runtimeRole: "vite",
      category: "cleanup-unproven",
      identityLost: true,
      remainingCount: 0,
    });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("normal wrapper passes clean Git status with a canonical helper fixture and fails safely before evidence or launch", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-clean-preflight-"));
  const powershell = path.join(
    process.env.SystemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const wrapper = path.resolve(
    "scripts",
    "run-source-functional-native-ide-journey.ps1",
  );
  try {
    const playwrightFixture = path.join(root, "playwright-cli-fixture.cmd");
    mkdirSync(path.join(root, "src-tauri"));
    writeFileSync(
      path.join(root, "src-tauri", "tauri.conf.json"),
      JSON.stringify({ build: { devUrl: "http://127.0.0.1:1420" } }),
    );
    writeFileSync(
      path.join(root, "vite.config.ts"),
      "export default {\n  server: { port: 1420, strictPort: true, },\n};\n",
    );
    writeFileSync(playwrightFixture, "@echo off\r\nexit /b 0\r\n");
    for (const args of [
      ["init", "--quiet"],
      ["config", "user.name", "Sage preflight"],
      ["config", "user.email", "sage-preflight@localhost"],
      ["add", "--all"],
      ["commit", "--quiet", "-m", "fixture"],
    ]) {
      assert.equal(
        spawnSync("git", ["-C", root, ...args], { windowsHide: true }).status,
        0,
      );
    }
    const before = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-Command",
        "@(Get-Process|? ProcessName -match '^(sage|playwright)'|%{\"$($_.ProcessName):$($_.Id)\"}|sort)-join ','",
      ],
      { encoding: "utf8", windowsHide: true },
    ).stdout.trim();
    const result = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-File",
        wrapper,
        "-SageExecutable",
        wrapper,
        "-SourceRoot",
        root,
        "-EvidenceRoot",
        "relative-evidence",
      ],
      {
        encoding: "utf8",
        windowsHide: true,
        timeout: 10000,
        env: { ...process.env, SAGE_PLAYWRIGHT_CLI: playwrightFixture },
      },
    );
    assert.notEqual(result.status, 0);
    assert.match(
      `${result.stdout}\n${result.stderr}`,
      /EvidenceRoot must be an absolute canonical path/,
    );
    const after = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-Command",
        "@(Get-Process|? ProcessName -match '^(sage|playwright)'|%{\"$($_.ProcessName):$($_.Id)\"}|sort)-join ','",
      ],
      { encoding: "utf8", windowsHide: true },
    ).stdout.trim();
    assert.equal(after, before);
    assert.equal(existsSync(path.resolve("relative-evidence")), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("capture failure and identity-loss responses fail with bounded quarantine records", async () => {
  for (const branch of ["capture", "identity-loss"]) {
    const root = mkdtempSync(path.join(tmpdir(), "sage-tool-quarantine-"));
    try {
      const child = new EventEmitter();
      child.stdout = new PassThrough();
      child.stderr = new PassThrough();
      child.pid = 62;
      const promise = runBoundedTool("tool", [], Date.now() + 10, root, {
        spawn: () => child,
        captureIdentity:
          branch === "capture"
            ? () => {
                throw new Error("capture");
              }
            : () => ({ processId: 62, creationDate: "exact" }),
        cleanupIdentity: () => ({ identityLost: true, remaining: [] }),
      });
      await assert.rejects(promise, /playwright-cli-cleanup-unproven/);
      const names = readdirSync(root).filter((name) =>
        name.startsWith("tool-cleanup-quarantine-"),
      );
      assert.equal(names.length, 1);
      const record = JSON.parse(
        readFileSync(path.join(root, names[0]), "utf8"),
      );
      assert.equal(record.observedPid, 62);
      assert.equal(record.commandRole, "playwright-helper");
      assert.equal("executablePath" in record, false);
      assert.equal(
        record.category,
        branch === "capture" ? "identity-capture-failed" : "timeout",
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

function observed() {
  return {
    save: {
      editorPath: "journey.ts",
      saveDisabled: true,
      contentSha256: "1".repeat(64),
      diskSha256: "2".repeat(64),
    },
    diagnostic: {
      resource: "journey.ts",
      count: 1,
      messagesSha256: "3".repeat(64),
    },
    diff: {
      resource: "journey.ts",
      lineCount: 1,
      contentSha256: "4".repeat(64),
    },
  };
}

function runtimeReceipt() {
  return { mode: "tauri-dev-split", frontendReady: true };
}

function waitForText(stream, expected, timeoutMs) {
  return new Promise((resolve, reject) => {
    let text = "";
    const timer = setTimeout(() => {
      stream.removeListener("data", onData);
      reject(new Error(`timed out waiting for ${expected}`));
    }, timeoutMs);
    const onData = (chunk) => {
      text += chunk.toString("utf8");
      if (!text.includes(expected)) return;
      clearTimeout(timer);
      stream.removeListener("data", onData);
      resolve();
    };
    stream.on("data", onData);
  });
}

async function waitForChild(child, identity) {
  if (child.exitCode !== null) return Promise.resolve();
  child.kill();
  if (await waitForChildClose(child, 2000)) return;
  if (!identity)
    throw new Error(`child ${child.pid} did not exit; identity unavailable`);
  const cleanup = processIdentityHelper("cleanup", identity);
  if (cleanup.identityLost || cleanup.remaining.length !== 0)
    throw new Error(
      `child cleanup unproven for ${identity.processId}/${identity.creationDate}`,
    );
  if (!(await waitForChildClose(child, 2000)))
    throw new Error(
      `child exit unobserved for ${identity.processId}/${identity.creationDate}`,
    );
}

function waitForChildClose(child, timeoutMs) {
  if (child.exitCode !== null) return Promise.resolve(true);
  return Promise.race([
    new Promise((resolve) => child.once("close", () => resolve(true))),
    new Promise((resolve) => setTimeout(() => resolve(false), timeoutMs)),
  ]);
}

function assertWindowsPortFree(port) {
  const powershell = path.join(
    process.env.SystemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
  );
  const result = spawnSync(
    powershell,
    [
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      `$listeners=@(Get-NetTCPConnection -LocalPort ${port} -State Listen -ErrorAction SilentlyContinue);if($listeners.Count){exit 9}`,
    ],
    { encoding: "utf8", windowsHide: true, timeout: 5000 },
  );
  assert.equal(result.status, 0, `port ${port} retained a listener`);
}
