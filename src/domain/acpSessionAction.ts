import { isRecord } from "./guards";
import { utf8ByteLength } from "./text";

/** What a review covers, as Codex's `review/start` names its targets. */
export type AcpReviewTarget =
  | { kind: "uncommittedChanges" }
  | { kind: "baseBranch"; branch: string }
  | { kind: "commit"; sha: string }
  | { kind: "custom"; instructions: string };

/** Runs as a Turn on an idle session that negotiated it. */
export type AcpSessionAction =
  { kind: "compact" } | { kind: "review"; target: AcpReviewTarget };

/** Actions the chat offers directly. */
export const sessionActions = {
  reviewChanges: { kind: "review", target: { kind: "uncommittedChanges" } },
  compact: { kind: "compact" },
} as const satisfies Record<string, AcpSessionAction>;

/**
 * A branch in the characters git ref names commonly use, as the native side
 * bounds it: no shell syntax, and it cannot read as an option or a range.
 */
const isBranch = (value: unknown): value is string =>
  typeof value === "string" &&
  value.length <= 256 &&
  /^[A-Za-z0-9_@+][A-Za-z0-9._/@+-]*$/.test(value) &&
  !value.includes("..");

/** An abbreviated or full hex commit id. */
const isSha = (value: unknown): value is string =>
  typeof value === "string" && /^[0-9A-Fa-f]{4,64}$/.test(value);

/** Instructions that `/review <instructions>` reads back as a custom review. */
const isInstructions = (value: unknown): value is string => {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    // Rust's `str::trim` and `split_whitespace`, which `\s` is not.
    /^\p{White_Space}|\p{White_Space}$/u.test(value) ||
    value.includes("\0") ||
    utf8ByteLength(value) > 16384
  )
    return false;
  const words = value.split(/\p{White_Space}+/u);
  return !(
    words.length === 2 &&
    ((words[0] === "base" && isBranch(words[1])) ||
      (words[0] === "commit" && isSha(words[1])))
  );
};

const exactKeys = (value: Record<string, unknown>, keys: string[]) =>
  Object.keys(value).length === keys.length &&
  keys.every((key) => key in value);

function decodeReviewTarget(value: unknown): AcpReviewTarget | null {
  if (!isRecord(value)) return null;
  switch (value.kind) {
    case "uncommittedChanges":
      return exactKeys(value, ["kind"]) ? { kind: "uncommittedChanges" } : null;
    case "baseBranch":
      return exactKeys(value, ["kind", "branch"]) && isBranch(value.branch)
        ? { kind: "baseBranch", branch: value.branch }
        : null;
    case "commit":
      return exactKeys(value, ["kind", "sha"]) && isSha(value.sha)
        ? { kind: "commit", sha: value.sha }
        : null;
    case "custom":
      return exactKeys(value, ["kind", "instructions"]) &&
        isInstructions(value.instructions)
        ? { kind: "custom", instructions: value.instructions }
        : null;
    default:
      return null;
  }
}

/** The action a stored Turn ran, or null for one this build cannot run. */
export function decodeSessionAction(value: unknown): AcpSessionAction | null {
  if (!isRecord(value)) return null;
  if (value.kind === "compact")
    return exactKeys(value, ["kind"]) ? { kind: "compact" } : null;
  if (value.kind !== "review" || !exactKeys(value, ["kind", "target"]))
    return null;
  const target = decodeReviewTarget(value.target);
  return target ? { kind: "review", target } : null;
}
