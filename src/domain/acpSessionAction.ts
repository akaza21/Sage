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

/** One word of a review target, bounded as the native side bounds it. */
const isWord = (value: unknown, maximum: number): value is string =>
  typeof value === "string" &&
  value.length > 0 &&
  !value.startsWith("-") &&
  // As Rust's `char::is_whitespace`, which `\s` is not (it adds U+FEFF).
  !/[\p{White_Space}\p{Cc}]/u.test(value) &&
  utf8ByteLength(value) <= maximum;

const exactKeys = (value: Record<string, unknown>, keys: string[]) =>
  Object.keys(value).length === keys.length &&
  keys.every((key) => key in value);

function decodeReviewTarget(value: unknown): AcpReviewTarget | null {
  if (!isRecord(value)) return null;
  switch (value.kind) {
    case "uncommittedChanges":
      return exactKeys(value, ["kind"]) ? { kind: "uncommittedChanges" } : null;
    case "baseBranch":
      return exactKeys(value, ["kind", "branch"]) && isWord(value.branch, 256)
        ? { kind: "baseBranch", branch: value.branch }
        : null;
    case "commit":
      return exactKeys(value, ["kind", "sha"]) && isWord(value.sha, 64)
        ? { kind: "commit", sha: value.sha }
        : null;
    case "custom":
      return exactKeys(value, ["kind", "instructions"]) &&
        typeof value.instructions === "string" &&
        value.instructions.length > 0 &&
        !value.instructions.includes("\0") &&
        utf8ByteLength(value.instructions) <= 16384
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
