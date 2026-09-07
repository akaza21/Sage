import {
  existsSync,
  lstatSync,
  readFileSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  attestNativeDiagnostics,
  revalidateCollectionIdentities,
} from "./source-functional-acp04-finalizer.mjs";
import { decodeTerminalZeroCleanup } from "./source-functional-acp04-finalizer.mjs";
import { validatePendingAv01PreviewReceipt } from "./source-functional-av01-preview-contract.mjs";

function fail(code) {
  throw new Error(`av01-preview-finalizer-${code}`);
}

function absolute(value) {
  return (
    typeof value === "string" &&
    (path.isAbsolute(value) || path.win32.isAbsolute(value))
  );
}

function exactKeys(value, keys) {
  return (
    value &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    Object.keys(value).sort().join("\0") === [...keys].sort().join("\0")
  );
}

export function parseAv01PreviewFinalizerArguments(argv) {
  if (!Array.isArray(argv) || argv.length % 2 !== 0) fail("arguments-invalid");
  const values = Object.create(null);
  for (let index = 0; index < argv.length; index += 2) {
    const rawKey = argv[index];
    if (typeof rawKey !== "string" || !rawKey.startsWith("--"))
      fail("arguments-invalid");
    const key = rawKey.slice(2);
    if (!key || key in values) fail("arguments-invalid");
    values[key] = argv[index + 1];
  }
  const keys = [
    "pending",
    "final",
    "source-root",
    "stdout",
    "stderr",
    "sage-cleanup",
    "vite-cleanup",
    "deadline-ms",
  ];
  const deadlineText = values["deadline-ms"];
  const deadlineMs = /^[1-9]\d*$/u.test(deadlineText ?? "")
    ? Number(deadlineText)
    : Number.NaN;
  if (
    !exactKeys(values, keys) ||
    ![
      values.pending,
      values.final,
      values["source-root"],
      values.stdout,
      values.stderr,
    ].every(absolute) ||
    !Number.isSafeInteger(deadlineMs) ||
    deadlineMs < 1_000 ||
    deadlineMs > 60_000 ||
    path.dirname(values.pending) !== path.dirname(values.final) ||
    path.dirname(values.pending) !== path.dirname(values.stdout) ||
    path.dirname(values.pending) !== path.dirname(values.stderr) ||
    path.basename(values.pending) !== "machine-receipt.pending.json" ||
    path.basename(values.final) !== "machine-receipt.json" ||
    existsSync(values.final)
  )
    fail("arguments-invalid");
  return Object.freeze({ ...values, deadlineMs });
}

function readPending(filePath) {
  if (
    !existsSync(filePath) ||
    lstatSync(filePath).isSymbolicLink() ||
    !lstatSync(filePath).isFile() ||
    lstatSync(filePath).size > 1024 * 1024
  )
    fail("pending-invalid");
  try {
    return JSON.parse(readFileSync(filePath, "utf8"));
  } catch {
    fail("pending-invalid");
  }
}

export async function finalizeAv01PreviewCollection(options, deps = {}) {
  const candidate = (deps.validatePending ?? validatePendingAv01PreviewReceipt)(
    readPending(options.pending),
    path.dirname(options.pending),
  );
  const cleanup = (deps.decodeCleanup ?? decodeTerminalZeroCleanup)(
    options["sage-cleanup"],
    options["vite-cleanup"],
  );
  const diagnostics = (deps.attestDiagnostics ?? attestNativeDiagnostics)(
    options.stdout,
    options.stderr,
  );
  await (deps.revalidate ?? revalidateCollectionIdentities)(
    {
      sourceRoot: options["source-root"],
      output: path.dirname(options.pending),
      source: candidate.source,
      executable: candidate.identities.sageExecutable,
      node: candidate.identities.node,
      codex: candidate.identities.codexPreflight,
    },
    Date.now() + options.deadlineMs,
  );
  const receipt = Object.freeze({
    ...candidate,
    finalizedAt: new Date().toISOString(),
    cleanup,
    diagnostics,
  });
  writeFileSync(options.final, `${JSON.stringify(receipt, null, 2)}\n`, {
    flag: "wx",
  });
  unlinkSync(options.pending);
  return receipt;
}

if (
  process.argv[1] &&
  path.resolve(fileURLToPath(import.meta.url)) === path.resolve(process.argv[1])
) {
  try {
    await finalizeAv01PreviewCollection(
      parseAv01PreviewFinalizerArguments(process.argv.slice(2)),
    );
  } catch (error) {
    console.error(String(error?.message ?? error).slice(0, 1024));
    process.exitCode = 1;
  }
}
