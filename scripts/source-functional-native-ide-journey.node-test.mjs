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
  symlinkSync,
  truncateSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { createServer } from "node:net";
import test from "node:test";
import { PassThrough } from "node:stream";

import {
  admitCdpTarget,
  assertEvidenceRoot,
  attachCdp,
  attachPlaywright,
  JOURNEY_STEPS,
  createJourneyReceipt,
  decodeBoundedScreenshot,
  hashExecutable,
  parseJourneyArguments,
  performUiAction,
  performUiMilestone,
  processIdentityHelper,
  revalidateControlledCdpTarget,
  resolvePlaywrightTransport,
  runBoundedTool,
  runPlaywrightTool,
  UI_ACTIONS,
  UI_PROBES,
  validateExecutableBinding,
  validatePlainCanonicalExecutable,
  validatePlaywrightTransport,
  validateCdpTarget,
  verificationFor,
  writePendingJourneyReceipt,
} from "./source-functional-native-ide-journey.mjs";
import {
  HELPER_DIAGNOSTIC_CHARS_MAX,
  processIdentityHelperOutput,
} from "./source-functional-native-ide-bounded-tool.mjs";
import { revalidateFreshProfile } from "./source-functional-acp04-evidence.mjs";
import { parseAcp04ProbeArguments } from "./source-functional-acp04-live-contract.mjs";
import { parseAv01PreviewArguments } from "./source-functional-av01-preview-contract.mjs";

function evaluateBrowserExpression(expression, globals) {
  const names = Object.keys(globals);
  return Function(
    ...names,
    `"use strict";return (${expression});`,
  )(...names.map((name) => globals[name]));
}

function uiAction(label) {
  return UI_ACTIONS.find((current) => current.label === label);
}

test("Windows journey fixture is BOM-free UTF-8 with the searched sentinel at column 14", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-native-ide-fixture-"));
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
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @('Invoke-GitAt','Initialize-JourneyFixture')){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};Initialize-JourneyFixture ${quote(root)}|ConvertTo-Json -Compress`;
    const result = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-EncodedCommand",
        Buffer.from(source, "utf16le").toString("base64"),
      ],
      { encoding: "utf8", windowsHide: true, timeout: 15_000 },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);

    const expected = Buffer.from(
      'export const SAGE_JOURNEY_SENTINEL: number = "diagnostic";',
      "utf8",
    );
    const bytes = readFileSync(path.join(root, "journey.ts"));
    assert.deepEqual(bytes, expected);
    assert.equal(
      bytes.toString("utf8").indexOf("SAGE_JOURNEY_SENTINEL") + 1,
      14,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("PowerShell 5 ACP-04 profile identity is BOM-free and revalidates", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-acp04-profile-identity-"));
  const profile = path.join(root, "acp04-live-profile");
  const artifact = path.join(root, "acp04-profile-identity.json");
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
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Write-Acp04ProfileIdentity'},$true);Invoke-Expression $fn.Extent.Text;[void](New-Item -ItemType Directory -Path ${quote(profile)});[IO.File]::WriteAllText((Join-Path ${quote(profile)} '.sage-disposable-profile-v1'),"sage-disposable-profile-v1\`n",[Text.UTF8Encoding]::new($false));$identity=[Security.Cryptography.SHA256]::Create();try{$digest=([BitConverter]::ToString($identity.ComputeHash([Text.Encoding]::UTF8.GetBytes(${quote(profile.toLowerCase())}))).Replace('-','').ToLowerInvariant())}finally{$identity.Dispose()};Write-Acp04ProfileIdentity ${quote(artifact)} ${quote(profile)} $digest`;
    const result = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-EncodedCommand",
        Buffer.from(source, "utf16le").toString("base64"),
      ],
      { encoding: "utf8", windowsHide: true, timeout: 15_000 },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);

    const bytes = readFileSync(artifact);
    assert.equal(bytes[0], 0x7b);
    assert.notDeepEqual([...bytes.subarray(0, 3)], [0xef, 0xbb, 0xbf]);
    assert.doesNotThrow(() => JSON.parse(bytes.toString("utf8")));
    assert.deepEqual(
      revalidateFreshProfile(
        { path: profile, identityArtifact: artifact },
        root,
      ),
      {
        path: profile,
        identityArtifact: {
          name: "acp04-profile-identity.json",
          bytes: bytes.length,
          sha256: createHash("sha256").update(bytes).digest("hex"),
        },
        identitySha256: createHash("sha256")
          .update(profile.toLowerCase())
          .digest("hex"),
      },
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows startup incomplete-identity diagnostics retain only safe attributed facts", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-startup-identity-"));
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
    const names = [
      "Assert-PlainContainedDirectory",
      "Get-CleanupRootTuple",
      "Test-StartupIncompleteIdentityFailure",
      "Get-StartupIncompleteIdentityProcessId",
      "Test-StartupIncompleteIdentityFieldPresent",
      "Add-StartupIncompleteIdentityFact",
      "Get-ValidatedStartupIncompleteIdentityFact",
      "Get-StartupIncompleteIdentityDiagnosticPath",
      "Write-StartupIncompleteIdentityDiagnostic",
      "Update-OwnedRuntimeTree",
      "Get-FrontendListenerIdentity",
    ];
    const source = [
      "$ErrorActionPreference = 'Stop'",
      "Set-StrictMode -Version Latest",
      "$tokens = $null; $errors = $null",
      `$ast = [Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)}, [ref]$tokens, [ref]$errors)`,
      `foreach ($name in @(${names.map(quote).join(", ")})) {`,
      "  $fn = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)",
      '  if ($null -eq $fn) { throw "missing $name" }',
      "  Invoke-Expression $fn.Extent.Text",
      "}",
      "$update = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Update-OwnedRuntimeTree' }, $true).Extent.Text",
      "$listener = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-FrontendListenerIdentity' }, $true).Extent.Text",
      "$message = 'A remembered process lacks executable or creation identity.'",
      "$row = [pscustomobject]@{ ProcessId = 101; ParentProcessId = 100; ExecutablePath = ''; CreationDate = $null; CommandLine = 'hostile-command'; Environment = 'hostile-environment' }",
      "function New-Incomplete([object]$target) { return [Management.Automation.ErrorRecord]::new([Management.Automation.RuntimeException]::new($message), 'incomplete', [Management.Automation.ErrorCategory]::InvalidData, $target) }",
      "$facts = @()",
      "foreach ($stage in @('update-root', 'update-descendant', 'frontend-listener')) {",
      "  $failure = New-Incomplete $row; $beforeMessage = $failure.Exception.Message; $beforeType = $failure.Exception.GetType().FullName",
      "  $failure.Exception.Data['hostile-extra'] = 'hostile-extra'",
      "  $isRoot = if ($stage -eq 'update-root') { $true } elseif ($stage -eq 'update-descendant') { $false } else { $null }",
      "  $attached = Add-StartupIncompleteIdentityFact $failure $stage $row $isRoot",
      "  $facts += [ordered]@{ stage = $stage; attached = $attached; message = $failure.Exception.Message; type = $failure.Exception.GetType().FullName; beforeMessage = $beforeMessage; beforeType = $beforeType; fact = Get-ValidatedStartupIncompleteIdentityFact $failure }",
      "}",
      "$diagnosticFailure = New-Incomplete $row; [void](Add-StartupIncompleteIdentityFact $diagnosticFailure 'update-root' $row $true); $diagnosticFailure.Exception.Data['hostile-extra'] = 'hostile-extra'",
      `$viteRuntime = [ordered]@{ identity = [ordered]@{ processId = 101; executablePath = ${quote(powershell)}; creationDate = '2026-09-08T00:00:00.000Z' } }`,
      `$written = Write-StartupIncompleteIdentityDiagnostic ${quote(root)} $diagnosticFailure $viteRuntime`,
      `$diagnosticPath = Join-Path ${quote(root)} 'startup-incomplete-identity.json'; $diagnosticText = Get-Content -LiteralPath $diagnosticPath -Raw; $diagnostic = $diagnosticText | ConvertFrom-Json`,
      "$noOverwrite = $false; try { [void](Write-StartupIncompleteIdentityDiagnostic " +
        quote(root) +
        " $diagnosticFailure $viteRuntime) } catch { $noOverwrite = $true }",
      "$spoofFailure = New-Incomplete $row; [void](Add-StartupIncompleteIdentityFact $spoofFailure 'update-root' $row $true); $spoofFailure.Exception.Data['SageStartupIncompleteIdentity']['spoof'] = 'discard'; $spoofRejected = $null -eq (Get-ValidatedStartupIncompleteIdentityFact $spoofFailure)",
      "$complete = [Management.Automation.ErrorRecord]::new([Management.Automation.RuntimeException]::new('complete row'), 'complete', [Management.Automation.ErrorCategory]::InvalidData, $row); $completeAttached = Add-StartupIncompleteIdentityFact $complete 'update-root' $row $true; $completeDiagnostic = Write-StartupIncompleteIdentityDiagnostic " +
        quote(root) +
        " $complete $viteRuntime",
      "$writeFailure = New-Incomplete $row; [void](Add-StartupIncompleteIdentityFact $writeFailure 'frontend-listener' $row $null); $writeBeforeMessage = $writeFailure.Exception.Message; $writeBeforeType = $writeFailure.Exception.GetType().FullName; $writeRejected = $false; try { [void](Write-StartupIncompleteIdentityDiagnostic (Join-Path " +
        quote(root) +
        " 'missing') $writeFailure $null) } catch { $writeRejected = $true }",
      `$oversizedRoot = Join-Path ${quote(root)} 'oversized'; [void](New-Item -ItemType Directory -Path $oversizedRoot)`,
      "$oversizedFailure = New-Incomplete $row; [void](Add-StartupIncompleteIdentityFact $oversizedFailure 'frontend-listener' $row $null); $originalRootTuple = ${function:Get-CleanupRootTuple}; $oversized = $false; try { function Get-CleanupRootTuple { param($Identity) return [ordered]@{ processId = 101; executablePath = ('x' * 5000); creationDate = '2026-09-08T00:00:00.000Z' } }; [void](Write-StartupIncompleteIdentityDiagnostic $oversizedRoot $oversizedFailure $viteRuntime) } catch { $oversized = $true } finally { Set-Item -Path Function:\\Get-CleanupRootTuple -Value $originalRootTuple }",
      "$oversizedExists = Test-Path -LiteralPath (Join-Path $oversizedRoot 'startup-incomplete-identity.json')",
      "[ordered]@{ facts = $facts; written = $written; diagnostic = $diagnostic; diagnosticBytes = ([Text.Encoding]::UTF8.GetByteCount($diagnosticText)); diagnosticLeakedHostile = $diagnosticText.Contains('hostile'); noOverwrite = $noOverwrite; spoofRejected = $spoofRejected; completeAttached = $completeAttached; completeDiagnostic = $completeDiagnostic; writeRejected = $writeRejected; writeMessage = $writeFailure.Exception.Message; writeType = $writeFailure.Exception.GetType().FullName; writeBeforeMessage = $writeBeforeMessage; writeBeforeType = $writeBeforeType; oversized = $oversized; oversizedExists = $oversizedExists; updateStages = $update.Contains('update-root') -and $update.Contains('update-descendant') -and $update.Contains('$isRoot'); listenerStage = $listener.Contains('frontend-listener') } | ConvertTo-Json -Depth 6 -Compress",
    ].join("\n");
    const result = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-EncodedCommand",
        Buffer.from(source, "utf16le").toString("base64"),
      ],
      { encoding: "utf8", windowsHide: true, timeout: 15_000 },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    const observed = JSON.parse(result.stdout.trim());
    assert.equal(observed.updateStages, true);
    assert.equal(observed.listenerStage, true);
    for (const [index, stage] of [
      "update-root",
      "update-descendant",
      "frontend-listener",
    ].entries()) {
      const entry = observed.facts[index];
      assert.equal(entry.stage, stage);
      assert.equal(entry.attached, true);
      assert.equal(entry.message, entry.beforeMessage);
      assert.equal(entry.type, entry.beforeType);
      assert.deepEqual(entry.fact, {
        stage,
        processId: 101,
        parentProcessId: 100,
        isRoot:
          stage === "update-root"
            ? true
            : stage === "update-descendant"
              ? false
              : null,
        executablePathPresent: false,
        creationDatePresent: false,
      });
    }
    assert.equal(observed.written, true);
    assert.equal(observed.diagnosticBytes <= 4096, true);
    assert.equal(observed.diagnosticLeakedHostile, false);
    assert.equal(observed.diagnostic.rootTupleAvailable, true);
    assert.equal(observed.diagnostic.fact.isRoot, true);
    assert.equal(observed.noOverwrite, true);
    assert.equal(observed.spoofRejected, true);
    assert.equal(observed.completeAttached, false);
    assert.equal(observed.completeDiagnostic, false);
    assert.equal(observed.writeRejected, true);
    assert.equal(observed.writeMessage, observed.writeBeforeMessage);
    assert.equal(observed.writeType, observed.writeBeforeType);
    assert.equal(observed.oversized, true);
    assert.equal(observed.oversizedExists, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows startup diagnostics exercise wrapper seams and preserve failures", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-startup-seams-"));
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
    const names = [
      "Test-StartupIncompleteIdentityFailure",
      "Get-StartupIncompleteIdentityProcessId",
      "Test-StartupIncompleteIdentityFieldPresent",
      "Add-StartupIncompleteIdentityFact",
      "Get-ValidatedStartupIncompleteIdentityFact",
      "Update-OwnedRuntimeTree",
      "Get-FrontendListenerIdentity",
    ];
    const source = [
      "$ErrorActionPreference = 'Stop'",
      "Set-StrictMode -Version Latest",
      "$tokens = $null; $errors = $null",
      `$ast = [Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)}, [ref]$tokens, [ref]$errors)`,
      `foreach ($name in @(${names.map(quote).join(", ")})) {`,
      "  $fn = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)",
      "  Invoke-Expression $fn.Extent.Text",
      "}",
      "$message = 'A remembered process lacks executable or creation identity.'",
      "$rootRow = [pscustomobject]@{ ProcessId = 91; ParentProcessId = 90; ExecutablePath = 'C:\\root.exe'; CreationDate = '2026-09-08T00:00:00.000Z' }",
      "$childRow = [pscustomobject]@{ ProcessId = 92; ParentProcessId = 91; ExecutablePath = ''; CreationDate = $null }",
      "$identity = [ordered]@{ processId = 91; executablePath = 'C:\\root.exe'; creationDate = '2026-09-08T00:00:00.000Z' }",
      "$runtime = [ordered]@{ identity = $identity; store = [Collections.Generic.Dictionary[string, object]]::new() }",
      "$results = @()",
      "function New-Incomplete([object]$target) { [Management.Automation.ErrorRecord]::new([Management.Automation.RuntimeException]::new($message), 'incomplete', [Management.Automation.ErrorCategory]::InvalidData, $target) }",
      "function Get-FootprintProcessSnapshot { @($rootRow, $childRow) }",
      "$rootFailure = New-Incomplete $rootRow",
      "function Get-FootprintRootIdentity { throw $script:rootFailure }",
      "try { Update-OwnedRuntimeTree $runtime } catch { $results += [ordered]@{ seam = 'update-root'; sameException = [object]::ReferenceEquals($_.Exception, $rootFailure.Exception); message = $_.Exception.Message; type = $_.Exception.GetType().FullName; fact = Get-ValidatedStartupIncompleteIdentityFact $_ } }",
      "$descendant = $childRow",
      "function Get-FootprintRootIdentity { return $identity }",
      "function Get-FootprintDescendants { param($id, $rows, $RootCreationDate) @($descendant) }",
      "$descendantFailure = New-Incomplete $childRow",
      "function Add-FootprintRememberedIdentity { param($store, $value) throw $script:descendantFailure }",
      "try { Update-OwnedRuntimeTree $runtime } catch { $results += [ordered]@{ seam = 'update-descendant'; sameException = [object]::ReferenceEquals($_.Exception, $descendantFailure.Exception); message = $_.Exception.Message; type = $_.Exception.GetType().FullName; fact = Get-ValidatedStartupIncompleteIdentityFact $_ } }",
      "function Get-NetTCPConnection { [pscustomobject]@{ OwningProcess = 91 } }",
      "$listenerFailure = New-Incomplete $rootRow",
      "function ConvertTo-FootprintProcessIdentity { param($value) throw $script:listenerFailure }",
      "try { Get-FrontendListenerIdentity ([ordered]@{ port = 1431 }) } catch { $results += [ordered]@{ seam = 'frontend-listener'; sameException = [object]::ReferenceEquals($_.Exception, $listenerFailure.Exception); message = $_.Exception.Message; type = $_.Exception.GetType().FullName; fact = Get-ValidatedStartupIncompleteIdentityFact $_ } }",
      "function Add-StartupIncompleteIdentityFact { throw 'injected attachment failure' }",
      "$attachmentFailure = New-Incomplete $rootRow",
      "function Get-FootprintRootIdentity { throw $script:attachmentFailure }",
      "try { Update-OwnedRuntimeTree $runtime } catch { $results += [ordered]@{ seam = 'attachment'; sameException = [object]::ReferenceEquals($_.Exception, $attachmentFailure.Exception); message = $_.Exception.Message; type = $_.Exception.GetType().FullName } }",
      "$results | ConvertTo-Json -Depth 5 -Compress",
    ].join("\n");
    const result = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-EncodedCommand",
        Buffer.from(source, "utf16le").toString("base64"),
      ],
      { encoding: "utf8", windowsHide: true, timeout: 15_000 },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    const observed = JSON.parse(result.stdout.trim());
    for (const [index, seam] of [
      "update-root",
      "update-descendant",
      "frontend-listener",
      "attachment",
    ].entries()) {
      const entry = observed[index];
      assert.equal(entry.seam, seam);
      assert.equal(entry.sameException, true);
      assert.equal(
        entry.message,
        "A remembered process lacks executable or creation identity.",
      );
      assert.equal(entry.type, "System.Management.Automation.RuntimeException");
      if (seam === "attachment") continue;
      assert.equal(entry.fact.stage, seam);
      assert.equal(
        entry.fact.processId,
        seam === "update-descendant" ? 92 : 91,
      );
      assert.equal(
        entry.fact.parentProcessId,
        seam === "update-descendant" ? 91 : 90,
      );
      assert.equal(
        entry.fact.isRoot,
        seam === "update-root"
          ? true
          : seam === "update-descendant"
            ? false
            : null,
      );
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows startup diagnostic write failure preserves the actual outer catch and cleanup flow", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-startup-outer-"));
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
    const source = [
      "$ErrorActionPreference = 'Stop'",
      "Set-StrictMode -Version Latest",
      "$tokens = $null; $errors = $null",
      `$ast = [Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)}, [ref]$tokens, [ref]$errors)`,
      "$main = $ast.Find({ param($node) $node -is [Management.Automation.Language.TryStatementAst] -and $node.CatchClauses.Count -eq 1 -and $node.CatchClauses[0].Body.Extent.Text.Contains('Write-StartupIncompleteIdentityDiagnostic') }, $true)",
      "$tail = @($ast.FindAll({ param($node) $node -is [Management.Automation.Language.IfStatementAst] -and $node.Extent.Text.Contains('if ($null -ne $journeyFailure) { throw $journeyFailure }') }, $true)) | Select-Object -First 1",
      "if ($null -eq $main -or $null -eq $tail) { throw 'missing journey catch/finally or rethrow' }",
      "$script:writeCalls = 0; $script:cleanupCalls = 0",
      "function Write-StartupIncompleteIdentityDiagnostic { param($EvidenceRoot, $Failure, $ViteRuntime) $script:writeCalls++; throw [IO.IOException]::new('injected diagnostic write failure') }",
      "function Stop-OwnedRuntimeTree { param($Role, $Runtime, $EvidenceRoot) $script:cleanupCalls++; return [ordered]@{ terminalZero = $true } }",
      "$previous = @{ profile = $null; project = $null; webview = $null }; $EvidenceRoot = " +
        quote(root) +
        "; $viteRuntime = $null; $sageRuntime = $null; $sageCleanup = $null; $viteCleanup = $null; $cleanupFailures = @(); $journeyFailure = $null",
      "$originalException = [Management.Automation.RuntimeException]::new('A remembered process lacks executable or creation identity.'); $original = [Management.Automation.ErrorRecord]::new($originalException, 'incomplete', [Management.Automation.ErrorCategory]::InvalidData, $null)",
      "try { throw $original } catch { Invoke-Expression ('. ' + $main.CatchClauses[0].Body.Extent.Text) } finally { Invoke-Expression ('. ' + $main.Finally.Extent.Text) }",
      "$final = $null; try { Invoke-Expression $tail.Extent.Text } catch { $final = $_ }",
      "[ordered]@{ writeCalls = $script:writeCalls; cleanupCalls = $script:cleanupCalls; sameJourneyException = [object]::ReferenceEquals($journeyFailure.Exception, $originalException); sameFinalException = [object]::ReferenceEquals($final.Exception, $originalException); journeyMessage = $journeyFailure.Exception.Message; finalMessage = $final.Exception.Message; journeyType = $journeyFailure.Exception.GetType().FullName; finalType = $final.Exception.GetType().FullName; cleanupFailures = $cleanupFailures.Count } | ConvertTo-Json -Compress",
    ].join("\n");
    const result = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-EncodedCommand",
        Buffer.from(source, "utf16le").toString("base64"),
      ],
      { encoding: "utf8", windowsHide: true, timeout: 15_000 },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    const observed = JSON.parse(result.stdout.trim());
    assert.equal(observed.writeCalls, 1);
    assert.equal(observed.cleanupCalls, 2);
    assert.equal(observed.cleanupFailures, 0);
    assert.equal(observed.sameJourneyException, true);
    assert.equal(observed.sameFinalException, true);
    assert.equal(
      observed.journeyMessage,
      "A remembered process lacks executable or creation identity.",
    );
    assert.equal(observed.finalMessage, observed.journeyMessage);
    assert.equal(
      observed.journeyType,
      "System.Management.Automation.RuntimeException",
    );
    assert.equal(observed.finalType, observed.journeyType);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("native IDE Project readiness scopes the retained file to its admitted tree", () => {
  const journeyItem = {};
  const outsideItem = {};
  let scopedItems = [journeyItem];
  const tree = {
    querySelectorAll: (selector) =>
      selector === '[role="treeitem"][aria-label="Open file journey.ts"]'
        ? scopedItems
        : [],
  };
  const outsideTree = {};
  let scopedTrees = [tree];
  const projectTools = {
    querySelectorAll: (selector) =>
      selector === '[role="tree"][aria-label="Project files"]'
        ? scopedTrees
        : [],
  };
  const document = {
    querySelectorAll: (selector) => {
      if (selector === 'aside[aria-label="Project tools"]')
        return [projectTools];
      if (selector === '[role="tree"][aria-label="Project files"]')
        return [tree, outsideTree];
      if (selector === '[role="treeitem"][aria-label="Open file journey.ts"]')
        return [journeyItem, outsideItem];
      return [];
    },
  };
  assert.equal(
    evaluateBrowserExpression(uiAction("Project").expression, { document }),
    true,
  );
  assert.equal(
    evaluateBrowserExpression(verificationFor("Explorer"), { document }),
    true,
  );

  scopedTrees = [tree, {}];
  assert.equal(
    evaluateBrowserExpression(uiAction("Project").expression, { document }),
    false,
  );
  assert.equal(
    evaluateBrowserExpression(verificationFor("Explorer"), { document }),
    false,
  );

  scopedTrees = [tree];
  scopedItems = [];
  assert.equal(
    evaluateBrowserExpression(uiAction("Project").expression, { document }),
    false,
  );
  assert.equal(
    evaluateBrowserExpression(verificationFor("Explorer"), { document }),
    false,
  );
});

