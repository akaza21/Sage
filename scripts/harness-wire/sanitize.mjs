// Strips what a committed transcript must never carry: local paths, the
// account's identity, secrets and emails. Ids stay consistent inside one
// transcript (each distinct UUID maps to the same placeholder) so a replay
// still correlates frames.
import os from "node:os";

import { OPAQUE } from "./shape.mjs";

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
  "displayName",
  "subscription_id",
  "chatgpt_account_id",
  "chatgptAccountId",
  // The account's plan says who captured, not how the protocol behaves.
  "subscriptionType",
  "subscription_type",
  "plan_type",
]);
const SECRET =
  /\b(?:sk-ant-[\w-]{8,}|sk-[A-Za-z0-9_-]{20,}|gh[pousr]_[A-Za-z0-9]{20,}|xox[abpr]-[\w-]{10,}|eyJ[\w-]{10,}\.[\w-]{10,}\.[\w-]{10,})/g;
const EMAIL = /[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g;
// A provider's request and edge ids tie a capture to its logs and location.
const REQUEST_IDS = [
  [/\bcf-ray: ([0-9a-f]+(?:-[A-Z]{3})?)/g, "cf-ray: "],
  [/\breq_([0-9A-Za-z]{16,})/g, "req_"],
];
// A model's id or display name wherever it appears, keys and free text
// included: a family and a version, so a command such as `claude-api` is not
// one.
const MODEL_ID =
  /\b(?:claude-(?:opus|sonnet|haiku|fable)|gpt)-\d+(?:[.-][0-9a-z]+)*/g;
const MODEL_NAME =
  /\b(?:GPT-\d[\d.]*(?: [A-Z][a-z]+)?|(?:Claude )?(?:Opus|Sonnet|Haiku|Fable) \d[\d.]*)/g;
// Objects that describe the capturing account rather than the protocol, and
// inside each the only paths whose values a binding reads (`*` is any one
// key, `[]` an array element). Every other value in one becomes
// "redacted", 0 or false; model ids and names there are placeholders. A model
// descriptor anywhere is `@model`.
const WINDOW = ["usedPercent", "windowDurationMins", "resetsAt"];
const LIMITS = [
  "rateLimitReachedType",
  "spendControlReached",
  "ordinaryUsageAllowed",
  ...["primary", "secondary"].flatMap((window) =>
    WINDOW.map((field) => `${window}.${field}`),
  ),
];
const ACCOUNT_RULES = new Map(
  Object.entries({
    account: ["type"],
    organization: [],
    subscription: [],
    credits: [],
    agents: ["[].name"],
    rate_limit_info: ["status", "rateLimitType", "utilization", "resetsAt"],
    rateLimits: LIMITS,
    rateLimitsByLimitId: LIMITS.map((field) => `*.${field}`),
    rate_limits: ["*.utilization"],
    models: [
      "[].value",
      "[].supportsEffort",
      "[].supportedEffortLevels[]",
      "[].contextWindow",
      "[].isDefault",
      "[].hidden",
    ],
    "@model": [
      "isDefault",
      "hidden",
      "contextWindow",
      "defaultReasoningEffort",
      "supportedReasoningEfforts[].reasoningEffort",
    ],
    modelUsage: ["*.contextWindow"],
  }).map(([root, paths]) => [
    root,
    paths.map(
      (rule) =>
        new RegExp(
          `^${rule
            .split(".")
            .map((segment) =>
              segment === "*" ? "[^.]+" : segment.replace(/[[\]]/g, "\\$&"),
            )
            .join("\\.")}$`,
        ),
    ),
  ]),
);
// Run, task and agent ids a harness made for the capture: each gets a
// stable pseudonym, replaced wherever it is written (text and paths too).
const RUN_IDS = new Map(
  Object.entries({
    run_id: "run",
    runId: "run",
    task_id: "task",
    taskId: "task",
    agent_id: "agent",
    agentId: "agent",
  }),
);
// Command, skill and agent catalogs an account can configure: names become
// stable pseudonyms and descriptions "redacted"; a field that names one of
// them uses the same pseudonym.
const CATALOGS = new Map(
  Object.entries({
    commands: "command",
    slash_commands: "command",
    availableCommands: "command",
    skills: "skill",
    agents: "agent",
  }),
);
const CATALOG_TEXT = new Set([
  "description",
  "argumentHint",
  "argument_hint",
  "hint",
  "whenToUse",
  "when_to_use",
]);
const CATALOG_REFS = new Set([
  "command",
  "commandName",
  "skill",
  "subagent_type",
  "agentType",
  "agent_type",
]);
// Maps keyed by an account's own ids.
const DYNAMIC_KEYS = new Set(["rateLimitsByLimitId"]);
// A provider's own message, tool-use and call ids.
const PROVIDER_ID = /\b(msg|toolu|srvtoolu|call|resp)_[0-9A-Za-z]{12,}/g;
// Fields of a model descriptor, and fields elsewhere that name a model.
const MODEL_FIELDS = new Set([
  "id",
  "value",
  "model",
  "resolvedModel",
  "name",
  "description",
  "upgrade",
]);
const MODEL_KEYS = new Set([
  "model",
  "resolvedModel",
  "modelId",
  "model_id",
  "defaultModel",
  "modelSlug",
  "normalModelSlug",
]);
// Token counts and costs of the captured turns, as fixed synthetic values.
const TOKENS = /tokens(?:_delta)?$|^(?:cacheRead|cacheWrite)$/i;
const COST_KEYS = new Set(["total_cost_usd", "costUSD", "cost_usd", "total"]);
// Absolute times move to a fixed epoch, one offset per unit for the whole
// capture, so every interval between them holds.
const TIME_KEY = /(?:At|Ms|_at|_time|Time|stamp)$/;
const NOT_TIME = /duration|Mins|resetsAt|resets_at|expires/i;
const ISO_TIME = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z$/;
const TIME_BASE_MS = 1800000000000;
const TIME_BASE_S = 1800000000;
// The instant normalized usage windows reset from (2027-01-15T08:00:00Z).
const USAGE_EPOCH = 1800000000;
// When the account's stored sign-in expires.
const CREDENTIAL_EXPIRY_KEYS = new Set([
  "credential_expires_at",
  "credentialExpiresAt",
]);
const CREDENTIAL_EXPIRY = "2027-01-01T00:00:00.000Z";
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
  // A provider's message, tool-use and call ids map to stable placeholders,
  // so references between frames hold but provider records cannot be found.
  const providerIds = new Map();
  const runIds = new Map();
  const catalogNames = new Map();
  const keyNames = new Map();
  const pseudonym = (map, original, kind) => {
    if (!map.has(original))
      map.set(
        original,
        `${kind}-${[...map.values()].filter((seen) => seen.startsWith(`${kind}-`)).length + 1}`,
      );
    return map.get(original);
  };
  const requestIds = new Map();

  const text = (value) => {
    let out = value;
    for (const [pattern, target] of replacements)
      out = out.replace(pattern, target);
    out = out.replace(PIPE, "fixture-pipe");
    out = out
      .replace(SECRET, "[redacted-secret]")
      .replace(EMAIL, "user@example.invalid");
    for (const [pattern, prefix] of REQUEST_IDS)
      out = out.replace(pattern, (match, id) => {
        // Placeholders (all zeros but the count) stay as they are.
        if (/^0+\d*(?:-XXX)?$/.test(id)) return match;
        if (!requestIds.has(match)) {
          const count =
            [...requestIds.keys()].filter((seen) => seen.startsWith(prefix))
              .length + 1;
          const ray = prefix !== "req_";
          requestIds.set(
            match,
            `${prefix}${String(count).padStart(ray ? 16 : 32, "0")}${ray ? "-XXX" : ""}`,
          );
        }
        return requestIds.get(match);
      });
    out = out
      .replace(MODEL_NAME, (match) => {
        // A version that ends a sentence keeps its full stop.
        const stop = match.endsWith(".") ? "." : "";
        return (
          modelPlaceholder(
            match.slice(0, match.length - stop.length),
            "Model ",
          ) + stop
        );
      })
      .replace(MODEL_ID, (match) => modelPlaceholder(match, "model-"));
    // A model id or name a catalog taught, wherever else it is written. Only
    // a model-like string (a digit, space or hyphen in it) is matched in free
    // text, so a catalog id that is also a protocol word stays a word.
    const learned = learnedModels();
    if (learned) out = out.replace(learned, (match) => models.get(match));
    const runs = runIdPattern();
    if (runs) out = out.replace(runs, (match) => runIds.get(match));
    out = out.replace(PROVIDER_ID, (match, prefix) => {
      if (!providerIds.has(match))
        providerIds.set(
          match,
          `${prefix}_${String(providerIds.size + 1).padStart(24, "0")}`,
        );
      return providerIds.get(match);
    });
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
  let runSize = -1;
  let runPattern = null;
  function runIdPattern() {
    if (runSize === runIds.size) return runPattern;
    runSize = runIds.size;
    const ids = [...runIds.keys()]
      .sort((a, b) => b.length - a.length)
      .map(escapeRegExp);
    runPattern = ids.length
      ? new RegExp(`(?<![\\w-])(?:${ids.join("|")})(?![\\w])`, "g")
      : null;
    return runPattern;
  }
  const findRunIds = (value) => {
    if (Array.isArray(value)) value.forEach(findRunIds);
    else if (value && typeof value === "object")
      for (const [key, entry] of Object.entries(value)) {
        if (
          RUN_IDS.has(key) &&
          typeof entry === "string" &&
          entry.length >= 6 &&
          !/^(?:run|task|agent)-\d+$/.test(entry)
        )
          pseudonym(runIds, entry, RUN_IDS.get(key));
        else findRunIds(entry);
      }
  };

  // One pattern for every model-like id or name learned so far, rebuilt
  // when the catalog grows.
  let learnedSize = -1;
  let learnedPattern = null;
  function learnedModels() {
    if (learnedSize === models.size) return learnedPattern;
    learnedSize = models.size;
    const names = [...models.keys()]
      .filter(
        (name) =>
          name.length >= 4 &&
          /[\d\s-]/.test(name) &&
          !/^(?:model-|Model )\d+$/.test(name),
      )
      .sort((a, b) => b.length - a.length)
      .map(escapeRegExp);
    learnedPattern = names.length
      ? new RegExp(`(?<![\\w-])(?:${names.join("|")})(?![\\w-])`, "g")
      : null;
    return learnedPattern;
  }

  // The placeholder for a model id or name, new on first sight.
  function modelPlaceholder(found, kind) {
    const key = kind === "model-" ? found.toLowerCase() : found;
    if (!models.has(key)) {
      modelCount += 1;
      models.set(key, `${kind}${modelCount}`);
    }
    return models.get(key);
  }
  // Claude Code's `{ value, resolvedModel, description }` too; its "default"
  // alias is protocol, not a model.
  const modelId = (value) =>
    typeof value?.id === "string"
      ? value.id
      : typeof value?.value === "string" && value.value !== "default"
        ? value.value
        : undefined;
  const isModel = (value) =>
    (typeof value?.id === "string" &&
      ((typeof value.api === "string" && typeof value.provider === "string") ||
        (typeof value.model === "string" && "displayName" in value))) ||
    (typeof value?.value === "string" &&
      typeof value.resolvedModel === "string");
  const findModels = (value) => {
    if (Array.isArray(value)) value.forEach(findModels);
    else if (value && typeof value === "object") {
      const id = modelId(value);
      if (isModel(value) && id !== undefined && !models.has(id)) {
        modelCount += 1;
        models.set(id, `model-${modelCount}`);
      }
      if (isModel(value)) {
        const n = models.get(id)?.slice(6) ?? String((modelCount += 1));
        for (const [field, label] of [
          ["model", `model-${n}`],
          ["resolvedModel", `model-${n}`],
          ["name", `Model ${n}`],
          ["displayName", `Model ${n}`],
          ["description", `Model ${n}`],
        ])
          if (
            typeof value[field] === "string" &&
            !models.has(value[field]) &&
            !/^(?:redacted|model-\d+|Model \d+)$/.test(value[field])
          )
            models.set(value[field], label);
      }
      Object.values(value).forEach(findModels);
    }
  };

  // The account's usage, as a limit notice reports it, normalized to fixed
  // values that keep their types and order: each window resets one window
  // after a fixed instant.
  const usage = (object) => {
    // Only a usage window (a limit notice's) or a credit balance.
    if (
      !("usedPercent" in object) &&
      !("utilization" in object) &&
      !("windowDurationMins" in object) &&
      !("hasCredits" in object)
    )
      return object;
    const out = { ...object };
    if (typeof out.usedPercent === "number") out.usedPercent = 10;
    if (typeof out.utilization === "number") out.utilization = 0.1;
    if (typeof out.status === "string" && "utilization" in out)
      out.status = "allowed";
    if (typeof out.resetsAt === "number")
      out.resetsAt = USAGE_EPOCH + (out.windowDurationMins ?? 0) * 60;
    return out;
  };

  // Earliest absolute time seen, per unit.
  const earliest = { ms: Infinity, s: Infinity };
  const timeUnit = (value) =>
    typeof value === "number"
      ? value >= 1e12
        ? "ms"
        : value >= 1e9
          ? "s"
          : undefined
      : typeof value === "string" && ISO_TIME.test(value)
        ? "iso"
        : undefined;
  const isTime = (key) => TIME_KEY.test(key) && !NOT_TIME.test(key);
  const findTimes = (value, key = "") => {
    if (Array.isArray(value)) value.forEach((entry) => findTimes(entry, key));
    else if (value && typeof value === "object") {
      for (const [name, entry] of Object.entries(value))
        if (!OPAQUE.has(name)) findTimes(entry, name);
    } else if (isTime(key)) {
      const unit = timeUnit(value);
      if (unit === "s") earliest.s = Math.min(earliest.s, value);
      if (unit === "ms") earliest.ms = Math.min(earliest.ms, value);
      if (unit === "iso")
        earliest.ms = Math.min(earliest.ms, Date.parse(value));
    }
  };
  const shifted = (value) => {
    const unit = timeUnit(value);
    if (unit === "s") return TIME_BASE_S + (value - earliest.s);
    if (unit === "ms") return TIME_BASE_MS + (value - earliest.ms);
    if (unit === "iso")
      return new Date(
        TIME_BASE_MS + (Date.parse(value) - earliest.ms),
      ).toISOString();
    return value;
  };

  // Inside an account object only the paths its rules name keep their value.
  const kept = (value, account) => {
    if (!account) return value;
    const at = account.path.join(".");
    if ((ACCOUNT_RULES.get(account.root) ?? []).some((rule) => rule.test(at)))
      return value;
    if (typeof value === "string")
      return /^(?:redacted|model-\d+|Model \d+)$/.test(value)
        ? value
        : "redacted";
    if (typeof value === "number") return 0;
    if (typeof value === "boolean") return false;
    return value;
  };

  const walk = (
    value,
    key,
    inModel = false,
    opaque = false,
    account = null,
    catalog = null,
  ) => kept(scrub(value, key, inModel, opaque, account, catalog), account);

  // A key inside an account object that is not a plain identifier, or any key
  // of a map keyed by the account's own ids, gets a stable pseudonym.
  const accountKey = (name, base) =>
    /^(?:model-|Model |key-)\d+$/.test(name) ||
    (/^[A-Za-z][A-Za-z0-9_]*$/.test(name) &&
      !(DYNAMIC_KEYS.has(base.root) && base.path.length === 0))
      ? name
      : pseudonym(keyNames, name, "key");

  const scrub = (value, key, inModel, opaque, account, catalog) => {
    // A process id changes every run and says nothing about the protocol.
    if (key === "pid" && typeof value === "number") return 0;
    if (!opaque && typeof value === "number") {
      if (TOKENS.test(key)) return 100;
      if (COST_KEYS.has(key)) return 0.01;
    }
    if (!opaque && isTime(key)) {
      const unit = timeUnit(value);
      if (unit) return shifted(value);
    }
    if (CREDENTIAL_EXPIRY_KEYS.has(key) && typeof value === "string")
      return CREDENTIAL_EXPIRY;
    if (!opaque && typeof value === "string" && catalog) {
      if (catalog.field === "name" || catalog.field === "")
        return /^(?:command|skill|agent)-\d+$/.test(value)
          ? value
          : pseudonym(catalogNames, value, catalog.kind);
      if (CATALOG_TEXT.has(catalog.field)) return "redacted";
    }
    if (
      !opaque &&
      typeof value === "string" &&
      CATALOG_REFS.has(key) &&
      catalogNames.has(value.replace(/^\//, ""))
    )
      return catalogNames.get(value.replace(/^\//, ""));
    if (typeof value === "string") {
      // A model a descriptor names that no catalog listed (an upgrade
      // target) gets its own placeholder; placeholders stay as they are.
      if (
        ((inModel && MODEL_FIELDS.has(key)) || key === "resolvedModel") &&
        value !== "default" &&
        !models.has(value) &&
        !/^(?:model-|Model )\d+$/.test(value)
      ) {
        modelCount += 1;
        models.set(value, `model-${modelCount}`);
      }
      if (
        models.has(value) &&
        ((inModel && MODEL_FIELDS.has(key)) || MODEL_KEYS.has(key))
      )
        return models.get(value);
      return IDENTITY_KEYS.has(key) ? "redacted" : text(value);
    }
    if (Array.isArray(value)) {
      // Elements are `<path>[]`; an account array starts its own path.
      const element = account
        ? {
            root: account.root,
            path: [
              ...account.path.slice(0, -1),
              `${account.path.at(-1) ?? ""}[]`,
            ],
          }
        : ACCOUNT_RULES.has(key)
          ? { root: key, path: ["[]"] }
          : null;
      return value
        .filter(
          (entry) =>
            !isSynced(entry) &&
            !(typeof entry === "string" && synced.has(entry)),
        )
        .map((entry) =>
          walk(
            entry,
            key,
            false,
            opaque,
            element,
            !opaque && CATALOGS.has(key)
              ? { kind: CATALOGS.get(key), field: "" }
              : null,
          ),
        );
    }
    if (value && typeof value === "object") {
      const model = isModel(value);
      const base =
        account ??
        (model
          ? { root: "@model", path: [] }
          : ACCOUNT_RULES.has(key)
            ? { root: key, path: [] }
            : null);
      return Object.fromEntries(
        Object.entries(opaque ? value : usage(value)).map(([name, entry]) => {
          const scrubbed = text(name);
          const renamed =
            base && !opaque ? accountKey(scrubbed, base) : scrubbed;
          return [
            renamed,
            walk(
              entry,
              name,
              model,
              opaque || OPAQUE.has(name),
              base && { root: base.root, path: [...base.path, renamed] },
              catalog && catalog.field === ""
                ? { kind: catalog.kind, field: name }
                : null,
            ),
          ];
        }),
      );
    }
    return value;
  };

  const learn = (list) =>
    list.forEach((record) => {
      findSynced(record.frame);
      findModels(record.frame);
      findTimes(record.frame);
      findRunIds(record.frame);
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
