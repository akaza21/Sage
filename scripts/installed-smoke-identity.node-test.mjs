import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import {
  mkdirSync,
  mkdtempSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

test("installed-smoke paths reject a junction ancestor and ambiguous drive root", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-smoke-path-"));
  const target = path.join(root, "target");
  const junction = path.join(root, "junction");
  const ordinaryRoot = path.join(root, "ordinary");
  const driver = path.join(root, "driver.ps1");
  try {
    mkdirSync(target);
    mkdirSync(path.join(target, "output"));
    writeFileSync(path.join(target, "output", "sage.exe"), "payload");
    symlinkSync(target, junction, "junction");
    mkdirSync(ordinaryRoot);
    writeFileSync(path.join(ordinaryRoot, "sage.exe"), "payload");
    writeFileSync(
      driver,
      `param([string]$Module,[string]$OrdinaryRoot,[string]$JunctionRoot)\nImport-Module $Module -Force\n$ordinary = Assert-InstalledSmokePath (Join-Path $OrdinaryRoot 'sage.exe') $OrdinaryRoot 'File'\n$junctionRejected = $false\ntry { [void](Assert-InstalledSmokePath (Join-Path $JunctionRoot 'sage.exe') $JunctionRoot 'File') } catch { $junctionRejected = $true }\n$driveRejected = $false\n$driveRoot = [IO.Path]::GetPathRoot($OrdinaryRoot)\ntry { [void](Assert-InstalledSmokePath $driveRoot $driveRoot 'Directory') } catch { $driveRejected = $true }\n[ordered]@{ ordinary = [IO.Path]::IsPathRooted($ordinary); junctionRejected = $junctionRejected; driveRejected = $driveRejected } | ConvertTo-Json -Compress\n`,
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
        path.resolve("scripts/footprint/InstalledSmokeIdentity.psm1"),
        ordinaryRoot,
        path.join(junction, "output"),
      ],
      { encoding: "utf8", timeout: 10_000, windowsHide: true },
    );
    assert.deepEqual(JSON.parse(stdout.trim()), {
      ordinary: true,
      junctionRejected: true,
      driveRejected: true,
    });
  } finally {
    rmSync(junction, { force: true });
    rmSync(root, { recursive: true, force: true });
  }
});
