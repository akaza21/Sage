import {
  appendFileSync,
  existsSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { runTool, executableIdentity } from "./capture-native-chat-process.mjs";
import { cleanSource } from "./capture-native-chat-source.mjs";
import {
  parseRun,
  runCapture,
  runBatch,
  nextBatch,
  ledgerRecords,
  stagedProbe,
  nextLedgerPath,
} from "./capture-native-chat-batch.mjs";
import {
  editorCells,
  parseEditorArgs,
  validateEditorProbe,
} from "./capture-native-editor-catalog.mjs";

const workspace = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
export function parseEditorRun(line) {
  const run = parseRun(line, /^(editor):([a-z0-9-]{1,64}):(1888|720|480)$/u);
  parseEditorArgs([
    "--harness",
    run.harness,
    "--state",
    run.state,
    "--width",
    String(run.width),
  ]);
  return run;
}
export const planEditorRuns = () =>
  editorCells()
    .filter(({ supported }) => supported)
    .map(({ state, width }) => `editor:${state}:${width}`);

export async function editorPreflight(root = workspace) {
  const tasks = await runTool("tasklist.exe", [
    "/FI",
    "IMAGENAME eq sage.exe",
    "/NH",
  ]);
  if (/sage\.exe/iu.test(tasks)) return "sage.exe already running";
  try {
    await cleanSource(root);
  } catch {
    return "tree dirty or source unavailable";
  }
  return null;
}
export function runEditorBatch({ root = workspace, executable, ...options }) {
  return runBatch({
    ...options,
    parse: parseEditorRun,
    capture:
      options.capture ??
      ((run) =>
        runCapture(
          run,
          root,
          "scripts/capture-native-editor-capture.mjs",
          executable ? { NATIVE_EDITOR_EXE: executable } : {},
        )),
    check: options.check ?? (() => editorPreflight(root)),
  });
}
const lines = (file) =>
  readFileSync(file, "utf8")
    .replace(/^\uFEFF/u, "")
    .split(/\r?\n/u)
    .filter(Boolean);
export async function main(args, root = workspace) {
  const [command, ...rest] = args;
  const values = {};
  if (rest.length % 2) throw new Error("editor-batch-arguments");
  for (let index = 0; index < rest.length; index += 2) {
    if (
      !["--out", "--runs", "--prefix", "--build"].includes(rest[index]) ||
      rest[index] in values
    )
      throw new Error("editor-batch-arguments");
    values[rest[index]] = rest[index + 1];
  }
  if (command === "catalogue") {
    writeFileSync(
      values["--out"],
      JSON.stringify({ schemaVersion: 1, cells: editorCells() }, null, 2) +
        "\n",
      { flag: "wx" },
    );
    return { cells: 18 };
  }
  if (command === "plan") {
    const runs = planEditorRuns();
    writeFileSync(values["--out"], runs.join("\n") + "\n", { flag: "wx" });
    return { runs: runs.length };
  }
  if (command !== "run") throw new Error("editor-batch-command");
  const order = lines(values["--runs"]);
  order.forEach(parseEditorRun);
  const build = executableIdentity(values["--build"]);
  const records = ledgerRecords(root, values["--prefix"]);
  const probeFor = (record) => {
    try {
      const probe = stagedProbe(root, record);
      validateEditorProbe(probe);
      if (
        record.harness !== "editor" ||
        probe.caseId !== "editor-" + record.state ||
        probe.dimensions.client.width !== record.width
      )
        return null;
      const png = path.join(
        root,
        record.result.directory,
        `${record.state}-editor-${record.width}.png`,
      );
      return existsSync(png) ? probe : null;
    } catch {
      return null;
    }
  };
  const runs = nextBatch({ order, records, build: build.sha256, probeFor });
  const ledger = nextLedgerPath(root, values["--prefix"]);
  if (!runs.length) {
    const captured = new Set(
      records
        .filter(
          (record) =>
            record.result?.verdict === "captured" &&
            probeFor(record)?.hashes.build === build.sha256,
        )
        .map(({ state, width }) => `editor:${state}:${width}`),
    );
    const done = order.every((cell) => captured.has(cell));
    return {
      done,
      blocked: !done,
      remaining: done
        ? "All supported cells captured by this build."
        : "Attempts exhausted; inspect blocked ledgers before starting a new prefix.",
    };
  }
  const result = await runEditorBatch({
    root,
    runs,
    ledger,
    executable: build.path,
    append: (record) =>
      appendFileSync(ledger, JSON.stringify(record) + "\n", "utf8"),
  });
  return { ledger: path.relative(root, ledger), ...result };
}
if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href
) {
  try {
    const result = await main(process.argv.slice(2));
    console.log(JSON.stringify(result));
    if (
      result.aborted ||
      result.blocked ||
      result.records?.some(({ result }) => result?.verdict !== "captured")
    )
      process.exitCode = 1;
  } catch (error) {
    console.error("native-editor-batch-blocked: " + error.message);
    process.exitCode = 1;
  }
}
