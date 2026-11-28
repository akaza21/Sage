// Strips what a committed transcript must never carry: local paths, the
// account's identity, secrets and emails. Ids stay consistent inside one
// transcript (each distinct UUID maps to the same placeholder) so a replay
// still correlates frames.
import os from "node:os";

export const WORKSPACE = "C:\\fixture-workspace";
const MAX_STRING = 6000;
const SYNCED = "(claude.ai sync)";

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
  // The account's plan says who captured, not how the protocol behaves.
  "subscriptionType",
  "subscription_type",
  "planType",
  "plan_type",
]);
const SECRET =
  /\b(?:sk-ant-[\w-]{8,}|sk-[A-Za-z0-9_-]{20,}|gh[pousr]_[A-Za-z0-9]{20,}|xox[abpr]-[\w-]{10,}|eyJ[\w-]{10,}\.[\w-]{10,}\.[\w-]{10,})/g;
const EMAIL = /[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g;
// A provider's request and edge ids tie a capture to its logs and location.
const REQUEST_IDS = [
  [/\bcf-ray: [0-9a-f]+(?:-[A-Z]{3})?/g, "cf-ray: 0000000000000000-XXX"],
  [/\breq_[0-9A-Za-z]{16,}/g, "req_00000000000000000000000000000000"],
];
// Fields of a model descriptor, and fields elsewhere that name a model.
const MODEL_FIELDS = new Set(["id", "model", "name"]);
const MODEL_KEYS = new Set([
  "model",
  "modelId",
  "model_id",
  "defaultModel",
  "modelSlug",
  "normalModelSlug",
]);
// The instant normalized usage windows reset from (2027-01-15T08:00:00Z).
const USAGE_EPOCH = 1800000000;
// Bounded by any non-hex character, so an id after `_` (a Pi session file
// name) is caught too.
const UUID =
  /(?<![0-9a-f])[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}(?![0-9a-f])/gi;
// A Windows named pipe carries a per-run name.
const PIPE = /\\\\\.\\pipe\\[^\s"]+/gi;

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
  const system = process.env.SystemRoot
    ? [[process.env.SystemRoot, "windows"]]
    : [];
  for (const [path, label] of [...roots, [os.homedir(), "home"], ...system]) {
    for (const [spelling, target] of spellings(path, label)) {
      replacements.push([
        new RegExp(escapeRegExp(spelling), "gi"),
        target,
        spelling.length,
      ]);
    }
  }
  replacements.sort((a, b) => b[2] - a[2]);
  const host = os.hostname();
  const user = os.userInfo().username;
  const uuids = new Map();

  const text = (value) => {
    let out = value;
    for (const [pattern, target] of replacements)
      out = out.replace(pattern, target);
    out = out.replace(PIPE, "fixture-pipe");
    out = out
      .replace(SECRET, "[redacted-secret]")
      .replace(EMAIL, "user@example.invalid");
    for (const [pattern, target] of REQUEST_IDS)
      out = out.replace(pattern, target);
    out = out.replace(UUID, (match) => {
      const key = match.toLowerCase();
      if (!uuids.has(key)) {
        uuids.set(
          key,
          `00000000-0000-4000-8000-${String(uuids.size + 1).padStart(12, "0")}`,
        );
      }
      return uuids.get(key);
    });
    if (host.length > 2)
      out = out.replace(
        new RegExp(`\\b${escapeRegExp(host)}\\b`, "gi"),
        "fixture-host",
      );
    if (user.length > 3)
      out = out.replace(
        new RegExp(`\\b${escapeRegExp(user)}\\b`, "gi"),
        "fixture-user",
      );
    if (out.length > MAX_STRING)
      out = `${out.slice(0, MAX_STRING)}…[truncated]`;
    return out;
  };

  // Skills a signed-in account syncs from claude.ai name the account's own
  // skills and arrive whenever the sync lands, so they are left out wherever
  // they are listed, by entry or by name.
  const synced = new Set();
  const isSynced = (entry) =>
    entry &&
    typeof entry === "object" &&
    typeof entry.description === "string" &&
    entry.description.endsWith(SYNCED);
  const findSynced = (value) => {
    if (Array.isArray(value)) value.forEach(findSynced);
    else if (value && typeof value === "object") {
      if (isSynced(value) && typeof value.name === "string")
        synced.add(value.name);
      Object.values(value).forEach(findSynced);
    }
  };

  // The models an account can use say what it is entitled to, so each model
  // descriptor in a catalog (Pi's `{ id, api, provider }`, Codex's
  // `{ id, model, displayName }`) maps its id, model and name to a
  // placeholder. Only those descriptor fields and fields that name a model
  // are replaced, so a protocol string that happens to equal a model id
  // (`text`) is never touched.
  const models = new Map();
  let modelCount = 0;
  const isModel = (value) =>
    typeof value?.id === "string" &&
    ((typeof value.api === "string" && typeof value.provider === "string") ||
      (typeof value.model === "string" && "displayName" in value));
  const findModels = (value) => {
    if (Array.isArray(value)) value.forEach(findModels);
    else if (value && typeof value === "object") {
      if (isModel(value) && !models.has(value.id)) {
        modelCount += 1;
        models.set(value.id, `model-${modelCount}`);
        for (const [field, label] of [
          ["model", `model-${modelCount}`],
          ["name", `Model ${modelCount}`],
        ])
          if (typeof value[field] === "string" && !models.has(value[field]))
            models.set(value[field], label);
      }
      Object.values(value).forEach(findModels);
    }
  };

  // The account's usage, as a limit notice reports it, normalized to fixed
  // values that keep their types and order: each window resets one window
  // after a fixed instant.
  const usage = (object) => {
    const out = { ...object };
    if (typeof out.usedPercent === "number") out.usedPercent = 10;
    if (typeof out.utilization === "number") out.utilization = 0.1;
    if (typeof out.resetsAt === "number")
      out.resetsAt = USAGE_EPOCH + (out.windowDurationMins ?? 0) * 60;
    if (typeof out.balance === "string") out.balance = "0";
    return out;
  };

  const walk = (value, key, inModel = false) => {
    // A process id changes every run and says nothing about the protocol.
    if (key === "pid" && typeof value === "number") return 0;
    if (typeof value === "string") {
      if (
        models.has(value) &&
        ((inModel && MODEL_FIELDS.has(key)) || MODEL_KEYS.has(key))
      )
        return models.get(value);
      return IDENTITY_KEYS.has(key) ? "redacted" : text(value);
    }
    if (Array.isArray(value)) {
      return value
        .filter(
          (entry) =>
            !isSynced(entry) &&
            !(typeof entry === "string" && synced.has(entry)),
        )
        .map((entry) => walk(entry, key));
    }
    if (value && typeof value === "object") {
      const model = isModel(value);
      return Object.fromEntries(
        Object.entries(usage(value)).map(([name, entry]) => [
          text(name),
          walk(entry, name, model),
        ]),
      );
    }
    return value;
  };

  const learn = (list) =>
    list.forEach((record) => {
      findSynced(record.frame);
      findModels(record.frame);
    });

  return {
    learn,
    record: (record) => walk(record, ""),
    records: (list) => {
      learn(list);
      return joinSplitRuns(list, text).map((r) => walk(r, ""));
    },
    text,
  };
}

// The streamed piece of text a frame carries, keyed by the stream it extends:
// a stream-json content block delta, a JSON-RPC `.../delta` notification, or a
// Pi message update.
function deltaSlot(frame) {
  const event = frame?.event;
  if (event?.type === "content_block_delta" && event.delta) {
    const field = ["text", "thinking", "partial_json"].find(
      (name) => typeof event.delta[name] === "string",
    );
    if (field)
      return {
        stream: `${frame.api_message_id}:${frame.parent_tool_use_id}:${event.index}:${field}`,
        holder: event.delta,
        field,
      };
  }
  if (
    typeof frame?.method === "string" &&
    /[dD]elta$/.test(frame.method) &&
    typeof frame.params?.delta === "string"
  ) {
    return {
      stream: `${frame.method}:${frame.params.itemId}`,
      holder: frame.params,
      field: "delta",
    };
  }
  const update = frame?.assistantMessageEvent;
  if (frame?.type === "message_update" && typeof update?.delta === "string") {
    return {
      stream: `${update.type}:${update.contentIndex}`,
      holder: update,
      field: "delta",
    };
  }
  return null;
}

// A path or name split across streamed pieces escapes a per-string scrub, so
// all pieces of one stream are scrubbed joined, even when other frames come
// between them. A stream the scrub changes, or one that names a drive path,
// becomes one frame (at its first piece) carrying the whole; any other stream
// is untouched.
function joinSplitRuns(records, text) {
  const streams = new Map();
  let messages = 0;
  records.forEach((record, at) => {
    // Content is numbered within a message, so a message start (Pi's, or a
    // stream-json stream event's) opens new streams.
    if (
      record.frame?.type === "message_start" ||
      record.frame?.event?.type === "message_start"
    ) {
      messages += 1;
    }
    const slot = record.direction === "in" ? deltaSlot(record.frame) : null;
    if (!slot) return;
    const key = `${messages}:${slot.stream}`;
    if (!streams.has(key)) streams.set(key, []);
    streams.get(key).push(at);
  });
  const replaced = new Map();
  const dropped = new Set();
  for (const indices of streams.values()) {
    if (indices.length < 2) continue;
    const joined = indices
      .map((at) => {
        const slot = deltaSlot(records[at].frame);
        return slot.holder[slot.field];
      })
      .join("");
    if (text(joined) === joined && !/[A-Za-z]:(?:\\|\/)/.test(joined)) continue;
    const first = structuredClone(records[indices[0]]);
    const slot = deltaSlot(first.frame);
    slot.holder[slot.field] = joined;
    replaced.set(indices[0], first);
    indices.slice(1).forEach((at) => dropped.add(at));
  }
  return records
    .map((record, at) => replaced.get(at) ?? record)
    .filter((_, at) => !dropped.has(at));
}
