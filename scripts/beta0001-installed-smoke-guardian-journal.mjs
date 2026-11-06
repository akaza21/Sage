import { readFileSync } from "node:fs";
import { appendFile, stat } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { guardianPhases } from "./beta0001-installed-smoke-output-diagnostics.mjs";

// CI points this at a file on the runner's temp disk so a job can report how
// long each guardian took to start, including the ones that succeeded.
export const GUARDIAN_JOURNAL_ENVIRONMENT = "SAGE_GUARDIAN_PHASE_JOURNAL";
export const GUARDIAN_JOURNAL_MAXIMUM_BYTES = 64 * 1024;
const outcomes = new Set(["ready", "timeout"]);

function observedMs(phase) {
  return Number.isSafeInteger(phase?.observedMs) && phase.observedMs >= 0
    ? phase.observedMs
    : null;
}

// Appends run one at a time, so the size check before each one holds the cap.
let pending = Promise.resolve();

async function appendWithinCap(journal, line) {
  let size = 0;
  try {
    size = (await stat(journal)).size;
  } catch {
    /* A missing journal starts empty. */
  }
  if (size + Buffer.byteLength(line) > GUARDIAN_JOURNAL_MAXIMUM_BYTES) {
    return false;
  }
  await appendFile(journal, line);
  return true;
}

// Records fixed labels and numbers only, never paths or child output. The
// write is asynchronous and the promise never rejects: callers settle the
// guardian first and never wait on this, because the journal exists to
// diagnose a stalled disk. An unset, full or unwritable journal records
// nothing.
export function recordGuardianStartup(
  snapshot,
  outcome,
  journal = process.env[GUARDIAN_JOURNAL_ENVIRONMENT],
) {
  if (!journal || !outcomes.has(outcome)) return Promise.resolve(false);
  const record = {
    outcome,
    elapsedMs: snapshot.elapsedMs,
    phases: Object.fromEntries(
      guardianPhases.map((phase) => [
        phase,
        observedMs(snapshot.phases?.[phase]),
      ]),
    ),
  };
  const line = `${JSON.stringify(record)}\n`;
  const written = pending.then(() =>
    appendWithinCap(journal, line).catch(() => false),
  );
  pending = written;
  return written;
}

// One line: how many guardians started, and the slowest time each startup
// step took across them. A step runs from the previous reached phase, or from
// the spawn for script-start, to its own phase.
export function summarizeGuardianJournal(text) {
  const slowest = Object.fromEntries(
    guardianPhases.map((phase) => [phase, null]),
  );
  let starts = 0;
  let timeouts = 0;
  let slowestTotal = 0;
  for (const line of text.split("\n")) {
    if (!line.trim()) continue;
    let record;
    try {
      record = JSON.parse(line);
    } catch {
      continue;
    }
    if (!outcomes.has(record?.outcome)) continue;
    starts++;
    if (record.outcome === "timeout") timeouts++;
    if (Number.isSafeInteger(record.elapsedMs)) {
      slowestTotal = Math.max(slowestTotal, record.elapsedMs);
    }
    let previous = 0;
    for (const phase of guardianPhases) {
      const reached = record.phases?.[phase];
      if (!Number.isSafeInteger(reached) || reached < 0) continue;
      const step = Math.max(0, reached - previous);
      slowest[phase] = Math.max(slowest[phase] ?? 0, step);
      previous = reached;
    }
  }
  if (starts === 0) return "guardian startup: no guardian started";
  const steps = guardianPhases
    .filter((phase) => slowest[phase] !== null)
    .map((phase) => `${phase} ${slowest[phase]}`)
    .join(", ");
  return `guardian startup: ${starts} starts, ${timeouts} timed out, slowest total ${slowestTotal} ms; slowest step ms: ${steps}`;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  const journal = process.argv[2];
  let text = "";
  try {
    text = readFileSync(journal, "utf8");
  } catch {
    /* No guardian started, or the journal was never created. */
  }
  console.log(summarizeGuardianJournal(text));
}