test("native IDE Project waits for pure readiness within one fixed deadline", async () => {
  const project = uiAction("Project");
  const calls = [];
  const deadline = Date.now() + 1_000;
  await performUiAction(
    {
      evaluate: async (expression, actualDeadline) => {
        calls.push({ expression, actualDeadline });
        return calls.length === 2;
      },
    },
    project,
    deadline,
  );
  assert.deepEqual(
    calls.map(({ expression }) => expression),
    [project.expression, project.expression],
  );
  assert.deepEqual(
    new Set(calls.map(({ actualDeadline }) => actualDeadline)),
    new Set([deadline]),
  );
});

test("native IDE Project times out truthfully without retrying click actions", async () => {
  const projectCalls = [];
  await assert.rejects(
    performUiAction(
      {
        evaluate: async (expression) => {
          projectCalls.push(expression);
          return false;
        },
      },
      uiAction("Project"),
      Date.now() + 20,
    ),
    /Project readiness timed out/u,
  );
  assert.ok(projectCalls.length >= 1);

  const search = uiAction("Search");
  const clickCalls = [];
  await assert.rejects(
    performUiAction(
      {
        evaluate: async (expression) => {
          clickCalls.push(expression);
          return false;
        },
      },
      search,
      Date.now() + 1_000,
    ),
    /Search control is ambiguous/u,
  );
  assert.deepEqual(clickCalls, [search.expression]);
});

test("native IDE Explorer action leaves the retained expanded panel open", () => {
  let clicks = 0;
  const explorer = {
    getAttribute: (name) => (name === "aria-expanded" ? "true" : null),
    click: () => {
      clicks += 1;
    },
  };
  const document = {
    querySelectorAll: (selector) =>
      selector === 'button[aria-label="Explorer"]' ? [explorer] : [],
  };
  assert.equal(
    evaluateBrowserExpression(uiAction("Explorer").expression, { document }),
    true,
  );
  assert.equal(clicks, 0);
  explorer.getAttribute = () => "false";
  assert.equal(
    evaluateBrowserExpression(uiAction("Explorer").expression, { document }),
    true,
  );
  assert.equal(clicks, 1);
});

test("native IDE Search readiness selects the exact panel input", () => {
  const searchInput = {};
  let scopedInputs = [searchInput];
  const form = {
    querySelectorAll: (selector) => {
      if (selector === 'input[type="search"][aria-label="Search text"]')
        return scopedInputs;
      return [];
    },
  };
  let forms = [form];
  const document = {
    querySelectorAll: (selector) =>
      selector === ".workspace-search__form" ? forms : [],
  };
  assert.equal(
    evaluateBrowserExpression(verificationFor("Search"), { document }),
    true,
  );

  forms = [form, {}];
  assert.equal(
    evaluateBrowserExpression(verificationFor("Search"), { document }),
    false,
  );

  forms = [form];
  scopedInputs = [];
  assert.equal(
    evaluateBrowserExpression(verificationFor("Search"), { document }),
    false,
  );
});

test("native IDE File action searches through the exact panel form, not the header", () => {
  class HTMLInputElement {
    set value(next) {
      this.assignedValue = next;
    }

    dispatchEvent(event) {
      this.event = event;
      return true;
    }
  }
  class Event {
    constructor(type, options) {
      this.type = type;
      this.bubbles = options?.bubbles === true;
      this.cancelable = options?.cancelable === true;
    }
  }
  const input = new HTMLInputElement();
  const header = { textContent: "Search files", clicks: 0 };
  const decoy = {
    textContent: "Search",
    clicks: 0,
    click() {
      this.clicks += 1;
    },
  };
  const outsideInput = new HTMLInputElement();
  const outsideSubmit = {
    textContent: "Search",
    clicks: 0,
    click() {
      this.clicks += 1;
    },
  };
  const form = {
    querySelectorAll: (selector) => {
      if (selector === 'input[type="search"][aria-label="Search text"]')
        return [input];
      return [];
    },
    dispatchEvent(event) {
      this.event = event;
      return false;
    },
  };
  const document = {
    querySelector: (selector) =>
      selector === '[aria-label="Search files"]' ? header : null,
    querySelectorAll: (selector) => {
      if (selector === ".workspace-search__form") return [form];
      if (selector === 'input[type="search"][aria-label="Search text"]')
        return [input, outsideInput];
      if (selector === "button") return [decoy, outsideSubmit];
      return [];
    },
  };
  assert.equal(
    evaluateBrowserExpression(uiAction("File").expression, {
      document,
      HTMLInputElement,
      Event,
    }),
    true,
  );
  assert.equal(input.assignedValue, "SAGE_JOURNEY_SENTINEL");
  assert.deepEqual(
    { type: input.event.type, bubbles: input.event.bubbles },
    { type: "input", bubbles: true },
  );
  assert.deepEqual(
    {
      type: form.event.type,
      bubbles: form.event.bubbles,
      cancelable: form.event.cancelable,
    },
    { type: "submit", bubbles: true, cancelable: true },
  );
  assert.equal(outsideSubmit.clicks, 0);
  assert.equal(decoy.clicks, 0);
  assert.equal(header.clicks, 0);
});

test("native IDE File action rejects ambiguous or split Search forms", () => {
  const input = {};
  let scopedInputs = [input];
  const outsideSubmit = {
    clicks: 0,
    click() {
      this.clicks += 1;
    },
  };
  const form = {
    querySelectorAll: (selector) =>
      selector === 'input[type="search"][aria-label="Search text"]'
        ? scopedInputs
        : [],
  };
  let forms = [form, {}];
  const document = {
    querySelectorAll: (selector) => {
      if (selector === ".workspace-search__form") return forms;
      if (selector === 'button[type="submit"]') return [outsideSubmit];
      return [];
    },
  };

  assert.equal(
    evaluateBrowserExpression(uiAction("File").expression, { document }),
    false,
  );
  forms = [form];
  scopedInputs = [];
  assert.equal(
    evaluateBrowserExpression(uiAction("File").expression, { document }),
    false,
  );
  assert.equal(outsideSubmit.clicks, 0);
});

