// Strips what a committed transcript must never carry: local paths, the
// account's identity, secrets and emails. Ids stay consistent inside one
// transcript (each distinct UUID maps to the same placeholder) so a replay
// still correlates frames.
import os from "node:os";

export const WORKSPACE = "C:\\fixture-workspace";
const MAX_STRING = 6000;

const IDENTITY_KEYS = new Set([
  "email",
  "emailAddress",
  "account_uuid",
  "accountUuid",
  "account_id",
  "accountId",
  "organization",
  "organization_uuid",
  "organizationUuid",
  "organization_id",
  "organizationId",
  "organization_name",
  "organizationName",
  "org_id",
  "orgId",
  "user_id",
  "userId",
  "display_name",
  "displayName",
  "subscription_id",
  "chatgpt_account_id",
  "chatgptAccountId",
]);
const SECRET = /\b(?:sk-ant-[\w-]{8,}|sk-[A-Za-z0-9_-]{20,}|gh[pousr]_[A-Za-z0-9]{20,}|xox[abpr]-[\w-]{10,}|eyJ[\w-]{10,}\.[\w-]{10,}\.[\w-]{10,})/g;
const EMAIL = /[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g;
const UUID = /\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/gi;

function escapeRegExp(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

// Each spelling a path takes on the wire, with the placeholder it becomes:
// native, forward slashes, the drive-less POSIX form a Git Bash tool prints,
// and the dashed project slug Claude Code names its session folders with.
function spellings(path, label) {
  const native = path.replace(/[\\/]+$/, "");
  const forward = native.replace(/\\/g, "/");
  const back = `${WORKSPACE}\\${label}`;
  const slash = back.replace(/\\/g, "/");
  const out = new Map([
    [native, back],
    // Inside JSON a string carries (a streamed tool input), each backslash
    // is doubled.
    [native.replace(/\\/g, "\\\\"), back.replace(/\\/g, "\\\\")],
    [forward, slash],
    [native.replace(/[^A-Za-z0-9]/g, "-"), slash.replace(/[^A-Za-z0-9]/g, "-")],
    // Pi's session folder slug keeps every character but separators.
    [native.replace(/[\\/:]/g, "-"), back.replace(/[\\/:]/g, "-")],
  ]);
  const drive = /^([A-Za-z]):\//.exec(forward);
  if (drive) out.set(`/${drive[1]}${forward.slice(2)}`, slash);
  return out;
}

export function makeSanitizer({ roots }) {
  // Longest first, so a scratch repo inside the home folder maps to the repo.
  const replacements = [];
  for (const [path, label] of [...roots, [os.homedir(), "home"]]) {
    for (const [spelling, target] of spellings(path, label)) {
      replacements.push([new RegExp(escapeRegExp(spelling), "gi"), target, spelling.length]);
    }
  }
  replacements.sort((a, b) => b[2] - a[2]);
  const host = os.hostname();
  const user = os.userInfo().username;
  const uuids = new Map();

  const text = (value) => {
    let out = value;
    for (const [pattern, target] of replacements) out = out.replace(pattern, target);
    out = out.replace(SECRET, "[redacted-secret]").replace(EMAIL, "user@example.invalid");
    out = out.replace(UUID, (match) => {
      const key = match.toLowerCase();
      if (!uuids.has(key)) {
        uuids.set(key, `00000000-0000-4000-8000-${String(uuids.size + 1).padStart(12, "0")}`);
      }
      return uuids.get(key);
    });
    if (host.length > 2) out = out.replace(new RegExp(`\\b${escapeRegExp(host)}\\b`, "gi"), "fixture-host");
    if (user.length > 3) out = out.replace(new RegExp(`\\b${escapeRegExp(user)}\\b`, "g"), "fixture-user");
    if (out.length > MAX_STRING) out = `${out.slice(0, MAX_STRING)}…[truncated]`;
    return out;
  };

  const walk = (value, key) => {
    // A process id changes every run and says nothing about the protocol.
    if (key === "pid" && typeof value === "number") return 0;
    if (typeof value === "string") {
      return IDENTITY_KEYS.has(key) ? "redacted" : text(value);
    }
    if (Array.isArray(value)) return value.map((entry) => walk(entry, key));
    if (value && typeof value === "object") {
      return Object.fromEntries(
        Object.entries(value).map(([name, entry]) => [text(name), walk(entry, name)]),
      );
    }
    return value;
  };

  return { record: (record) => walk(record, ""), records: (list) => joinSplitRuns(list, text).map((r) => walk(r, "")), text };
}

// The streamed piece of text a frame carries, keyed by the stream it extends:
// a stream-json content block delta, a JSON-RPC `.../delta` notification, or a
// Pi message update.
function deltaSlot(frame) {
  const event = frame?.event;
  if (event?.type === "content_block_delta" && event.delta) {
    const field = ["text", "thinking", "partial_json"].find((name) => typeof event.delta[name] === "string");
    if (field) return { stream: `${frame.parent_tool_use_id}:${event.index}:${field}`, holder: event.delta, field };
  }
  if (typeof frame?.method === "string" && /[dD]elta$/.test(frame.method) && typeof frame.params?.delta === "string") {
    return { stream: `${frame.method}:${frame.params.itemId}`, holder: frame.params, field: "delta" };
  }
  const update = frame?.assistantMessageEvent;
  if (frame?.type === "message_update" && typeof update?.delta === "string") {
    return { stream: `${update.type}:${update.contentIndex}`, holder: update, field: "delta" };
  }
  return null;
}

// A path or name split across streamed pieces escapes a per-string scrub, so
// each run of one stream's pieces is scrubbed joined. A run the scrub changes
// becomes one frame carrying the scrubbed whole; any other run is untouched.
function joinSplitRuns(records, text) {
  const out = [];
  let run = [];
  const flush = () => {
    if (run.length > 1) {
      const slots = run.map((record) => deltaSlot(record.frame));
      const joined = slots.map((slot) => slot.holder[slot.field]).join("");
      const scrubbed = text(joined);
      if (scrubbed !== joined) {
        const first = structuredClone(run[0]);
        const slot = deltaSlot(first.frame);
        slot.holder[slot.field] = joined;
        out.push(first);
        run = [];
        return;
      }
    }
    out.push(...run);
    run = [];
  };
  for (const record of records) {
    const slot = record.direction === "in" ? deltaSlot(record.frame) : null;
    const current = run.length ? deltaSlot(run[0].frame).stream : null;
    if (!slot || slot.stream !== current) flush();
    if (slot) run.push(record);
    else out.push(record);
  }
  flush();
  return out;
}
