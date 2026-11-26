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

// Each spelling a Windows or POSIX path takes on the wire: native, forward
// slashes, and the drive-less POSIX form a Git Bash tool prints.
function spellings(path) {
  const native = path.replace(/[\\/]+$/, "");
  const forward = native.replace(/\\/g, "/");
  const out = new Set([native, forward]);
  const drive = /^([A-Za-z]):\//.exec(forward);
  if (drive) {
    const rest = forward.slice(2);
    out.add(`/${drive[1].toLowerCase()}${rest}`);
    out.add(`/${drive[1].toUpperCase()}${rest}`);
    out.add(`${drive[1].toLowerCase()}:${native.slice(2)}`);
    out.add(`${drive[1].toLowerCase()}:${rest}`);
  }
  return [...out].sort((a, b) => b.length - a.length);
}

export function makeSanitizer({ roots }) {
  // Longest first, so a scratch repo inside the home folder maps to the repo.
  const replacements = [];
  const home = os.homedir();
  for (const [path, label] of [...roots, [home, "home"]]) {
    for (const spelling of spellings(path)) {
      const target = spelling.includes("\\") || /^[A-Za-z]:/.test(spelling)
        ? `${WORKSPACE}\\${label}`
        : `${WORKSPACE.replace(/\\/g, "/")}/${label}`;
      replacements.push([new RegExp(escapeRegExp(spelling), "gi"), target]);
    }
  }
  replacements.sort((a, b) => b[0].source.length - a[0].source.length);
  const host = os.hostname();
  const user = os.userInfo().username;
  const uuids = new Map();

  const text = (value) => {
    let out = value;
    for (const [pattern, target] of replacements) {
      out = out.replace(pattern, (match) =>
        match.includes("/") && !match.includes("\\") ? target.replace(/\\/g, "/") : target,
      );
    }
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

  return { record: (record) => walk(record, ""), text };
}
