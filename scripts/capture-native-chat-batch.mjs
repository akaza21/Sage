// Resumable native capture batches (NC-3/NC-4 operating model). Each batch
// runs at most 30 captures one at a time and appends one JSONL ledger record
// per run; `next` skips cells already captured by the given sage.exe build,
// so an interrupted pass resumes from its ledgers. `pairs` lists each
// captured PNG (and motion frames) beside the live Codex frames it must
// match. Nothing here builds binaries or promotes evidence.
//
//   node scripts/capture-native-chat-batch.mjs plan --out <runs.txt> [--parity] [--states a,b] [--harness h]
//   node scripts/capture-native-chat-batch.mjs next --runs <runs.txt> --prefix <name> --build <sha256> --out <batch.txt> [--max-attempts 1] [--size 30]
//   node scripts/capture-native-chat-batch.mjs run --runs <batch.txt> --ledger <work/<name>-<n>.jsonl>
//   node scripts/capture-native-chat-batch.mjs loop --runs <runs.txt> --prefix <name> --build <sha256> [--max-attempts 1] [--max-batches 20]
//   node scripts/capture-native-chat-batch.mjs pairs --prefix <name> --live <codex-live dir> --out <pairs.jsonl>
import { spawn, spawnSync } from "node:child_process";
import {
  appendFileSync,
  existsSync,
  readFileSync,
  readdirSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:net";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs as parseCaptureArgs } from "./capture-native-chat-contract.mjs";
import { nativeCases } from "./acp-native-cases.mjs";
import {
  PARITY_STATE_NAMES,
  parityEntry,
} from "./capture-native-chat-parity.mjs";

export const MAX_BATCH = 30;
const HARNESSES = ["codex", "claude", "pi"];
const WIDTHS = [1888, 720, 480];
const RUN = /^(codex|claude|pi):([a-z0-9-]{1,64}):(1888|720|480)$/u;
const SHA = /^[a-f0-9]{64}$/u;
const NAME = /^[a-z0-9][a-z0-9-]{0,47}$/u;
const workspace = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);

function fail(code) {
  throw new Error(code);
}
export function parseRun(line, pattern = RUN) {
  const match = pattern.exec(line.trim());
  if (!match) fail("batch-run");
  return { harness: match[1], state: match[2], width: Number(match[3]) };
}
const key = ({ harness, state, width }) => harness + ":" + state + ":" + width;

/** Every admitted harness x state x width, catalogue order (CLI refusals dropped). */
export function planRuns({ states = null, harnesses = HARNESSES } = {}) {
  const order = [];
  const seen = new Set();
  for (const { id } of nativeCases()) {
    const [harness, ...rest] = id.split("-");
    const state = rest.join("-");
    if (!harnesses.includes(harness) || (states && !states.includes(state)))
      continue;
    for (const width of WIDTHS) {
      const run = { harness, state, width };
      try {
        parseCaptureArgs([
          "--harness",
          harness,
          "--state",
          state,
          "--width",
          String(width),
        ]);
      } catch {
        continue;
      }
      if (seen.has(key(run))) continue;
      seen.add(key(run));
      order.push(key(run));
    }
  }
  return order;
}

/** Ledger records, tolerant of a UTF-8 BOM and blank lines (PowerShell). */
export function readLedger(text) {
  return text
    .split(/\r?\n/u)
    .map((line) => line.replace(/^\uFEFF/u, "").trim())
    .filter(Boolean)
    .map((line) => JSON.parse(line));
}

/**
 * The next batch: cells in order not yet captured by `build` and attempted
 * fewer than `maxAttempts` times. `probeFor(record)` returns the staged
 * probe JSON of a captured record, or null.
 */
export function nextBatch({
  order,
  records,
  build,
  probeFor,
  maxAttempts = 1,
  size = MAX_BATCH,
}) {
  if (!SHA.test(build)) fail("batch-build");
  if (!Number.isSafeInteger(size) || size < 1 || size > MAX_BATCH)
    fail("batch-size");
  const attempts = new Map(),
    done = new Set();
  for (const record of records) {
    const cell = key(record);
    attempts.set(cell, (attempts.get(cell) ?? 0) + 1);
    if (record.result?.verdict !== "captured") continue;
    const probe = probeFor(record);
    if (probe?.verdict === "captured" && probe.hashes?.build === build)
      done.add(cell);
  }
  return order
    .filter(
      (cell) => !done.has(cell) && (attempts.get(cell) ?? 0) < maxAttempts,
    )
    .slice(0, size);
}