test("native IDE search result contract opens the exact fixture match", () => {
  const result = {
    clicks: 0,
    click() {
      this.clicks += 1;
    },
  };
  const selector =
    '.workspace-search__results[role="list"] [role="listitem"] > button.workspace-search__result[aria-label="Open journey.ts at line 1, column 14"]';
  const document = {
    querySelectorAll: (candidate) => (candidate === selector ? [result] : []),
  };
  assert.equal(
    evaluateBrowserExpression(verificationFor("File"), { document }),
    true,
  );
  assert.equal(
    evaluateBrowserExpression(UI_PROBES.openExactSearchResult, {
      document,
    }),
    true,
  );
  assert.equal(result.clicks, 1);
});

test("native IDE File and edit-outline preparations are strictly ordered", async () => {
  const fileCalls = [];
  await performUiMilestone(
    {
      evaluate: async (expression) => {
        fileCalls.push(expression);
        return true;
      },
    },
    uiAction("File"),
    Date.now() + 1_000,
  );
  assert.deepEqual(fileCalls, [
    uiAction("File").expression,
    verificationFor("File"),
    UI_PROBES.openExactSearchResult,
    UI_PROBES.editableEditor,
  ]);

  const editCalls = [];
  const names = await performUiMilestone(
    {
      evaluate: async (expression) => {
        editCalls.push(expression);
        return expression === UI_PROBES.outlineNames
          ? JSON.stringify(["journeyEdited"])
          : true;
      },
    },
    uiAction("edit-outline"),
    Date.now() + 1_000,
  );
  assert.deepEqual(editCalls, [
    uiAction("edit-outline").expression,
    UI_PROBES.outlineMounted,
    UI_PROBES.insertEditorEdit,
    UI_PROBES.outlineReady,
    UI_PROBES.outlineNames,
  ]);
  assert.equal(names, JSON.stringify(["journeyEdited"]));
});

test("native IDE edit inserts the admitted declaration once", () => {
  const calls = [];
  const editor = { focus: () => calls.push(["focus"]) };
  const document = {
    querySelectorAll: () => [editor],
    execCommand: (...arguments_) => {
      calls.push(arguments_);
      return true;
    },
  };
  assert.equal(
    evaluateBrowserExpression(UI_PROBES.insertEditorEdit, {
      document,
    }),
    true,
  );
  assert.deepEqual(calls, [
    ["focus"],
    ["insertText", false, "\nexport function journeyEdited() {}\n"],
  ]);
});

test("native IDE Outline probes bind one exact panel and the inserted declaration", () => {
  const outlineButton = {
    clicks: 0,
    click() {
      this.clicks += 1;
    },
  };
  let names = ["journey"];
  const cards = () => names.map((name) => ({ textContent: name }));
  const outline = {
    querySelectorAll: (selector) =>
      selector === "button.outline-card .outline-card__name" ? cards() : [],
  };
  let panels = 1;
  const panel = {
    querySelectorAll: (selector) =>
      selector === 'section[aria-label="Outline"]' ? [outline] : [],
  };
  const document = {
    querySelectorAll: (selector) => {
      if (selector === 'button[aria-label="Outline"]') return [outlineButton];
      return selector === 'section[aria-label="Outline project tools"]'
        ? Array.from({ length: panels }, () => panel)
        : [];
    },
  };
  assert.equal(
    evaluateBrowserExpression(uiAction("edit-outline").expression, {
      document,
    }),
    true,
  );
  assert.equal(outlineButton.clicks, 1);
  assert.equal(
    evaluateBrowserExpression(UI_PROBES.outlineMounted, { document }),
    true,
  );
  assert.equal(
    evaluateBrowserExpression(UI_PROBES.outlineReady, { document }),
    false,
  );
  names = ["journey", "journeyEdited"];
  assert.equal(
    evaluateBrowserExpression(UI_PROBES.outlineReady, { document }),
    true,
  );
  assert.deepEqual(
    JSON.parse(evaluateBrowserExpression(UI_PROBES.outlineNames, { document })),
    names,
  );
  panels = 2;
  assert.equal(
    evaluateBrowserExpression(UI_PROBES.outlineMounted, { document }),
    false,
  );
  assert.equal(
    evaluateBrowserExpression(UI_PROBES.outlineNames, { document }),
    null,
  );
});

test("native IDE save action uses the admitted File editor shortcut", () => {
  let dirty = true;
  let conflictVisible = true;
  const received = [];
  class TestKeyboardEvent {
    constructor(type, init) {
      this.type = type;
      this.defaultPrevented = false;
      Object.assign(this, init);
    }
    preventDefault() {
      this.defaultPrevented = true;
    }
  }
  const editor = {
    dispatchEvent(event) {
      received.push(event);
      event.preventDefault();
      return false;
    },
    querySelectorAll: (selector) => {
      if (selector === ".file-header__save-state--dirty")
        return dirty ? [{}] : [];
      if (selector === ".editor-notice--conflict")
        return conflictVisible ? [{}] : [];
      if (selector === ".cm-content")
        return [{ innerText: "saved journey content" }];
      return [];
    },
  };
  const document = {
    querySelector: (selector) =>
      selector === '[aria-label="File editor journey.ts"] .cm-content'
        ? { innerText: "wrong global content" }
        : null,
    querySelectorAll: (selector) => {
      if (selector === 'section[aria-label="File editor journey.ts"]')
        return [editor];
      if (selector === ".tab--active .tab__dirty") return dirty ? [{}] : [];
      return [];
    },
  };
  assert.equal(
    evaluateBrowserExpression(uiAction("save").expression, {
      document,
      KeyboardEvent: TestKeyboardEvent,
    }),
    true,
  );
  assert.equal(received.length, 1);
  assert.equal(received[0].key, "s");
  assert.equal(received[0].ctrlKey, true);
  dirty = false;
  assert.equal(
    evaluateBrowserExpression(UI_PROBES.saveSettled, { document }),
    false,
  );
  conflictVisible = false;
  assert.equal(
    evaluateBrowserExpression(UI_PROBES.saveSettled, { document }),
    true,
  );
  assert.equal(
    evaluateBrowserExpression(UI_PROBES.savedContent, { document }),
    "saved journey content",
  );
});

test("native IDE Source Control and Diff readiness wait for the exact ready change", () => {
  let diffRows = [];
  const sourcePanel = {
    querySelectorAll: (selector) =>
      selector === 'button[aria-label="Open diff for journey.ts"]'
        ? diffRows
        : [],
  };
  const diffEditor = { textContent: "", innerText: "" };
  const diffSurface = {
    innerText: "",
    querySelectorAll: (selector) =>
      selector === '.diff-editor[role="table"][aria-label="Unified file diff"]'
        ? [diffEditor]
        : [],
  };
  const document = {
    querySelector: (selector) =>
      selector === '[aria-label="Diff for journey.ts"]'
        ? { innerText: "wrong loading surface" }
        : null,
    querySelectorAll: (selector) => {
      if (
        selector === '[aria-label="Source Control project tools"]' ||
        selector === 'section[aria-label="Source Control project tools"]'
      )
        return [sourcePanel];
      if (
        selector === '[aria-label="Diff for journey.ts"]' ||
        selector === 'section.diff-surface[aria-label="Diff for journey.ts"]'
      )
        return [diffSurface];
      return [];
    },
  };

  assert.equal(
    evaluateBrowserExpression(verificationFor("Source Control"), { document }),
    false,
  );
  const diffRow = {
    clicks: 0,
    click() {
      this.clicks += 1;
    },
  };
  diffRows = [diffRow];
  assert.equal(
    evaluateBrowserExpression(verificationFor("Source Control"), { document }),
    true,
  );
  assert.equal(
    evaluateBrowserExpression(uiAction("Diff").expression, { document }),
    true,
  );
  assert.equal(diffRow.clicks, 1);

  assert.equal(
    evaluateBrowserExpression(verificationFor("Diff"), { document }),
    false,
  );
  diffEditor.textContent = "+export function journeyEdited() {}";
  diffEditor.innerText = diffEditor.textContent;
  diffSurface.innerText = `journey.ts\n${diffEditor.innerText}`;
  assert.equal(
    evaluateBrowserExpression(verificationFor("Diff"), { document }),
    true,
  );
  assert.equal(
    evaluateBrowserExpression(UI_PROBES.diffContent, { document }),
    diffSurface.innerText,
  );
});

test("native IDE recovery probes present and clear one external-change conflict", () => {
  const explorer = {
    clicks: 0,
    click() {
      this.clicks += 1;
    },
  };
  const file = {
    clicks: 0,
    click() {
      this.clicks += 1;
    },
  };
  const tree = {
    querySelectorAll: (selector) =>
      selector === '[role="treeitem"][aria-label="Open file journey.ts"]'
        ? [file]
        : [],
  };
  const tools = {
    querySelectorAll: (selector) =>
      selector === '[role="tree"][aria-label="Project files"]' ? [tree] : [],
  };
  const editorInput = {
    focus() {},
  };
  const conflict = { innerText: "External change detected. Disk changed." };
  const close = {
    clicks: 0,
    click() {
      this.clicks += 1;
    },
  };
  const fileTab = {
    textContent: "journey.ts",
    querySelector: (selector) => (selector === ".file-kind-icon" ? {} : null),
    querySelectorAll: (selector) =>
      selector === 'button[aria-label="Close journey.ts"]' ? [close] : [],
  };
  const discard = {
    textContent: "Discard",
    clicks: 0,
    click() {
      this.clicks += 1;
    },
  };
  let dialogVisible = true;
  let conflictVisible = true;
  const dialog = {
    innerText: "Save changes to journey.ts?",
    querySelectorAll: (selector) => (selector === "button" ? [discard] : []),
  };
  const document = {
    execCommand: (command, _ui, value) =>
      command === "insertText" && value.includes("SAGE_CONFLICT_EDIT"),
    querySelectorAll: (selector) => {
      if (selector === 'button[aria-label="Explorer"]') return [explorer];
      if (selector === 'aside[aria-label="Project tools"]') return [tools];
      if (
        selector ===
        '[role="tree"][aria-label="Project files"] [role="treeitem"][aria-label="Open file journey.ts"]'
      )
        return [file];
      if (
        selector ===
        'section[aria-label="File editor journey.ts"] .cm-content[contenteditable="true"]'
      )
        return [editorInput];
      if (
        selector ===
        'section[aria-label="File editor journey.ts"] .editor-notice--conflict[role="alert"]'
      )
        return conflictVisible ? [conflict] : [];
      if (selector === ".tab") return [fileTab];
      if (selector === 'section.dirty-dialog[role="dialog"]')
        return dialogVisible ? [dialog] : [];
      return [];
    },
  };

  assert.equal(
    evaluateBrowserExpression(uiAction("error-recovery").expression, {
      document,
    }),
    true,
  );
  assert.equal(explorer.clicks, 1);
  assert.equal(
    evaluateBrowserExpression(verificationFor("error-recovery"), { document }),
    true,
  );
  assert.equal(
    evaluateBrowserExpression(UI_PROBES.openExactExplorerFile, { document }),
    true,
  );
  assert.equal(file.clicks, 1);
  assert.equal(
    evaluateBrowserExpression(UI_PROBES.insertConflictEdit, { document }),
    true,
  );
  assert.equal(
    evaluateBrowserExpression(UI_PROBES.conflictVisible, { document }),
    true,
  );
  assert.equal(
    evaluateBrowserExpression(UI_PROBES.closeDirtyFile, { document }),
    true,
  );
  assert.equal(close.clicks, 1);
  assert.equal(
    evaluateBrowserExpression(UI_PROBES.dirtyDialogVisible, { document }),
    true,
  );
  assert.equal(
    evaluateBrowserExpression(UI_PROBES.discardDirtyClose, { document }),
    true,
  );
  assert.equal(discard.clicks, 1);
  dialogVisible = false;
  conflictVisible = false;
  assert.equal(
    evaluateBrowserExpression(UI_PROBES.recoverySettled, { document }),
    true,
  );
});

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
    "--executable-bytes",
    "123",
    "--executable-sha256",
    "c".repeat(64),
    "--node-bytes",
    "456",
    "--node-sha256",
    "e".repeat(64),
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
  assert.equal(parsed.nodeBytes, 456);
  assert.equal(parsed.nodeSha256, "e".repeat(64));
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
    const input = {
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
      automationTransport: rawCdpTransportReceipt(),
      observations: observed(),
      milestones: JOURNEY_STEPS.map((step, index) => ({
        step,
        ordinal: index + 1,
      })),
      artifacts: [screenshot, log],
    };
    const receipt = createJourneyReceipt(input);
    assert.equal(receipt.completed, true);
    assert.equal(receipt.schemaVersion, 6);
    assert.deepEqual(
      receipt.milestones.map(({ step }) => step),
      JOURNEY_STEPS,
    );
    assert.equal(receipt.artifacts.length, 2);
    assert.match(receipt.artifacts[0].sha256, /^[a-f0-9]{64}$/u);
    assert.equal("path" in receipt.artifacts[0], false);
    assert.equal(receipt.sourceBinding, "collection-only");
    assert.deepEqual(receipt.runtime, runtimeReceipt());
    assert.deepEqual(receipt.automationTransport, rawCdpTransportReceipt());
    assert.deepEqual(Object.keys(receipt.automationTransport).sort(), [
      "connectionCount",
      "kind",
      "maxInFlight",
      "node",
      "requestCount",
    ]);
    assert.equal(receipt.automationTransport.node.path, process.execPath);
    assert.equal(Object.isFrozen(receipt.automationTransport), true);
    assert.equal(Object.isFrozen(receipt.automationTransport.node), true);
    assert.deepEqual(Object.keys(receipt.runtime).sort(), [
      "frontendReady",
      "mode",
    ]);
    for (const automationTransport of [
      {
        ...rawCdpTransportReceipt(),
        node: {
          ...rawCdpTransportReceipt().node,
          bytes: 0,
        },
      },
      { ...rawCdpTransportReceipt(), connectionCount: 2 },
      { ...rawCdpTransportReceipt(), maxInFlight: 2 },
      { ...rawCdpTransportReceipt(), requestCount: 40 },
      { ...rawCdpTransportReceipt(), requestCount: 1025 },
      {
        ...rawCdpTransportReceipt(),
        source: { commit: "a".repeat(40), tree: "b".repeat(40) },
      },
    ]) {
      assert.throws(
        () =>
          createJourneyReceipt({
            ...input,
            automationTransport,
          }),
        /journey-receipt-invalid/u,
      );
    }
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
      automationTransport: rawCdpTransportReceipt(),
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

