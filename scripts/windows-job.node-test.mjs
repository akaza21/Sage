import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

test("native Job launch preserves exact argv and proves active-process convergence", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-windows-job-"));
  try {
    const values = [
      "",
      "plain",
      "with space",
      'quote"inside',
      "trailing\\",
      'slashes\\\\"quote',
      "carriage\rreturn",
      "form\ffeed",
      "unicode-λ",
    ];
    const helper = path.join(root, "argv-helper.mjs");
    const result = path.join(root, "argv.json");
    writeFileSync(
      helper,
      `import fs from "node:fs";\nconst [output, ...values] = process.argv.slice(2);\nsetTimeout(() => fs.writeFileSync(output, JSON.stringify(values)), 250);\n`,
    );
    const modulePath = path.resolve("scripts/footprint/WindowsJob.psm1");
    const completionModulePath = path.resolve(
      "scripts/footprint/WindowsJobCompletion.psm1",
    );
    const driver = path.join(root, "driver.ps1");
    const encodedValues = Buffer.from(JSON.stringify(values)).toString(
      "base64",
    );
    writeFileSync(
      driver,
      `param([string]$ModulePath,[string]$CompletionModulePath,[string]$NodePath,[string]$Helper,[string]$Result,[string]$EncodedValues)\n[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)\nImport-Module $ModulePath -Force\nImport-Module $CompletionModulePath -Force\n[string[]]$values = ([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($EncodedValues)) | ConvertFrom-Json)\n$info = [Diagnostics.ProcessStartInfo]::new()\n$info.FileName = $NodePath\n$info.WorkingDirectory = Split-Path -Parent $Helper\n$info.UseShellExecute = $false\n$info.CreateNoWindow = $true\n$launch = Start-FootprintJobProcess $info -ArgumentList (@($Helper,$Result) + $values) -ActiveProcessLimit 4\n$active = [uint32]$launch.boundary.ActiveProcesses()\n$cleanup = Wait-FootprintJobProcess $launch.process $launch.boundary 5000 200\n$launch.boundary.Dispose()\n[string[]]$argv = ([IO.File]::ReadAllText($Result,[Text.Encoding]::UTF8) | ConvertFrom-Json)\n[ordered]@{ active = $active; orphan = $cleanup.orphanProcessesAfterClose; argv = $argv } | ConvertTo-Json -Compress\n`,
    );
    const stdout = execFileSync(
      "powershell.exe",
      [
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        driver,
        modulePath,
        completionModulePath,
        process.execPath,
        helper,
        result,
        encodedValues,
      ],
      { encoding: "utf8", timeout: 20_000, windowsHide: true },
    );
    const observed = JSON.parse(stdout.trim());
    assert.ok(observed.active >= 1);
    assert.equal(observed.orphan, 0);
    assert.deepEqual(observed.argv, values);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
