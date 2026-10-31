import { mkdir, readFile } from "node:fs/promises";
import { spawn, spawnSync } from "node:child_process";
import path from "node:path";
import { randomUUID } from "node:crypto";
import { setTimeout as delay } from "node:timers/promises";
import { expect, test } from "@playwright/test";
import { forbidBrowserRetries } from "./shared/browserRetryGuard";

forbidBrowserRetries();
const BRIDGE_STARTUP_MS = 15 * 60 * 1_000;
const BRIDGE_QUEUE_MS = 45 * 60 * 1_000;

test("a historical component read reaches the real native snapshot command and SQLite bytes", async ({
  page,
}) => {
  test.setTimeout(BRIDGE_QUEUE_MS + BRIDGE_STARTUP_MS + 30_000);
  const output = path.resolve(
    "output/playwright",
    `gap-c-native-${randomUUID()}`,
  );
  await mkdir(output, { recursive: true });
  const bootstrapPath = path.join(output, "bootstrap.json");
  const log = path.join(output, "cargo.log");
  const lane = process.cwd();
  const command = String.raw`powershell -NoProfile -ExecutionPolicy Bypass -File C:\Sage\work\cargo-wt-seq.ps1 "${lane}" test --lib component_snapshot_bridge_reads_historical_sqlite_bytes --- --test-threads=2 > "${log}" 2>&1`;
  const bridge = spawn("cmd.exe", ["/d", "/s", "/c", `"${command}"`], {
    cwd: lane,
    windowsHide: true,
    windowsVerbatimArguments: true,
    stdio: "ignore",
    env: { ...process.env, SAGE_GAP_C_SNAPSHOT_BOOTSTRAP: bootstrapPath },
  });
  let bootstrap:
    | { url: string; reference: unknown; page: unknown; project: unknown }
    | undefined;
  let assertionsPassed = false;
  try {
    let deadline = Date.now() + BRIDGE_QUEUE_MS;
    let acquired = false;
    while (!bootstrap && Date.now() < deadline) {
      if (bridge.exitCode !== null)
        throw new Error(
          `Native bridge exited ${bridge.exitCode}: ${await readFile(log, "utf8").catch(() => "No Cargo log was created")}`,
        );
      try {
        bootstrap = JSON.parse(await readFile(bootstrapPath, "utf8"));
      } catch (cause) {
        if ((cause as NodeJS.ErrnoException).code !== "ENOENT") throw cause;
      }
      if (
        !acquired &&
        (await readFile(log, "utf8").catch(() => "")).includes(
          "cargo-wt-seq: cargo test",
        )
      ) {
        acquired = true;
        deadline = Date.now() + BRIDGE_STARTUP_MS;
      }
      if (!bootstrap) await delay(100);
    }
    if (!bootstrap)
      throw new Error(
        "Native snapshot bridge did not start within its bounded deadline.",
      );
    await page.exposeFunction("gapCSnapshotBootstrap", () => bootstrap);
    await page.exposeFunction("gapCReadSnapshot", async (request: unknown) => {
      const response = await fetch(`${bootstrap!.url}/snapshot`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(request),
      });
      if (!response.ok)
        throw new Error(`Native snapshot rejected: ${await response.text()}`);
      return response.json();
    });
    await page.goto("/tests/ui/gap-c-surfaces.html?surface=native-snapshot");
    await page
      .getByRole("button", { name: "Open retained saved diff" })
      .click();
    const diff = page.getByRole("table", { name: "Unified file diff" });
    await expect(diff).toContainText("original old");
    await expect(diff).toContainText("original new");
    await expect(
      page.getByText("Saved agent change", { exact: true }),
    ).toBeVisible();
    assertionsPassed = true;
  } finally {
    if (bootstrap)
      await fetch(`${bootstrap.url}/finish`, { method: "POST" }).catch(
        () => undefined,
      );
    const deadline = Date.now() + 10_000;
    while (bridge.exitCode === null && Date.now() < deadline) await delay(100);
    if (bridge.exitCode === null && bridge.pid)
      spawnSync("taskkill", ["/PID", String(bridge.pid), "/T", "/F"], {
        windowsHide: true,
        stdio: "ignore",
      });
    if (assertionsPassed)
      expect(bridge.exitCode, await readFile(log, "utf8")).toBe(0);
  }
});