test("native IDE receipt refuses unobserved save, Outline, and Diff claims", () => {
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
          automationTransport: rawCdpTransportReceipt(),
          milestones: JOURNEY_STEPS.map((step, index) => ({
            step,
            ordinal: index + 1,
          })),
          artifacts: [artifact, log],
          evidenceRoot: root,
          observations: {
            ...observed(),
            save: { ...observed().save, dirtyIndicatorCleared: false },
          },
        }),
      /journey-observations-invalid/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("native IDE screenshot decoding rejects non-string results without coercion", () => {
  for (const value of [{ encoded: "Zm9v" }, true, null, ["Zm9v"]]) {
    assert.throws(
      () => decodeBoundedScreenshot(value),
      /journey-screenshot-invalid/,
    );
  }
});

test("native IDE screenshot decoding admits only nonempty canonical base64", () => {
  for (const encoded of [
    "",
    "Zm 9v",
    "Zm9v\n",
    "Zm9*",
    "Zg=",
    "Zg===",
    "Zh==",
    "Zm9=",
  ]) {
    assert.throws(
      () => decodeBoundedScreenshot(encoded),
      /journey-screenshot-invalid/,
      encoded,
    );
  }
  assert.deepEqual(decodeBoundedScreenshot("Zm9v"), Buffer.from("foo"));
});

test("native IDE screenshot decoding refuses encoded payloads beyond the byte bound", () => {
  assert.throws(
    () => decodeBoundedScreenshot("A".repeat(12 * 1024 * 1024)),
    /journey-screenshot-oversize/,
  );
});