/** The staged probe of a ledger record, or null; paths stay in the lane. */
export function stagedProbe(root, record) {
  const directory = record.result?.directory;
  if (
    typeof directory !== "string" ||
    !/^work[\\/]native-captures[\\/][a-f0-9-]{36}$/u.test(directory)
  )
    return null;
  const file = path.join(
    root,
    directory,
    `${record.state}-${record.harness}-${record.width}.probe.json`,
  );
  return existsSync(file) ? JSON.parse(readFileSync(file, "utf8")) : null;
}

export function ledgerRecords(root, prefix) {
  if (!NAME.test(prefix)) fail("batch-prefix");
  const directory = path.join(root, "work");
  if (!existsSync(directory)) return [];
  const pattern = new RegExp("^" + prefix + "-\\d+\\.jsonl$", "u");
  return readdirSync(directory)
    .filter((name) => pattern.test(name))
    .flatMap((name) =>
      readLedger(readFileSync(path.join(directory, name), "utf8")),
    );
}

export function nextLedgerPath(root, prefix) {
  if (!NAME.test(prefix)) fail("batch-prefix");
  let index = 1;
  while (existsSync(path.join(root, "work", `${prefix}-${index}.jsonl`)))
    index++;
  return path.join(root, "work", `${prefix}-${index}.jsonl`);
}

/** No sage.exe, port 1420 free and a clean tree, before every run. */
export async function preflight(root = workspace) {
  const tasks = spawnSync("tasklist", ["/FI", "IMAGENAME eq sage.exe", "/NH"], {
    encoding: "utf8",
    windowsHide: true,
  });
  if (/sage\.exe/iu.test(tasks.stdout ?? "")) return "sage.exe already running";
  const free = await new Promise((resolve) => {
    const server = createServer();
    server.once("error", () => resolve(false));
    server.listen(1420, "127.0.0.1", () => server.close(() => resolve(true)));
  });
  if (!free) return "port 1420 busy";
  const status = spawnSync("git", ["status", "--porcelain"], {
    cwd: root,
    encoding: "utf8",
    windowsHide: true,
  });
  if (status.status !== 0 || status.stdout.trim()) return "tree dirty";
  return null;
}

/** Runs one capture and returns its last JSON line (bounded output). */
export function runCapture(
  { harness, state, width },
  root = workspace,
  script = "scripts/capture-native-chat.mjs",
  environment = {},
) {
  return new Promise((resolve) => {
    const child = spawn(
      process.execPath,
      [
        script,
        "--harness",
        harness,
        "--state",
        state,
        "--width",
        String(width),
      ],
      {
        cwd: root,
        windowsHide: true,
        stdio: ["ignore", "pipe", "pipe"],
        env: { ...process.env, ...environment },
      },
    );
    let output = "";
    const take = (chunk) => {
      output = (output + chunk.toString()).slice(-65536);
    };
    child.stdout.on("data", take);
    child.stderr.on("data", take);
    child.on("close", (code) => {
      const line = output
        .split(/\r?\n/u)
        .reverse()
        .find((entry) => entry.startsWith("{"));
      let result;
      try {
        result = line ? JSON.parse(line) : null;
      } catch {
        result = null;
      }
      resolve({
        exit: code,
        result: result ?? {
          verdict: "no-json",
          raw: output.trim().slice(-512),
        },
      });
    });
  });
}

/** Runs a batch of at most 30 cells; stops (and says why) on a failed preflight. */
export async function runBatch({
  runs,
  ledger,
  capture = runCapture,
  check = preflight,
  append = (record) =>
    appendFileSync(ledger, JSON.stringify(record) + "\n", "utf8"),
  now = Date.now,
  parse = parseRun,
}) {
  if (runs.length > MAX_BATCH) fail("batch-size");
  const records = [];
  for (const line of runs) {
    const run = parse(line);
    const blocker = await check();
    if (blocker) return { aborted: blocker + " before " + key(run), records };
    const started = now();
    const { exit, result } = await capture(run);
    const record = {
      ...run,
      exit,
      secs: Math.round((now() - started) / 1000),
      result,
    };
    append(record);
    records.push(record);
  }
  return { aborted: null, records };
}

