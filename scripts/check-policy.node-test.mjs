import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { validateMainCapabilityPermissions } from "./check-policy-permissions.mjs";

test("app commands are allowed only in the exact main webview capability", () => {
  const main = {
    identifier: "main",
    webviews: ["main"],
    permissions: ["core:default", "main-commands"],
  };
  assert.doesNotThrow(() => validateMainCapabilityPermissions(main));
  for (const capability of [
    { ...main, webviews: ["main", "browser-1"] },
    { ...main, webviews: ["browser-1"] },
    { ...main, windows: ["main"] },
    { ...main, remote: { urls: ["https://example.com"] } },
    { ...main, identifier: "other" },
    { ...main, permissions: ["core:default", "other-command-set"] },
  ]) {
    assert.throws(() => validateMainCapabilityPermissions(capability));
  }
});

test("reviewed Windows API features include only ConPTY and existing containment needs", () => {
  const policy = readFileSync(
    new URL("./check-policy.mjs", import.meta.url),
    "utf8",
  );
  const list = policy.match(/const windowsFeatures = \[([\s\S]*?)\];/u)?.[1];
  assert.ok(list, "Windows feature allowlist must exist");
  const features = [...list.matchAll(/"([^"]+)"/gu)].map((match) => match[1]);
  assert.deepEqual(features, [
    "Win32_Foundation",
    "Win32_Security",
    "Win32_Storage_FileSystem",
    "Win32_System_Console",
    "Win32_System_JobObjects",
    "Win32_System_Pipes",
    "Win32_System_Performance",
    "Win32_System_SystemInformation",
    "Win32_System_Threading",
  ]);
});