test("native IDE screenshot decoding accepts exactly 8 MiB and rejects one decoded byte more", () => {
  const boundary = Buffer.alloc(8 * 1024 * 1024, 0xa5);
  const encodedBoundary = boundary.toString("base64");
  assert.equal(encodedBoundary.length, 4 * Math.ceil(boundary.length / 3));
  assert.deepEqual(decodeBoundedScreenshot(encodedBoundary), boundary);

  const overflow = Buffer.alloc(8 * 1024 * 1024 + 1, 0xa5).toString("base64");
  assert.throws(
    () => decodeBoundedScreenshot(overflow),
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
  const projected = admitCdpTarget(
    [
      {
        ...page("one", "http://127.0.0.1:1420/"),
        description: "ambient CDP field",
        devtoolsFrontendUrl: "/devtools/inspector.html",
      },
    ],
    9229,
    "http://127.0.0.1:1420/",
  );
  assert.deepEqual(Object.keys(projected).sort(), [
    "id",
    "title",
    "type",
    "url",
    "webSocketDebuggerUrl",
  ]);
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

test("native IDE journey attaches one persistent raw CDP transport", async () => {
  const admitted = {
    id: "page-1",
    type: "page",
    title: "Sage",
    url: "http://127.0.0.1:1420/",
    webSocketDebuggerUrl: "ws://127.0.0.1:9229/devtools/page/page-1",
  };
  const expectedClient = {
    evaluate: async () => true,
    screenshot: async () => "",
    close: async () => {},
    stats: { connectionCount: 1, maxInFlight: 1, requestCount: 1 },
  };
  const connections = [];
  const realNow = Date.now;
  let clock = realNow.call(Date);
  Date.now = () => {
    clock += 25;
    return clock;
  };
  let started;
  let deadline;
  let actual;
  let settled;
  try {
    started = Date.now();
    deadline = started + 90_000;
    actual = await attachCdp(admitted, deadline, {
      connectCdp: async (options) => {
        connections.push(options);
        return expectedClient;
      },
    });
    settled = Date.now();
  } finally {
    Date.now = realNow;
  }
  assert.equal(actual, expectedClient);
  assert.equal(connections.length, 1);
  assert.equal(connections[0].target, admitted);
  assert.equal(connections[0].deadline, deadline);
  assert.ok(connections[0].openDeadline >= started + 10_000);
  assert.ok(connections[0].openDeadline <= settled + 10_000);
  assert.ok(connections[0].openDeadline < deadline);
});

test("native IDE journey decodes decorated Playwright results and binds the stable target", async () => {
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
  const transport = playwrightExecutionTransport();
  let validations = 0;
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
        transport,
        validateTransport: (actual) => {
          assert.equal(actual, transport);
          validations += 1;
          return actual;
        },
        runTool: async (tool, args, _deadline, _cwd, toolOptions) => {
          assert.equal(tool, transport.node.path);
          assert.equal(args[0], transport.entryPath);
          assert.equal(toolOptions.shell, undefined);
          const playwrightArgs = args.slice(1);
          calls.push(playwrightArgs);
          if (playwrightArgs.includes("eval")) {
            return "### Result\ntrue\n### Ran Playwright code\ntrue";
          }
          if (!playwrightArgs.includes("run-code")) return "";
          const payload = playwrightArgs.at(-1);
          assert.match(payload, /^async page =>/u);
          const callback = Function(`"use strict"; return (${payload});`)();
          assert.equal(typeof callback, "function");
          return `### Result\n${JSON.stringify(await callback(page))}\n### Ran Playwright code\n${payload}`;
        },
      },
    );
    assert.equal(await playwright.evaluate("true"), true);
    assert.deepEqual(await playwright.inspectTarget(), controlled);
    const screenshot = await playwright.screenshot("command-contract.png");
    assert.deepEqual(readFileSync(screenshot), screenshotBytes);
    await playwright.close();
    const runCodeCalls = calls.filter((args) => args.includes("run-code"));
    assert.equal(runCodeCalls.length, 2);
    assert.match(runCodeCalls[0].at(-1), /newCDPSession\(page\)/u);
    assert.match(runCodeCalls[0].at(-1), /Target\.getTargetInfo/u);
    assert.match(runCodeCalls[1].at(-1), /page\.screenshot\(\)/u);
    assert.equal(validations, 5);
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

test("native IDE screenshot capture does not coerce parsed non-string results", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-ide-screenshot-value-"));
  try {
    for (const value of [{ encoded: "Zm9v" }, true]) {
      const playwright = await attachPlaywright(
        {
          playwrightCli: "C:\\tools\\playwright-cli.cmd",
          output: root,
          sourceRoot: root,
          port: 9229,
        },
        Date.now() + 5000,
        {
          transport: playwrightExecutionTransport(),
          validateTransport: (transport) => transport,
          runTool: async (_tool, args) =>
            args.includes("run-code")
              ? `### Result\n${JSON.stringify(value)}\n### Ran Playwright code\nasync page => page.screenshot()`
              : "",
        },
      );
      await assert.rejects(
        playwright.screenshot("must-not-exist.png"),
        /journey-screenshot-invalid/u,
      );
      assert.equal(existsSync(path.join(root, "must-not-exist.png")), false);
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("native IDE journey rejects ambiguous or unbounded Playwright result envelopes", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-ide-output-envelope-"));
  const cases = [
    ["missing", "true"],
    [
      "duplicate",
      "### Result\ntrue\n### Result\nfalse\n### Ran Playwright code\ntrue",
    ],
    [
      "misordered",
      "### Ran Playwright code\ntrue\n### Result\ntrue\n### Ran Playwright code\ntrue",
    ],
    [
      "oversize",
      `### Result\n${JSON.stringify("x".repeat(64 * 1024))}\n### Ran Playwright code\ntrue`,
    ],
    [
      "trailing ambiguous result",
      "### Result\ntrue\n### Ran Playwright code\ntrue\n### Result",
    ],
    ["raw fallback", 'diagnostic\n{"id":"page-1"}'],
  ];
  try {
    for (const [name, output] of cases) {
      const playwright = await attachPlaywright(
        {
          playwrightCli: "C:\\tools\\playwright-cli.cmd",
          output: root,
          sourceRoot: root,
          port: 9229,
        },
        Date.now() + 5000,
        {
          transport: playwrightExecutionTransport(),
          validateTransport: (transport) => transport,
          runTool: async (_tool, args) => (args.includes("eval") ? output : ""),
        },
      );
      await assert.rejects(
        playwright.evaluate("true"),
        /journey-playwright-output-invalid/u,
        name,
      );
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
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

test("Node identity requires a canonical path with only plain ancestors", () => {
  const expected = { bytes: 456, sha256: "e".repeat(64) };
  const inspected = [];
  const actual = validatePlainCanonicalExecutable(process.execPath, expected, {
    lstatSync: (current) => {
      inspected.push(current);
      return { isSymbolicLink: () => false };
    },
    realpathSync: (current) => current,
    validateBinding: (current, binding) => {
      assert.equal(current, process.execPath);
      return binding;
    },
  });
  assert.deepEqual(actual, { path: process.execPath, ...expected });
  assert.ok(inspected.includes(path.parse(process.execPath).root));
  if (process.platform === "win32") {
    const extendedPath = `\\\\?\\${process.execPath}`;
    assert.deepEqual(
      validatePlainCanonicalExecutable(extendedPath, expected, {
        validateBinding: () => expected,
      }),
      { path: extendedPath, ...expected },
    );
  }
  assert.throws(
    () =>
      validatePlainCanonicalExecutable(
        `${path.dirname(process.execPath)}${path.sep}.${path.sep}${path.basename(process.execPath)}`,
        expected,
      ),
    /journey-executable-identity-invalid/u,
  );
  assert.throws(
    () =>
      validatePlainCanonicalExecutable(process.execPath, expected, {
        lstatSync: (current) => ({
          isSymbolicLink: () => current === path.dirname(process.execPath),
        }),
        realpathSync: (current) => current,
        validateBinding: () => expected,
      }),
    /journey-executable-identity-invalid/u,
  );
});

test("Windows verbatim executable paths match their canonical filesystem form", () => {
  const expected = { bytes: 456, sha256: "e".repeat(64) };
  const verbatim = `\\\\?\\${process.execPath}`;
  const actual = validatePlainCanonicalExecutable(verbatim, expected, {
    lstatSync: () => ({ isSymbolicLink: () => false }),
    realpathSync: (current) =>
      current.startsWith("\\\\?\\") ? current.slice(4) : current,
    validateBinding: (current, binding) => {
      assert.equal(current, verbatim);
      return binding;
    },
  });
  assert.deepEqual(actual, { path: verbatim, ...expected });
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

test("bounded tool releases an identity handshake only after exact capture", async () => {
  const child = new EventEmitter();
  child.stdout = new PassThrough();
  child.stderr = new PassThrough();
  child.pid = 63;
  let acknowledgement;
  child.stdin = {
    destroyed: false,
    writable: true,
    end(value) {
      acknowledgement = value;
      this.writable = false;
    },
  };
  const pending = runBoundedTool(
    "tool",
    [],
    Date.now() + 5_000,
    process.cwd(),
    {
      spawn: (_executable, _args, options) => {
        assert.deepEqual(options.stdio, ["pipe", "pipe", "pipe"]);
        assert.equal(options.windowsVerbatimArguments, false);
        assert.equal(options.cwd, "D:\\source fixture");
        return child;
      },
      identityHandshake: "captured-owner\r\n",
      spawnCwd: "D:\\source fixture",
      captureIdentity: (pid) => ({ processId: pid, creationDate: "exact" }),
    },
  );
  assert.equal(acknowledgement, "captured-owner\r\n");
  child.emit("close", 0);
  assert.equal(await pending, "");
});

test("Windows bounded tool handshake preserves quoted cmd arguments after exact capture", async () => {
  const root = mkdtempSync(
    path.join(tmpdir(), "sage bounded handshake with spaces-"),
  );
  const fixture = path.join(root, "Codex Fixture.cmd");
  const command = path.join(process.env.SystemRoot, "System32", "cmd.exe");
  try {
    writeFileSync(fixture, "@echo off\r\necho codex-cli 0.146.0\r\n", "utf8");
    const output = await runBoundedTool(
      command,
      [
        "/d",
        "/s",
        "/c",
        `set /p "SAGE_ACP04_ACK=" && call "${fixture}" --version`,
      ],
      Date.now() + 15_000,
      root,
      {
        commandRole: "codex-version-preflight",
        expectedExecutablePath: command,
        identityHandshake: "captured-owner\r\n",
        maxOutputBytes: 4096,
        windowsVerbatimArguments: true,
      },
    );
    assert.equal(output.trim(), "codex-cli 0.146.0");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("identity capture failure releases the handshake gate and retains quarantine", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-tool-handshake-"));
  try {
    const child = new EventEmitter();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    child.pid = 64;
    let acknowledgement = "unclosed";
    child.stdin = {
      destroyed: false,
      writable: true,
      end(value) {
        acknowledgement = value;
        this.writable = false;
      },
      destroy() {
        this.destroyed = true;
      },
    };
    await assert.rejects(
      runBoundedTool("tool", [], Date.now() + 5_000, root, {
        spawn: () => child,
        identityHandshake: "must-not-reach-child\r\n",
        captureIdentity: () => {
          throw new Error("identity unavailable");
        },
      }),
      /playwright-cli-cleanup-unproven/,
    );
    assert.equal(acknowledgement, undefined);
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

test("Windows npm Playwright transport invokes the authenticated JS entry byte-exactly through Node", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-playwright-transport-"));
  const environmentKeys = [
    "NoDe_BeNiGn_PrElOaD",
    "PlAyWrIgHt_PrElOaD",
    "pW_pReLoAd",
  ];
  const previousEnvironment = new Map(
    environmentKeys.map((name) => [name, process.env[name]]),
  );
  try {
    const fixture = writeNpmPlaywrightFixture(
      path.join(root, "npm graph with spaces"),
    );
    let directCaptureCalls = 0;
    let directCleanupCalls = 0;
    await assert.rejects(
      runBoundedTool(
        fixture.shim,
        ["direct-cmd-must-not-run"],
        Date.now() + 5000,
        root,
        {
          captureIdentity: () => {
            directCaptureCalls += 1;
          },
          cleanupIdentity: () => {
            directCleanupCalls += 1;
          },
        },
      ),
      /playwright-cli-spawn-(?:EINVAL-before-pid|without-pid)/u,
    );
    assert.equal(
      readdirSync(root).some((name) =>
        name.startsWith("tool-cleanup-quarantine-"),
      ),
      false,
    );
    assert.equal(directCaptureCalls, 0);
    assert.equal(directCleanupCalls, 0);

    const transport = resolvePlaywrightTransport(fixture.shim);
    assert.deepEqual(Object.keys(transport.packages), [
      "cli",
      "playwright",
      "playwrightCore",
    ]);
    for (const packageIdentity of Object.values(transport.packages)) {
      assert.ok(packageIdentity.fileCount >= 2);
      assert.ok(packageIdentity.bytes > 0);
      assert.match(packageIdentity.treeSha256, /^[a-f0-9]{64}$/u);
    }
    const expected = [
      "space value",
      "&|<>^%!",
      "double \"quote\" and single 'quote'",
    ];
    const sentinel = path.join(root, "controlled-env-sentinel.txt");
    const preload = path.join(root, "benign-preload.cjs");
    writeFileSync(
      preload,
      `require("node:fs").writeFileSync(${JSON.stringify(sentinel)}, "loaded");\n`,
    );
    for (const name of environmentKeys) process.env[name] = preload;
    let capturedIdentity;
    let spawned;
    const output = await runPlaywrightTool(
      transport,
      expected,
      Date.now() + 5000,
      root,
      {
        spawn: (executable, args, options) => {
          spawned = { executable, args, options };
          return spawn(executable, args, options);
        },
        captureIdentity: (pid) => {
          capturedIdentity = processIdentityHelper("capture", pid);
          return capturedIdentity;
        },
      },
    );
    assert.equal(spawned.executable, process.execPath);
    assert.deepEqual(spawned.args, [fixture.entry, ...expected]);
    assert.equal(spawned.options.shell, false);
    assert.ok(spawned.options.env);
    assert.equal(
      Object.keys(spawned.options.env).some((name) =>
        /^(?:NODE_|PLAYWRIGHT_|PW_)/iu.test(name),
      ),
      false,
    );
    assert.equal(spawned.options.env.SYSTEMROOT, process.env.SystemRoot);
    assert.deepEqual(JSON.parse(output), expected);
    assert.equal(existsSync(sentinel), false);
    assert.equal(capturedIdentity.executablePath, process.execPath);
    const cleanup = processIdentityHelper("cleanup", capturedIdentity);
    assert.equal(cleanup.identityLost, false);
    assert.deepEqual(cleanup.remaining, []);
  } finally {
    for (const [name, value] of previousEnvironment) {
      if (value === undefined) delete process.env[name];
      else process.env[name] = value;
    }
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows Playwright launch rejects mixed-case Node preload configuration before spawn", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-playwright-env-red-"));
  const poisonKey = "NoDe_OpTiOnS";
  const previous = process.env[poisonKey];
  try {
    const fixture = writeNpmPlaywrightFixture(path.join(root, "graph"));
    const transport = resolvePlaywrightTransport(fixture.shim);
    const sentinel = path.join(root, "preload-sentinel.txt");
    const preload = path.join(root, "valid-preload.cjs");
    writeFileSync(
      preload,
      `require("node:fs").writeFileSync(${JSON.stringify(sentinel)}, "loaded");\n`,
    );
    process.env[poisonKey] = `--require=${preload}`;
    let spawnCalls = 0;
    await assert.rejects(
      async () =>
        runPlaywrightTool(
          transport,
          ["must-not-launch"],
          Date.now() + 5000,
          root,
          {
            spawn: () => {
              spawnCalls += 1;
              throw Object.assign(new Error("unexpected spawn"), {
                code: "UNEXPECTED",
              });
            },
          },
        ),
      /journey-tool-environment-invalid/u,
    );
    assert.equal(spawnCalls, 0);
    assert.equal(existsSync(sentinel), false);
  } finally {
    if (previous === undefined) delete process.env[poisonKey];
    else process.env[poisonKey] = previous;
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows npm Playwright transport revalidates an imported CLI sibling", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-playwright-sibling-"));
  try {
    const fixture = writeNpmPlaywrightFixture(path.join(root, "graph"));
    const admitted = resolvePlaywrightTransport(fixture.shim);
    writeFileSync(fixture.skillCheck, "export const admitted = false;\n");
    assert.throws(
      () => validatePlaywrightTransport(admitted),
      /journey-playwright-transport-mutated/u,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows npm Playwright transport revalidates playwright-core runtime files", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-playwright-core-"));
  try {
    const fixture = writeNpmPlaywrightFixture(path.join(root, "graph"));
    const admitted = resolvePlaywrightTransport(fixture.shim);
    writeFileSync(
      fixture.coreRuntime,
      "module.exports = { tampered: true };\n",
    );
    assert.throws(
      () => validatePlaywrightTransport(admitted),
      /journey-playwright-transport-mutated/u,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows npm Playwright transport rejects post-admission add, delete, and reparse changes", () => {
  const changes = [
    {
      name: "empty-directory",
      mutate: ({ packageRoot }) =>
        mkdirSync(path.join(packageRoot, "empty-added")),
    },
    {
      name: "add",
      mutate: ({ packageRoot }) =>
        writeFileSync(path.join(packageRoot, "added.js"), "export {};\n"),
    },
    {
      name: "delete",
      mutate: ({ skillCheck }) => unlinkSync(skillCheck),
    },
    {
      name: "reparse",
      mutate: ({ playwrightRoot }) => {
        const external = `${playwrightRoot}-outside`;
        renameSync(playwrightRoot, external);
        symlinkSync(external, playwrightRoot, "junction");
      },
    },
  ];
  for (const change of changes) {
    const root = mkdtempSync(
      path.join(tmpdir(), `sage-playwright-${change.name}-`),
    );
    try {
      const fixture = writeNpmPlaywrightFixture(path.join(root, "graph"));
      const admitted = resolvePlaywrightTransport(fixture.shim);
      change.mutate(fixture);
      assert.throws(
        () => validatePlaywrightTransport(admitted),
        /journey-playwright-transport-(?:invalid|mutated)/u,
        change.name,
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("Windows npm Playwright transport rejects an in-flight package-tree addition", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-playwright-in-flight-"));
  try {
    const fixture = writeNpmPlaywrightFixture(path.join(root, "graph"));
    let added = false;
    assert.throws(
      () =>
        resolvePlaywrightTransport(fixture.shim, {
          afterRead: ({ packageName, relativePath }) => {
            if (
              packageName === "playwright-core" &&
              relativePath === "package.json" &&
              !added
            ) {
              added = true;
              writeFileSync(
                path.join(fixture.packageRoot, "in-flight.js"),
                "export {};\n",
              );
            }
          },
        }),
      /journey-playwright-transport-mutated/u,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows npm Playwright transport rejects depth, entry-count, and relative-path budget overflow", () => {
  const limits = [
    ["depth", { maxDepth: 1 }],
    ["entries", { maxEntries: 2 }],
    ["relative-path", { maxRelativePathBytes: 8 }],
    ["total-relative-paths", { maxTotalRelativePathBytes: 16 }],
  ];
  for (const [name, treeLimits] of limits) {
    const root = mkdtempSync(path.join(tmpdir(), `sage-playwright-${name}-`));
    try {
      const fixture = writeNpmPlaywrightFixture(path.join(root, "graph"));
      if (name === "depth")
        mkdirSync(path.join(fixture.packageRoot, "deep", "deeper"), {
          recursive: true,
        });
      assert.throws(
        () => resolvePlaywrightTransport(fixture.shim, { limits: treeLimits }),
        /journey-playwright-transport-invalid/u,
        name,
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("Windows npm Playwright transport stops an oversized directory at budget plus one reads", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-playwright-iterator-"));
  try {
    const fixture = writeNpmPlaywrightFixture(path.join(root, "graph"));
    let reads = 0;
    let closes = 0;
    assert.throws(
      () =>
        resolvePlaywrightTransport(fixture.shim, {
          limits: { maxEntries: 3 },
          openDirectory: () => ({
            readSync: () => {
              reads += 1;
              return { name: "package.json" };
            },
            closeSync: () => {
              closes += 1;
            },
          }),
        }),
      /journey-playwright-transport-invalid/u,
    );
    assert.ok(reads <= 4, `directory read ${reads} entries`);
    assert.equal(reads, 4);
    assert.equal(closes, 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows npm Playwright transport rejects graph tampering, escape, reparse, oversize, and mutation", () => {
  const cases = [
    {
      name: "shim",
      mutate: ({ shim }) => writeFileSync(shim, "@echo off\r\nnode %*\r\n"),
      error: /journey-playwright-transport-invalid/u,
    },
    {
      name: "package-name",
      mutate: ({ manifest }) =>
        mutateJsonFile(manifest, (value) => ({
          ...value,
          name: "not-playwright",
        })),
      error: /journey-playwright-transport-invalid/u,
    },
    {
      name: "package-bin",
      mutate: ({ manifest }) =>
        mutateJsonFile(manifest, (value) => ({
          ...value,
          bin: { "playwright-cli": "../escape.js" },
        })),
      error: /journey-playwright-transport-invalid/u,
    },
    {
      name: "cli-dependency-version",
      mutate: ({ manifest }) =>
        mutateJsonFile(manifest, (value) => ({
          ...value,
          dependencies: { ...value.dependencies, playwright: "9.9.9" },
        })),
      error: /journey-playwright-transport-invalid/u,
    },
    {
      name: "core-dependency-version",
      mutate: ({ playwrightManifest }) =>
        mutateJsonFile(playwrightManifest, (value) => ({
          ...value,
          dependencies: { "playwright-core": "9.9.9" },
        })),
      error: /journey-playwright-transport-invalid/u,
    },
    {
      name: "unbound-windows-dependency",
      mutate: ({ manifest }) =>
        mutateJsonFile(manifest, (value) => ({
          ...value,
          dependencies: { ...value.dependencies, unbound: "1.0.0" },
        })),
      error: /journey-playwright-transport-invalid/u,
    },
    {
      name: "bundled-windows-dependency",
      mutate: ({ manifest }) =>
        mutateJsonFile(manifest, (value) => ({
          ...value,
          bundledDependencies: ["unbound"],
        })),
      error: /journey-playwright-transport-invalid/u,
    },
    {
      name: "entry-oversize",
      mutate: ({ entry }) => truncateSync(entry, 8 * 1024 * 1024 + 1),
      error: /journey-playwright-transport-invalid/u,
    },
    {
      name: "package-reparse",
      mutate: ({ entry }) => {
        const packageRoot = path.dirname(entry);
        const external = path.join(
          path.dirname(path.dirname(packageRoot)),
          "outside-cli",
        );
        renameSync(packageRoot, external);
        symlinkSync(external, packageRoot, "junction");
      },
      error: /journey-playwright-transport-invalid/u,
    },
  ];
  for (const current of cases) {
    const root = mkdtempSync(
      path.join(tmpdir(), `sage-playwright-${current.name}-`),
    );
    try {
      const fixture = writeNpmPlaywrightFixture(path.join(root, "graph"));
      current.mutate(fixture);
      assert.throws(
        () => resolvePlaywrightTransport(fixture.shim),
        current.error,
        current.name,
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }

  const root = mkdtempSync(path.join(tmpdir(), "sage-playwright-mutation-"));
  try {
    const fixture = writeNpmPlaywrightFixture(path.join(root, "graph"));
    let mutated = false;
    assert.throws(
      () =>
        resolvePlaywrightTransport(fixture.shim, {
          afterRead: ({ role }) => {
            if (role === "entry" && !mutated) {
              mutated = true;
              truncateSync(fixture.entry, 1);
            }
          },
        }),
      /journey-playwright-transport-mutated/u,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }

  const revalidationRoot = mkdtempSync(
    path.join(tmpdir(), "sage-playwright-revalidation-"),
  );
  try {
    const fixture = writeNpmPlaywrightFixture(
      path.join(revalidationRoot, "graph"),
    );
    const transport = resolvePlaywrightTransport(fixture.shim);
    writeFileSync(fixture.entry, "process.exit(9);\n");
    let spawnCalls = 0;
    assert.throws(
      () =>
        runPlaywrightTool(
          transport,
          ["must-not-spawn"],
          Date.now() + 5000,
          revalidationRoot,
          {
            spawn: () => {
              spawnCalls += 1;
              throw new Error("must not spawn");
            },
          },
        ),
      /journey-playwright-transport-mutated/u,
    );
    assert.equal(spawnCalls, 0);
  } finally {
    rmSync(revalidationRoot, { recursive: true, force: true });
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
  const source = `const {spawn}=require('node:child_process');const child=spawn(${JSON.stringify(powershell)},['-NoProfile','-Command','Start-Sleep -Seconds 30']);child.once('spawn',()=>process.stdout.write('ready\\n'));setTimeout(()=>{},30000);`;
  const owned = spawn(process.execPath, ["-e", source], {
    windowsHide: true,
    stdio: ["ignore", "pipe", "ignore"],
  });
  try {
    await waitForText(owned.stdout, "ready", 5000);
    const identity = processIdentityHelper("capture", owned.pid);
    assert.ok(identity.remembered.length >= 2);
    const cleanup = processIdentityHelper("cleanup", identity);
    assert.equal(cleanup.identityLost, false);
    assert.deepEqual(cleanup.remaining, []);
  } finally {
    if (!owned.killed) owned.kill();
  }
});

test("Windows wrapper has no Playwright CLI input and rejects noncanonical SourceRoot before launch", () => {
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
        windowsHide: true,
        timeout: 5000,
      },
    );
  const before = processSnapshot();
  const wrapperSource = readFileSync(wrapper, "utf8");
  assert.doesNotMatch(wrapperSource, /SAGE_PLAYWRIGHT_CLI|--playwright-cli/u);
  assert.match(wrapperSource, /--node-bytes/u);
  assert.match(wrapperSource, /--node-sha256/u);
  assert.match(
    wrapperSource,
    /--remote-debugging-address=127\.0\.0\.1 --remote-debugging-port=\$port/u,
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

test("Windows wrapper admits a linked-worktree Git marker before Git validates it", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-linked-worktree-marker-"));
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
    writeFileSync(path.join(root, ".git"), "gitdir: C:\\missing-gitdir\n");
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
        path.join(root, "local-evidence", "linked-worktree-check"),
      ],
      { encoding: "utf8", windowsHide: true, timeout: 5000 },
    );
    const output = `${result.stdout}\n${result.stderr}`;
    assert.notEqual(result.status, 0);
    assert.doesNotMatch(output, /SourceRoot must be a Git checkout/u);
    assert.match(output, /git status .* failed/u);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows wrapper rejects a mixed-case Node preload before invoking the collector", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-wrapper-env-poison-"));
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
    const preloadSentinel = path.join(root, "preload-sentinel.txt");
    const collectorSentinel = path.join(root, "collector-sentinel.txt");
    const preload = path.join(root, "preload.cjs");
    const collector = path.join(root, "collector.mjs");
    writeFileSync(
      preload,
      `require("node:fs").writeFileSync(${JSON.stringify(preloadSentinel)}, "loaded");\n`,
    );
    writeFileSync(
      collector,
      `import {writeFileSync} from "node:fs";writeFileSync(${JSON.stringify(collectorSentinel)},"ran");\n`,
    );
    const quote = (value) => `'${value.replaceAll("'", "''")}'`;
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Invoke-ControlledNodeCollector'},$true);if($null-eq$fn){throw 'collector environment helper missing'};Invoke-Expression $fn.Extent.Text;$failure=$null;try{Invoke-ControlledNodeCollector ${quote(process.execPath)} @(${quote(collector)})}catch{$failure=$_.Exception.Message};[ordered]@{failure=$failure;restored=[Environment]::GetEnvironmentVariable('NODE_OPTIONS','Process');preload=(Test-Path -LiteralPath ${quote(preloadSentinel)});collector=(Test-Path -LiteralPath ${quote(collectorSentinel)})}|ConvertTo-Json -Compress`;
    const poison = `--require=${preload}`;
    const result = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-EncodedCommand",
        Buffer.from(source, "utf16le").toString("base64"),
      ],
      {
        encoding: "utf8",
        windowsHide: true,
        timeout: 10000,
        env: { ...process.env, NoDe_OpTiOnS: poison },
      },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.deepEqual(JSON.parse(result.stdout.trim()), {
      failure: "Collector code-loading environment is not allowed.",
      restored: poison,
      preload: false,
      collector: false,
    });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows wrapper excludes benign Node and Playwright variables and restores them after collector failure", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-wrapper-env-restore-"));
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
    const observation = path.join(root, "collector-environment.json");
    const collector = path.join(root, "collector.mjs");
    writeFileSync(
      collector,
      `import {writeFileSync} from "node:fs";const excluded=Object.fromEntries(Object.entries(process.env).filter(([name])=>/^(?:NODE_|PLAYWRIGHT_|PW_)/iu.test(name)));writeFileSync(${JSON.stringify(observation)},JSON.stringify({excluded,sage:process.env.SAGE_PROJECT_ROOT??null}));process.exit(7);\n`,
    );
    const quote = (value) => `'${value.replaceAll("'", "''")}'`;
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Invoke-ControlledNodeCollector'},$true);if($null-eq$fn){throw 'collector environment helper missing'};Invoke-Expression $fn.Extent.Text;$failure=$null;try{Invoke-ControlledNodeCollector ${quote(process.execPath)} @(${quote(collector)})}catch{$failure=$_.Exception.Message};[ordered]@{failure=$failure;node=[Environment]::GetEnvironmentVariable('NODE_BENIGN_VALUE','Process');playwright=[Environment]::GetEnvironmentVariable('PLAYWRIGHT_BENIGN_VALUE','Process');pw=[Environment]::GetEnvironmentVariable('PW_BENIGN_VALUE','Process');sage=[Environment]::GetEnvironmentVariable('SAGE_PROJECT_ROOT','Process')}|ConvertTo-Json -Compress`;
    const result = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-EncodedCommand",
        Buffer.from(source, "utf16le").toString("base64"),
      ],
      {
        encoding: "utf8",
        windowsHide: true,
        timeout: 10000,
        env: {
          ...process.env,
          NoDe_BeNiGn_VaLuE: "node-value",
          PlAyWrIgHt_BeNiGn_VaLuE: "playwright-value",
          pW_BeNiGn_VaLuE: "pw-value",
          SAGE_PROJECT_ROOT: "sage-project-root",
        },
      },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.deepEqual(JSON.parse(readFileSync(observation, "utf8")), {
      excluded: {},
      sage: "sage-project-root",
    });
    assert.deepEqual(JSON.parse(result.stdout.trim()), {
      failure:
        "Native IDE journey runner failed; inspect machine-failure.json under EvidenceRoot.",
      node: "node-value",
      playwright: "playwright-value",
      pw: "pw-value",
      sage: "sage-project-root",
    });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
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
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;Import-Module ${quote(identityModule)} -Force;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @('Get-CleanupRootTuple','New-OwnedRuntime','Write-LaunchIdentityQuarantine','Stop-UnadmittedOwnedRuntime','Write-RuntimeCleanupQuarantine','Capture-OwnedRuntimeOrQuarantine')){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};$state=[ordered]@{attempts=0;elapsed=0;delays=0;cleanups=0};$resolve={param($candidate,$expected)$state.attempts++;if($state.attempts -eq 1){throw 'injected unavailable'};Get-FootprintLaunchIdentity $candidate $expected};$delay={param($milliseconds)$state.delays++;$state.elapsed+=$milliseconds};$elapsed={return [long]$state.elapsed};$cleanup={param($candidate,$expected,$role,$evidence)$state.cleanups++;return [ordered]@{terminalZero=$true}};$process=Start-Process -FilePath ${quote(powershell)} -ArgumentList @('-NoProfile','-Command','Wait-Event -Timeout 30') -WindowStyle Hidden -PassThru;try{$runtime=$null;$captureError=$null;try{$runtime=Capture-OwnedRuntimeOrQuarantine $process ${quote(powershell)} 'sage' ${quote(root)} 500 $resolve $delay $elapsed $cleanup}catch{$captureError=$_.Exception.Message};$processId=$null;$identityPath=$null;$creationDate=$null;if($null -ne $runtime){$processId=$runtime.identity.processId;$identityPath=$runtime.identity.executablePath;$creationDate=$runtime.identity.creationDate};$quarantine=Test-Path -LiteralPath (Join-Path ${quote(root)} 'runtime-launch-quarantine-sage.json');[ordered]@{captureError=$captureError;attempts=$state.attempts;elapsed=$state.elapsed;delays=$state.delays;cleanups=$state.cleanups;quarantine=$quarantine;processId=$processId;identityPath=$identityPath;creationDate=$creationDate}|ConvertTo-Json -Compress}finally{$process.Refresh();if(-not $process.HasExited){$process.Kill()};[void]$process.WaitForExit(5000)}`;
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
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;Import-Module ${quote(identityModule)} -Force;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @('Get-CleanupRootTuple','New-OwnedRuntime','Write-LaunchIdentityQuarantine','Stop-UnadmittedOwnedRuntime','Write-RuntimeCleanupQuarantine','Capture-OwnedRuntimeOrQuarantine')){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};$state=[ordered]@{attempts=0;now=70;delays=0};$resolve={param($candidate,$expected)$state.attempts++;throw 'injected unavailable'};$delay={param($milliseconds)$state.delays++;$state.now+=$milliseconds};$clock={return [long]$state.now};$ambient=Start-Process -FilePath ${quote(powershell)} -ArgumentList @('-NoProfile','-Command','Wait-Event -Timeout 30') -WindowStyle Hidden -PassThru;$owned=Start-Process -FilePath ${quote(process.execPath)} -ArgumentList @(${quote(fixture)},${quote(childPidFile)},${quote(powershell)}) -WindowStyle Hidden -PassThru;$childPid=$null;try{$ready=[Diagnostics.Stopwatch]::StartNew();while(-not (Test-Path -LiteralPath ${quote(childPidFile)}) -and $ready.ElapsedMilliseconds -lt 5000){Start-Sleep -Milliseconds 25};if(-not (Test-Path -LiteralPath ${quote(childPidFile)})){throw 'owned child did not start'};$childPid=[int](Get-Content -LiteralPath ${quote(childPidFile)} -Raw);[void](New-Item -ItemType Directory -Path (Join-Path ${quote(root)} 'runtime-launch-quarantine-sage.json'));$captureError=$null;try{[void](Capture-OwnedRuntimeOrQuarantine $owned ${quote(process.execPath)} 'sage' ${quote(root)} 85 $resolve $delay $clock)}catch{$captureError=$_.Exception.Message};$owned.Refresh();$ambient.Refresh();[ordered]@{captureError=$captureError;attempts=$state.attempts;now=$state.now;delays=$state.delays;ownedAlive=(-not $owned.HasExited);childAlive=($null -ne (Get-Process -Id $childPid -ErrorAction SilentlyContinue));ambientAlive=(-not $ambient.HasExited);launchQuarantineIsDirectory=(Test-Path -LiteralPath (Join-Path ${quote(root)} 'runtime-launch-quarantine-sage.json') -PathType Container);cleanupQuarantine=(Test-Path -LiteralPath (Join-Path ${quote(root)} 'runtime-cleanup-quarantine-sage.json'))}|ConvertTo-Json -Compress}finally{foreach($pidValue in @($childPid,$owned.Id,$ambient.Id)){if($pidValue){Stop-Process -Id $pidValue -Force -ErrorAction SilentlyContinue}}}`;
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
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;Import-Module ${quote(identityModule)} -Force;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @('Get-CleanupRootTuple','New-OwnedRuntime','Write-LaunchIdentityQuarantine','Stop-UnadmittedOwnedRuntime','Write-RuntimeCleanupQuarantine','Capture-OwnedRuntimeOrQuarantine')){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};$state=[ordered]@{resolves=0;delays=0;cleanups=0};$resolve={param($candidate,$expected)$state.resolves++;throw 'must not run'};$delay={param($milliseconds)$state.delays++};$elapsed={return [long]0};$cleanup={param($candidate,$expected,$role,$evidence)$state.cleanups++;$candidate.Refresh();if(-not $candidate.HasExited){$candidate.Kill()};[void]$candidate.WaitForExit(5000);return [ordered]@{terminalZero=$true}};$mismatch=Start-Process -FilePath ${quote(powershell)} -ArgumentList @('-NoProfile','-Command','Wait-Event -Timeout 30') -WindowStyle Hidden -PassThru;try{[void](Capture-OwnedRuntimeOrQuarantine $mismatch ${quote(wrapper)} 'sage' ${quote(root)} 500 $resolve $delay $elapsed $cleanup)}catch{};$mismatchRecord=Get-Content -LiteralPath (Join-Path ${quote(root)} 'runtime-launch-quarantine-sage.json') -Raw|ConvertFrom-Json;Remove-Item -LiteralPath (Join-Path ${quote(root)} 'runtime-launch-quarantine-sage.json');$exited=Start-Process -FilePath ${quote(powershell)} -ArgumentList @('-NoProfile','-Command','exit 0') -WindowStyle Hidden -PassThru;[void]$exited.WaitForExit(5000);try{[void](Capture-OwnedRuntimeOrQuarantine $exited ${quote(powershell)} 'sage' ${quote(root)} 500 $resolve $delay $elapsed $cleanup)}catch{};$exitRecord=Get-Content -LiteralPath (Join-Path ${quote(root)} 'runtime-launch-quarantine-sage.json') -Raw|ConvertFrom-Json;[ordered]@{resolves=$state.resolves;delays=$state.delays;cleanups=$state.cleanups;mismatch=$mismatchRecord.failureCategory;exited=$exitRecord.failureCategory;mismatchAlive=(-not $mismatch.HasExited);exitedAlive=(-not $exited.HasExited)}|ConvertTo-Json -Compress`;
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
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;Import-Module ${quote(identityModule)} -Force;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @('Get-CleanupRootTuple','New-OwnedRuntime','Write-LaunchIdentityQuarantine','Stop-UnadmittedOwnedRuntime','Write-RuntimeCleanupQuarantine','Capture-OwnedRuntimeOrQuarantine')){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};$state=[ordered]@{resolves=0;now=0;delays=0;cleanups=0};$mode='pid';$resolve={param($candidate,$expected)$state.resolves++;$identity=Get-FootprintLaunchIdentity $candidate $expected;if($mode -eq 'pid'){$identity.processId=[int]$identity.processId+1}else{$identity.creationDate='2000-01-01T00:00:00.000Z'};return $identity};$delay={param($milliseconds)$state.delays++;$state.now+=$milliseconds};$clock={return [long]$state.now};$cleanup={param($candidate,$expected,$role,$evidence)$state.cleanups++;$candidate.Refresh();if(-not $candidate.HasExited){$candidate.Kill()};[void]$candidate.WaitForExit(5000);return [ordered]@{terminalZero=$true}};$categories=@();foreach($modeValue in @('pid','creation')){$mode=$modeValue;$candidate=Start-Process -FilePath ${quote(powershell)} -ArgumentList @('-NoProfile','-Command','Wait-Event -Timeout 30') -WindowStyle Hidden -PassThru;try{try{[void](Capture-OwnedRuntimeOrQuarantine $candidate ${quote(powershell)} 'sage' ${quote(root)} 500 $resolve $delay $clock $cleanup)}catch{};$record=Get-Content -LiteralPath (Join-Path ${quote(root)} 'runtime-launch-quarantine-sage.json') -Raw|ConvertFrom-Json;$categories+=$record.failureCategory;Remove-Item -LiteralPath (Join-Path ${quote(root)} 'runtime-launch-quarantine-sage.json')}finally{$candidate.Refresh();if(-not $candidate.HasExited){$candidate.Kill()};[void]$candidate.WaitForExit(5000)}};[ordered]@{resolves=$state.resolves;delays=$state.delays;cleanups=$state.cleanups;categories=$categories}|ConvertTo-Json -Compress`;
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

test("Windows owned Vite fixture becomes ready and reaches terminal-zero cleanup", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-vite-runtime-"));
  const port = await availableLoopbackPort();
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
      "Get-CleanupRootTuple",
      "Resolve-CleanupFailureClass",
      "Write-RuntimeCleanupQuarantine",
      "Stop-OwnedRuntimeTree",
    ];
    // Each owned process's handle is opened before cleanup, so the exit wait
    // observes that exact process (never a reused PID) and returns on exit.
    // A process still alive at the deadline fails the test, and the finally
    // block still tears down whatever survived.
    const ownedTeardownMs = 5000;
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;Import-Module ${quote(identityModule)} -Force;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @(${functions.map(quote).join(",")})){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};$contract=[ordered]@{port=${port};origin='http://127.0.0.1:${port}/'};$runtime=$null;$cleanup=$null;$owned=@();$alive=0;$startupDeadline=[DateTime]::UtcNow.AddSeconds(12);$script:ownedViteClock=[Diagnostics.Stopwatch]::StartNew();$monotonic={return [long]$script:ownedViteClock.ElapsedMilliseconds};$deadlineMilliseconds=[long](&$monotonic)+12000;try{$runtime=Start-OwnedVite ${quote(root)} $contract ${quote(root)} $deadlineMilliseconds $monotonic;$ready=Wait-OwnedFrontendReady $runtime $contract $startupDeadline;$ownedIds=@($runtime.store.Values|ForEach-Object {[int]$_.processId}|Sort-Object -Unique);$owned=@($ownedIds|ForEach-Object {Get-Process -Id $_ -ErrorAction SilentlyContinue}|Where-Object {$null -ne $_.Handle});$cleanup=Stop-OwnedRuntimeTree 'vite' $runtime ${quote(root)};$alive=@($owned|Where-Object {-not $_.WaitForExit(${ownedTeardownMs})}).Count;[ordered]@{port=$contract.port;mode=$ready.mode;frontendReady=$ready.frontendReady;identityCount=$ownedIds.Count;identityLost=$cleanup.identityLost;remainingCount=$cleanup.remainingCount;alive=$alive}|ConvertTo-Json -Compress}finally{if($null -ne $runtime -and ($null -eq $cleanup -or $alive -gt 0)){try{[void](Stop-OwnedRuntimeTree 'vite' $runtime ${quote(root)})}catch{}};foreach($process in $owned){if(-not $process.HasExited){try{$process.Kill()}catch{}}}}`;
    const encoded = Buffer.from(source, "utf16le").toString("base64");
    const result = spawnSync(
      powershell,
      ["-NoProfile", "-NonInteractive", "-EncodedCommand", encoded],
      { encoding: "utf8", windowsHide: true, timeout: 25000 },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    const observed = JSON.parse(result.stdout.trim());
    assert.equal(observed.port, port);
    assert.equal(observed.mode, "tauri-dev-split");
    assert.equal(observed.frontendReady, true);
    assert.ok(observed.identityCount >= 2);
    assert.equal(observed.identityLost, false);
    assert.equal(observed.remainingCount, 0);
    assert.equal(observed.alive, 0);
    assertWindowsPortFree(port);
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
        "Get-CleanupRootTuple",
        "Resolve-CleanupFailureClass",
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
  const port = await availableLoopbackPort();
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
      `require("node:http").createServer((_q,r)=>r.end("ambient")).listen(${port},"127.0.0.1",()=>process.stdout.write("ready\\n"))`,
    ],
    { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] },
  );
  let occupiedIdentity;
  let cleanupFailure;
  try {
    await waitForText(occupied.stdout, "ready", 5000);
    occupiedIdentity = processIdentityHelper("capture", occupied.pid);
    const quote = (value) => `'${value.replaceAll("'", "''")}'`;
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @('Assert-FrontendPortAvailable')){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};Assert-FrontendPortAvailable ([ordered]@{port=${port};origin='http://127.0.0.1:${port}/'})`;
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
      assertWindowsPortFree(port);
    } catch (error) {
      failures.push(error);
    }
    if (failures.length)
      cleanupFailure = new AggregateError(failures, "fixture cleanup failed");
  }
  if (cleanupFailure) throw cleanupFailure;
});

test("Windows frontend readiness rejects an ambient listener race outside the owned tree", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-vite-ambient-race-"));
  const port = await availableLoopbackPort();
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
      `require("node:http").createServer((_q,r)=>r.end("ambient")).listen(${port},"127.0.0.1",()=>process.stdout.write("ready\\n"))`,
    ],
    { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] },
  );
  let ambientIdentity;
  let cleanupFailure;
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
      "Get-CleanupRootTuple",
      "Resolve-CleanupFailureClass",
      "Write-RuntimeCleanupQuarantine",
      "Stop-OwnedRuntimeTree",
    ];
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;Import-Module ${quote(identityModule)} -Force;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @(${names.map(quote).join(",")})){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};$contract=[ordered]@{port=${port};origin='http://127.0.0.1:${port}/'};$process=Start-Process -FilePath ${quote(powershell)} -ArgumentList @('-NoProfile','-Command','Start-Sleep -Seconds 30') -WindowStyle Hidden -PassThru;$identity=Get-FootprintLaunchIdentity $process ${quote(powershell)};$runtime=New-OwnedRuntime $process ${quote(powershell)} $identity;try{$rejected=$false;try{[void](Wait-OwnedFrontendReady $runtime $contract ([DateTime]::UtcNow.AddSeconds(5)))}catch{$rejected=$_.Exception.Message -match 'not owned'};$cleanup=Stop-OwnedRuntimeTree 'vite' $runtime ${quote(root)};[ordered]@{rejected=$rejected;identityLost=$cleanup.identityLost;remainingCount=$cleanup.remainingCount}|ConvertTo-Json -Compress}finally{if($null -ne (Get-Process -Id $process.Id -ErrorAction SilentlyContinue)){Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue}}`;
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
      assertWindowsPortFree(port);
    } catch (error) {
      failures.push(error);
    }
    rmSync(root, { recursive: true, force: true });
    if (failures.length)
      cleanupFailure = new AggregateError(failures, "fixture cleanup failed");
  }
  if (cleanupFailure) throw cleanupFailure;
});

test("Windows Vite cleanup quarantines post-teardown identity loss with the exact root tuple", () => {
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
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;Import-Module ${quote(identityModule)} -Force;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @('Get-CleanupRootTuple','Resolve-CleanupFailureClass','Update-OwnedRuntimeTree','Write-RuntimeCleanupQuarantine','Stop-OwnedRuntimeTree')){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};$process=Start-Process -FilePath ${quote(powershell)} -ArgumentList @('-NoProfile','-Command','Start-Sleep -Seconds 30') -WindowStyle Hidden -PassThru;try{$actual=Get-FootprintLaunchIdentity $process ${quote(powershell)};$forged=[ordered]@{processId=$actual.processId;executablePath=$actual.executablePath;creationDate='1900-01-01T00:00:00.000Z'};$runtime=[ordered]@{process=$process;identity=$forged;store=(New-FootprintIdentityStore)};$failed=$false;try{[void](Stop-OwnedRuntimeTree 'vite' $runtime ${quote(root)})}catch{$failed=$true};$alive=$null -ne (Get-Process -Id $process.Id -ErrorAction SilentlyContinue);$record=Get-Content -LiteralPath (Join-Path ${quote(root)} 'runtime-cleanup-quarantine-vite.json') -Raw|ConvertFrom-Json;[ordered]@{failed=$failed;alive=$alive;record=$record;expectedRoot=$forged}|ConvertTo-Json -Depth 4 -Compress}finally{Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue;[void]$process.WaitForExit(5000)}`;
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
    assert.equal(observed.record.schemaVersion, 2);
    assert.equal(observed.record.runtimeRole, "vite");
    assert.equal(observed.record.category, "cleanup-unproven");
    assert.equal(observed.record.stage, "post-teardown");
    assert.equal(observed.record.failureClass, "terminal-zero-invalid");
    assert.equal(observed.record.identityLost, true);
    assert.equal(observed.record.remainingCount, 0);
    assert.deepEqual(observed.record.root, observed.expectedRoot);
    const diagnostic = JSON.parse(
      result.stderr.split("[runtime-teardown] ")[1].split(/\r?\n/)[0],
    );
    assert.equal(diagnostic.role, "vite");
    assert.equal(diagnostic.identityLost, true);
    assert.deepEqual(diagnostic.remainingProcessIds, []);
    assert.deepEqual(diagnostic.reusedProcessIds, [
      observed.expectedRoot.processId,
    ]);
    assert.equal(diagnostic.truncated, false);
    assert.deepEqual(diagnostic.tracked, [
      { ...observed.expectedRoot, pathTruncated: false, outcome: "lost" },
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows Vite cleanup records an injected stop failure and blocks receipt promotion", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-vite-stop-failure-"));
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
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;Import-Module ${quote(identityModule)} -Force;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @('Get-CleanupRootTuple','Resolve-CleanupFailureClass','Update-OwnedRuntimeTree','Write-RuntimeCleanupQuarantine','Stop-OwnedRuntimeTree','Publish-PendingJourneyReceipt')){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};function Stop-FootprintTree {param($Identity,$Store,$Grace)throw [UnauthorizedAccessException]::new('injected cleanup stop failure')};$process=Start-Process -FilePath ${quote(powershell)} -ArgumentList @('-NoProfile','-Command','Start-Sleep -Seconds 30') -WindowStyle Hidden -PassThru;try{$identity=Get-FootprintLaunchIdentity $process ${quote(powershell)};$runtime=[ordered]@{process=$process;identity=$identity;store=(New-FootprintIdentityStore)};$pending=Join-Path ${quote(root)} 'machine-receipt.pending.json';$final=Join-Path ${quote(root)} 'machine-receipt.json';Set-Content -LiteralPath $pending -Value '{}' -NoNewline -Encoding utf8;$failed=$false;try{[void](Stop-OwnedRuntimeTree 'vite' $runtime ${quote(root)})}catch{$failed=$_.Exception.Message -eq 'The vite runtime cleanup could not be proven.'};$published=$true;try{Publish-PendingJourneyReceipt $pending $final $null ([ordered]@{identityLost=$false;remainingCount=0})}catch{$published=$false};$recordText=Get-Content -LiteralPath (Join-Path ${quote(root)} 'runtime-cleanup-quarantine-vite.json') -Raw;$record=$recordText|ConvertFrom-Json;$expectedRoot=[ordered]@{processId=$identity.processId;executablePath=$identity.executablePath;creationDate=$identity.creationDate};[ordered]@{failed=$failed;published=$published;pending=(Test-Path -LiteralPath $pending -PathType Leaf);final=(Test-Path -LiteralPath $final -PathType Leaf);alive=($null -ne (Get-Process -Id $process.Id -ErrorAction SilentlyContinue));record=$record;recordLeakedInjectedMessage=$recordText.Contains('injected cleanup stop failure');expectedRoot=$expectedRoot}|ConvertTo-Json -Depth 4 -Compress}finally{Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue;[void]$process.WaitForExit(5000)}`;
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
    assert.equal(observed.failed, true);
    assert.equal(observed.published, false);
    assert.equal(observed.pending, true);
    assert.equal(observed.final, false);
    assert.equal(observed.alive, true);
    assert.equal(observed.recordLeakedInjectedMessage, false);
    const diagnostic = JSON.parse(
      result.stderr.split("[runtime-teardown] ")[1].split(/\r?\n/)[0],
    );
    assert.equal(diagnostic.stage, "stop-tree");
    assert.equal(diagnostic.identityLost, null);
    assert.deepEqual(
      diagnostic.tracked.find(
        (entry) => entry.processId === observed.expectedRoot.processId,
      ),
      { ...observed.expectedRoot, pathTruncated: false, outcome: "unknown" },
    );
    assert.ok(diagnostic.tracked.every((entry) => entry.outcome === "unknown"));
    assert.deepEqual(observed.record, {
      schemaVersion: 2,
      runtimeRole: "vite",
      category: "cleanup-unproven",
      stage: "stop-tree",
      failureClass: "stop-access-denied",
      root: observed.expectedRoot,
      identityLost: null,
      remainingCount: null,
    });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows cleanup failure reports the exact surviving identity in the failure log", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-vite-survivor-"));
  const powershell = path.join(
    process.env.SystemRoot,
    "System32/WindowsPowerShell/v1.0/powershell.exe",
  );
  const wrapper = path.resolve(
    "scripts/run-source-functional-native-ide-journey.ps1",
  );
  const identityModule = path.resolve("scripts/footprint/ProcessIdentity.psm1");
  const quote = (value) => `'${value.replaceAll("'", "''")}'`;
  // Inject a failed stop result for a real live process. The wrapper must keep
  // failing closed and report that exact survivor without inventing a new probe.
  const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;Import-Module ${quote(identityModule)} -Force;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);foreach($name in @('Get-CleanupRootTuple','Resolve-CleanupFailureClass','Update-OwnedRuntimeTree','Write-RuntimeCleanupQuarantine','Stop-OwnedRuntimeTree')){$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true);Invoke-Expression $fn.Extent.Text};function Stop-FootprintTree {param($Identity,$Store,$Grace)return [ordered]@{identityLost=$false;remaining=@($Identity.processId);reusedProcessIds=@()}};$process=Start-Process -FilePath ${quote(powershell)} -ArgumentList @('-NoProfile','-Command','Start-Sleep -Seconds 30') -WindowStyle Hidden -PassThru;try{$identity=Get-FootprintLaunchIdentity $process ${quote(powershell)};$runtime=[ordered]@{process=$process;identity=$identity;store=(New-FootprintIdentityStore)};$message='';try{[void](Stop-OwnedRuntimeTree 'vite' $runtime ${quote(root)})}catch{$message=$_.Exception.Message};[ordered]@{message=$message;alive=($null -ne (Get-Process -Id $process.Id -ErrorAction SilentlyContinue));identity=(Get-CleanupRootTuple $identity)}|ConvertTo-Json -Depth 4 -Compress}finally{Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue;[void]$process.WaitForExit(5000)}`;
  try {
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
    assert.equal(
      observed.message,
      "The vite runtime cleanup was not terminal-zero.",
    );
    assert.equal(observed.alive, true);
    const diagnostic = JSON.parse(
      result.stderr.split("[runtime-teardown] ")[1].split(/\r?\n/)[0],
    );
    assert.equal(diagnostic.identityLost, false);
    assert.deepEqual(diagnostic.remainingProcessIds, [
      observed.identity.processId,
    ]);
    assert.deepEqual(
      diagnostic.tracked.filter((entry) => entry.outcome === "survived"),
      [{ ...observed.identity, pathTruncated: false, outcome: "survived" }],
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows cleanup failure classes distinguish Win32 access denied from invalid handle", () => {
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
  const quote = (value) => `'${value.replaceAll("'", "''")}'`;
  const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);$fn=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Resolve-CleanupFailureClass'},$true);Invoke-Expression $fn.Extent.Text;$classes=@();foreach($code in @(5,6)){$exception=[ComponentModel.Win32Exception]::new($code);$record=[Management.Automation.ErrorRecord]::new($exception,'injected',[Management.Automation.ErrorCategory]::NotSpecified,$null);$classes+=Resolve-CleanupFailureClass $record};$classes|ConvertTo-Json -Compress`;
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
  assert.deepEqual(JSON.parse(result.stdout.trim()), [
    "stop-access-denied",
    "stop-unknown",
  ]);
});

test("Windows wrapper emits strict ACP-04 and AV01 collector vectors from actual branch variables", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-wrapper-argv-"));
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
    const source = `$ErrorActionPreference='Stop';Set-StrictMode -Version Latest;$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile(${quote(wrapper)},[ref]$tokens,[ref]$errors);$assignments=@($ast.FindAll({param($node) $node -is [Management.Automation.Language.AssignmentStatementAst] -and $node.Left.Extent.Text -eq '$arguments'},$true));$acp=@($assignments|Where-Object {$_.Extent.Text -match 'two-turn-cancel-restart'})|Select-Object -First 1;$av01=@($assignments|Where-Object {$_.Extent.Text -match '"--git-path"'})|Select-Object -First 1;$invoke=@($ast.FindAll({param($node) $node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -eq 'Invoke-ControlledNodeCollector'},$true)|Where-Object {$_.Extent.StartOffset -gt $av01.Extent.EndOffset})|Select-Object -First 1;if($null-eq$acp-or $null-eq$av01-or $null-eq$invoke){throw 'collector argv AST extraction failed'};$runner=Join-Path ${quote(root)} 'collector.mjs';$node=${quote(process.execPath)};$port=1431;$EvidenceRoot=[IO.Path]::GetFullPath(${quote(root)});$commit=('b'*40);$tree=('c'*40);$resolvedExecutable=${quote(powershell)};$resolvedSource=[IO.Path]::GetFullPath(${quote(root)});$preLaunchExecutableBytes=100;$preLaunchExecutableSha256=('a'*64);$preLaunchNodeBytes=100;$preLaunchNodeSha256=('a'*64);$av01PowerShell=${quote(powershell)};$av01PowerShellBytes=100;$av01PowerShellSha256=('a'*64);$codex=Join-Path $EvidenceRoot 'codex.cmd';$codexBytes=100;$codexSha256=('a'*64);$gitExecutable=Join-Path $EvidenceRoot 'git.exe';$gitBytes=100;$gitSha256=('a'*64);$profile=Join-Path $EvidenceRoot 'profile';$profileIdentityArtifact=Join-Path $EvidenceRoot 'acp04-profile-identity.json';$sageStdoutLog=Join-Path $EvidenceRoot 'sage.stdout.log';$sageStderrLog=Join-Path $EvidenceRoot 'sage.stderr.log';$sageRuntime=[ordered]@{identity=[ordered]@{processId=4100;creationDate='2026-09-08T12:00:00.000Z'}};$frontendContract=[ordered]@{origin='http://127.0.0.1:1431/'};$runtimeReceipt=[ordered]@{mode='tauri-dev-split';frontendReady=$true};$pendingReceipt=Join-Path $EvidenceRoot 'machine-receipt.pending.json';$DeadlineMs=90000;$fixture=Join-Path $EvidenceRoot 'fixture-project';$fixtureProcessHelper=Join-Path $EvidenceRoot 'source-functional-av01-preview-process.ps1';$PreviewCase='starting-active';$captured=[ordered]@{};function Invoke-ControlledNodeCollector {param($NodePath,$CollectorArguments,$Kind)$script:captured[$Kind]=@($CollectorArguments)};$JourneyKind='acp04-live';Invoke-Expression $acp.Extent.Text;Invoke-Expression $invoke.Extent.Text;$JourneyKind='av01-preview';Invoke-Expression $av01.Extent.Text;Invoke-Expression $invoke.Extent.Text;$captured|ConvertTo-Json -Depth 4 -Compress`;
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
    const vectors = JSON.parse(result.stdout.trim());
    const runner = path.join(root, "collector.mjs");
    assert.equal(vectors["acp04-live"][0], runner);
    assert.equal(vectors["av01-preview"][0], runner);
    assert.doesNotThrow(() =>
      parseAcp04ProbeArguments(vectors["acp04-live"].slice(1)),
    );
    assert.doesNotThrow(() =>
      parseAv01PreviewArguments(vectors["av01-preview"].slice(1)),
    );
    const gitFlags = ["--git-path", "--git-bytes", "--git-sha256"];
    assert.deepEqual(
      vectors["acp04-live"].filter((entry) => gitFlags.includes(entry)),
      [],
    );
    assert.deepEqual(
      vectors["av01-preview"].filter((entry) => gitFlags.includes(entry)),
      gitFlags,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("normal wrapper passes clean Git status with a canonical helper fixture and fails safely before evidence or launch", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-clean-preflight-"));
  const executable = path.join(root, "sage.cmd");
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
  // Observe only processes owned by this fixture, and fail closed if the
  // process snapshot itself fails. Other lanes may start or stop Sage.
  const ownedProcesses = () => {
    const result = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        `$ErrorActionPreference='Stop'; @(Get-CimInstance Win32_Process|Where-Object { $_.ProcessId -ne $PID -and $_.CommandLine -and $_.CommandLine.Contains('${root.replaceAll("'", "''")}') }|ForEach-Object { "$($_.Name):$($_.ProcessId)" }|Sort-Object)-join ','`,
      ],
      { encoding: "utf8", windowsHide: true, timeout: 60_000 },
    );
    assert.equal(result.status, 0, `${result.error ?? ""}\n${result.stderr}`);
    return result.stdout.trim();
  };
  try {
    writeFileSync(
      executable,
      '@echo off\r\n> "%~dp0launched.txt" echo launched\r\nexit /b 71\r\n',
    );
    mkdirSync(path.join(root, "src-tauri"));
    writeFileSync(
      path.join(root, "src-tauri", "tauri.conf.json"),
      JSON.stringify({ build: { devUrl: "http://127.0.0.1:1420" } }),
    );
    writeFileSync(
      path.join(root, "vite.config.ts"),
      "export default {\n  server: { port: 1420, strictPort: true, },\n};\n",
    );
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
    const before = ownedProcesses();
    const result = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-File",
        wrapper,
        "-SageExecutable",
        executable,
        "-SourceRoot",
        root,
        "-EvidenceRoot",
        "relative-evidence",
      ],
      {
        encoding: "utf8",
        windowsHide: true,
        timeout: 10000,
      },
    );
    assert.notEqual(result.status, 0);
    assert.match(
      `${result.stdout}\n${result.stderr}`,
      /EvidenceRoot must be an absolute canonical path/,
    );
    const after = ownedProcesses();
    assert.equal(before, "");
    assert.equal(after, before);
    assert.equal(existsSync(path.join(root, "launched.txt")), false);
    assert.equal(existsSync(path.resolve("relative-evidence")), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("Windows wrapper startup clock remains absolute under validated root parameters", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-wrapper-clock-"));
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
    const wrapperLines = readFileSync(wrapper, "utf8").split(/\r?\n/u);
    const clockStart = wrapperLines.findIndex((line) =>
      line.includes("startupClock ="),
    );
    assert.notEqual(clockStart, -1);
    const clockBlock = wrapperLines
      .slice(clockStart, clockStart + 3)
      .join("\r\n");
    assert.match(clockBlock, /\$startupDeadlineMilliseconds/u);
    const contract = path.join(root, "clock-contract.ps1");
    writeFileSync(
      contract,
      `[CmdletBinding()]\r\nparam([ValidateSet("capture", "cleanup")][string]$ToolIdentityAction)\r\n$ErrorActionPreference="Stop"\r\nSet-StrictMode -Version Latest\r\n$startupWindowMilliseconds=500\r\n${clockBlock}\r\n$deadlineBefore=[long]$startupDeadlineMilliseconds\r\n$first=[long](& $monotonicMilliseconds)\r\n$remainingBefore=$deadlineBefore-$first\r\nStart-Sleep -Milliseconds 80\r\n$second=[long](& $monotonicMilliseconds)\r\n$deadlineAfter=[long]$startupDeadlineMilliseconds\r\n$remainingAfter=$deadlineAfter-$second\r\n[ordered]@{toolIdentityAction=$ToolIdentityAction;first=$first;second=$second;deadlineBefore=$deadlineBefore;deadlineAfter=$deadlineAfter;remainingBefore=$remainingBefore;remainingAfter=$remainingAfter}|ConvertTo-Json -Compress\r\n`,
    );
    const result = spawnSync(
      powershell,
      ["-NoProfile", "-NonInteractive", "-File", contract],
      { encoding: "utf8", windowsHide: true, timeout: 5000 },
    );
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    const observed = JSON.parse(result.stdout.trim());
    assert.equal(observed.toolIdentityAction, "");
    assert.ok(observed.first >= 0, JSON.stringify(observed));
    assert.ok(observed.second - observed.first >= 40, JSON.stringify(observed));
    assert.equal(observed.deadlineAfter, observed.deadlineBefore);
    assert.ok(
      observed.remainingAfter < observed.remainingBefore,
      JSON.stringify(observed),
    );
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

function writeNpmPlaywrightFixture(root) {
  const nodeModules = path.join(root, "node_modules");
  const bin = path.join(nodeModules, ".bin");
  const packageRoot = path.join(nodeModules, "@playwright", "cli");
  const playwrightRoot = path.join(nodeModules, "playwright");
  const coreRoot = path.join(nodeModules, "playwright-core");
  mkdirSync(bin, { recursive: true });
  mkdirSync(packageRoot, { recursive: true });
  mkdirSync(playwrightRoot, { recursive: true });
  mkdirSync(coreRoot, { recursive: true });
  const shim = path.join(bin, "playwright-cli.cmd");
  const manifest = path.join(packageRoot, "package.json");
  const entry = path.join(packageRoot, "playwright-cli.js");
  const skillCheck = path.join(packageRoot, "skillCheck.js");
  const playwrightManifest = path.join(playwrightRoot, "package.json");
  const playwrightRuntime = path.join(playwrightRoot, "index.js");
  const coreManifest = path.join(coreRoot, "package.json");
  const coreRuntime = path.join(coreRoot, "index.js");
  writeFileSync(
    shim,
    '@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\r\nIF EXIST "%dp0%\\node.exe" (\r\n  SET "_prog=%dp0%\\node.exe"\r\n) ELSE (\r\n  SET "_prog=node"\r\n  SET PATHEXT=%PATHEXT:;.JS;=;%\r\n)\r\n\r\nendLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & "%_prog%"  "%dp0%\\..\\@playwright\\cli\\playwright-cli.js" %*\r\n',
  );
  writeFileSync(
    manifest,
    `${JSON.stringify(
      {
        name: "@playwright/cli",
        version: "1.2.3",
        bin: { "playwright-cli": "playwright-cli.js" },
        dependencies: { playwright: "1.2.3", "playwright-core": "1.2.3" },
      },
      null,
      2,
    )}\n`,
  );
  writeFileSync(
    entry,
    "const preload=process.env.NODE_BENIGN_PRELOAD??process.env.PLAYWRIGHT_PRELOAD??process.env.PW_PRELOAD;if(preload)require(preload);process.stdout.write(JSON.stringify(process.argv.slice(2)));setTimeout(()=>{},2000);\n",
  );
  writeFileSync(skillCheck, "export const admitted = true;\n");
  writeFileSync(
    playwrightManifest,
    `${JSON.stringify(
      {
        name: "playwright",
        version: "1.2.3",
        dependencies: { "playwright-core": "1.2.3" },
        optionalDependencies: { fsevents: "2.3.2" },
      },
      null,
      2,
    )}\n`,
  );
  writeFileSync(playwrightRuntime, "module.exports = {};\n");
  writeFileSync(
    coreManifest,
    `${JSON.stringify({ name: "playwright-core", version: "1.2.3" }, null, 2)}\n`,
  );
  writeFileSync(coreRuntime, "module.exports = {};\n");
  return {
    nodeModules,
    packageRoot,
    playwrightRoot,
    coreRoot,
    shim,
    manifest,
    entry,
    skillCheck,
    playwrightManifest,
    playwrightRuntime,
    coreManifest,
    coreRuntime,
  };
}

function mutateJsonFile(file, mutate) {
  const value = JSON.parse(readFileSync(file, "utf8"));
  writeFileSync(file, `${JSON.stringify(mutate(value), null, 2)}\n`);
}

function playwrightTransportReceipt() {
  const identity = (name, digest) => ({
    path: `C:\\tools\\${name}`,
    bytes: 100 + digest.charCodeAt(0),
    sha256: digest.repeat(64),
  });
  return {
    node: identity("node.exe", "a"),
    shim: identity("playwright-cli.cmd", "b"),
    packages: {
      cli: packageTree("@playwright/cli", "cli", "c"),
      playwright: packageTree("playwright", "playwright", "d"),
      playwrightCore: packageTree("playwright-core", "playwright-core", "e"),
    },
  };
}

function rawCdpTransportReceipt() {
  return {
    kind: "node-websocket-cdp-v1",
    node: {
      path: process.execPath,
      bytes: 456,
      sha256: "e".repeat(64),
    },
    connectionCount: 1,
    maxInFlight: 1,
    requestCount: 41,
  };
}

function packageTree(name, directory, digest) {
  return {
    root: `C:\\tools\\node_modules\\${directory}`,
    name,
    version: "1.2.3",
    fileCount: 3,
    bytes: 300 + digest.charCodeAt(0),
    treeSha256: digest.repeat(64),
  };
}

function playwrightExecutionTransport() {
  return {
    ...playwrightTransportReceipt(),
    entryPath: "C:\\tools\\node_modules\\@playwright\\cli\\playwright-cli.js",
  };
}

function observed() {
  return {
    save: {
      editorPath: "journey.ts",
      dirtyIndicatorCleared: true,
      contentSha256: "1".repeat(64),
      diskSha256: "2".repeat(64),
    },
    outline: {
      resource: "journey.ts",
      count: 1,
      namesSha256: "3".repeat(64),
    },
    diff: {
      resource: "journey.ts",
      lineCount: 1,
      contentSha256: "4".repeat(64),
    },
    recovery: {
      resource: "journey.ts",
      conflictPresented: true,
      recovered: true,
      diskSha256: "5".repeat(64),
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

function availableLoopbackPort() {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close((error) => (error ? reject(error) : resolve(port)));
    });
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

test("process identity helper failures keep the stable code and carry what PowerShell reported", () => {
  const failure = (result) => {
    try {
      processIdentityHelperOutput(result);
    } catch (error) {
      assert.equal(error.message, "journey-tool-cleanup-unproven");
      return error.cause;
    }
    assert.fail("the helper result was accepted");
  };

  assert.deepEqual(
    processIdentityHelperOutput({ status: 0, stdout: '{"processId":7}\n' }),
    { processId: 7 },
  );
  assert.equal(
    failure({ status: 1, signal: null, stderr: "  Access is denied.\r\n" })
      .message,
    "process identity helper exited with status 1: Access is denied.",
  );
  assert.equal(
    failure({ status: null, signal: "SIGTERM", stderr: "" }).message,
    "process identity helper exited with status null (SIGTERM): ",
  );
  const oversized = failure({
    status: 1,
    signal: null,
    stderr: "x".repeat(HELPER_DIAGNOSTIC_CHARS_MAX + 1),
  }).message;
  assert.equal(
    oversized,
    `process identity helper exited with status 1: ${"x".repeat(HELPER_DIAGNOSTIC_CHARS_MAX)}`,
  );
  const spawnError = Object.assign(
    new Error("spawnSync powershell ETIMEDOUT"),
    {
      code: "ETIMEDOUT",
    },
  );
  assert.equal(failure({ error: spawnError, status: null }), spawnError);
  assert.ok(failure({ status: 0, stdout: "not json" }) instanceof SyntaxError);
});

function fakeBoundedToolChild(pid) {
  const child = new EventEmitter();
  child.stdout = new PassThrough();
  child.stderr = new PassThrough();
  child.pid = pid;
  return child;
}

test("bounded tool rejection carries the identity capture failure as its cause", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-tool-capture-cause-"));
  try {
    const captureFailure = new Error(
      "process identity helper exited with status 1: denied",
    );
    await assert.rejects(
      runBoundedTool("tool", [], Date.now() + 5_000, root, {
        spawn: () => fakeBoundedToolChild(71),
        captureIdentity: () => {
          throw captureFailure;
        },
      }),
      (error) => {
        assert.equal(error.message, "playwright-cli-cleanup-unproven");
        assert.equal(error.cause, captureFailure);
        return true;
      },
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("bounded tool rejection carries the cleanup failure as its cause", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-tool-cleanup-cause-"));
  try {
    const child = fakeBoundedToolChild(72);
    const cleanupFailure = new Error(
      "process identity helper exited with status 1: denied",
    );
    const promise = runBoundedTool("tool", [], Date.now() + 5_000, root, {
      spawn: () => child,
      captureIdentity: () => ({ processId: 72, creationDate: "exact" }),
      cleanupIdentity: () => {
        throw cleanupFailure;
      },
    });
    child.stdout.write(Buffer.alloc(64 * 1024 + 1));
    await assert.rejects(promise, (error) => {
      assert.equal(error.message, "playwright-cli-cleanup-unproven");
      assert.equal(error.cause, cleanupFailure);
      return true;
    });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