/** Lists the live Codex frames a reference names, e.g. "s2/f0093". */
export function liveFrames(liveRoot, ref) {
  const match = /^(s[0-9])\/(f[0-9]{4})$/u.exec(ref);
  if (!match) fail("batch-live-ref");
  const segment = readdirSync(liveRoot).find((name) =>
    name.startsWith(match[1] + "-"),
  );
  if (!segment) return [];
  return readdirSync(path.join(liveRoot, segment))
    .filter((name) => name.startsWith(match[2] + "-") && name.endsWith(".png"))
    .map((name) => path.join(liveRoot, segment, name));
}

/** Captured parity cells beside the live frames they must match. */
export function pairRuns(root, records, liveRoot) {
  return records
    .filter(
      (record) =>
        record.result?.verdict === "captured" &&
        PARITY_STATE_NAMES.includes(record.state),
    )
    .map((record) => {
      const entry = parityEntry(record.state);
      const directory = path.join(root, record.result.directory);
      const stem = `${record.state}-${record.harness}-${record.width}`;
      const frames = existsSync(path.join(directory, stem + ".frames.json"))
        ? JSON.parse(
            readFileSync(path.join(directory, stem + ".frames.json"), "utf8"),
          ).frames.map(({ name }) => path.join(directory, name))
        : [];
      return {
        harness: record.harness,
        state: record.state,
        width: record.width,
        png: path.join(directory, stem + ".png"),
        frames,
        spec: entry.spec,
        live: entry.live.flatMap((ref) => liveFrames(liveRoot, ref)),
      };
    });
}

function options(args) {
  const values = {};
  for (let index = 0; index < args.length; index++) {
    const name = args[index];
    if (!name.startsWith("--") || name in values) fail("batch-arguments");
    if (name === "--parity") values[name] = true;
    else values[name] = args[++index];
    if (values[name] === undefined) fail("batch-arguments");
  }
  return values;
}
const lines = (file) =>
  readFileSync(file, "utf8")
    .split(/\r?\n/u)
    .map((line) => line.trim())
    .filter(Boolean);

export async function main(argv, root = workspace) {
  const [command, ...rest] = argv;
  const values = options(rest);
  if (command === "plan") {
    const states = values["--parity"]
      ? PARITY_STATE_NAMES
      : values["--states"]?.split(",");
    const order = planRuns({
      states: states ?? null,
      harnesses: values["--harness"] ? [values["--harness"]] : HARNESSES,
    });
    writeFileSync(values["--out"], order.join("\n") + "\n");
    return { runs: order.length };
  }
  const batchFor = () =>
    nextBatch({
      order: lines(values["--runs"]),
      records: ledgerRecords(root, values["--prefix"]),
      build: values["--build"],
      probeFor: (record) => stagedProbe(root, record),
      maxAttempts: Number(values["--max-attempts"] ?? 1),
      size: Number(values["--size"] ?? MAX_BATCH),
    });
  if (command === "next") {
    const batch = batchFor();
    writeFileSync(
      values["--out"],
      batch.join(batch.length ? "\n" : "") + (batch.length ? "\n" : ""),
    );
    return { next: batch.length };
  }
  if (command === "run")
    return runBatch({
      runs: lines(values["--runs"]),
      ledger: values["--ledger"],
    });
  if (command === "loop") {
    const maxBatches = Number(values["--max-batches"] ?? 20);
    for (let index = 0; index < maxBatches; index++) {
      const batch = batchFor();
      if (!batch.length) return { done: true, batches: index };
      const ledger = nextLedgerPath(root, values["--prefix"]);
      const result = await runBatch({ runs: batch, ledger });
      console.log(JSON.stringify({ ledger: path.basename(ledger), ...result }));
      if (result.aborted) return { done: false, aborted: result.aborted };
    }
    return { done: false, batches: maxBatches };
  }
  if (command === "pairs") {
    const pairs = pairRuns(
      root,
      ledgerRecords(root, values["--prefix"]),
      values["--live"],
    );
    writeFileSync(
      values["--out"],
      pairs.map((pair) => JSON.stringify(pair)).join("\n") + "\n",
    );
    return { pairs: pairs.length };
  }
  fail("batch-command");
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href
) {
  try {
    console.log(JSON.stringify(await main(process.argv.slice(2))));
  } catch (error) {
    console.error("native-chat-batch-blocked: " + error.message);
    process.exitCode = 1;
  }
}
