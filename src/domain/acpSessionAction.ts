import { isRecord } from "./guards";

/** What a review covers, as Codex's `review/start` names its targets. */
export type AcpReviewTarget =
  | { kind: "uncommittedChanges" }
  | { kind: "baseBranch"; branch: string }
  | { kind: "commit"; sha: string }
  | { kind: "custom"; instructions: string };

export type AcpSessionAction =
  { kind: "compact" } | { kind: "review"; target: AcpReviewTarget };

export const sessionActions = {
  reviewChanges: { kind: "review", target: { kind: "uncommittedChanges" } },
  compact: { kind: "compact" },
} as const satisfies Record<string, AcpSessionAction>;

// The actions a stored Turn may hold; `String` stands for a target's text.
// The native side owns what a branch, commit or instructions may say and
// refuses anything else when a review runs; a stored action is only shown.
const shapes: unknown[] = [
  ...Object.values(sessionActions),
  { kind: "review", target: { kind: "baseBranch", branch: String } },
  { kind: "review", target: { kind: "commit", sha: String } },
  { kind: "review", target: { kind: "custom", instructions: String } },
];

const sameShape = (value: unknown, shape: unknown): boolean =>
  shape === String
    ? typeof value === "string" && value !== ""
    : isRecord(value) && isRecord(shape)
      ? Object.keys(value).length === Object.keys(shape).length &&
        Object.keys(shape).every((key) => sameShape(value[key], shape[key]))
      : value === shape;

export function decodeSessionAction(value: unknown): AcpSessionAction | null {
  return shapes.some((shape) => sameShape(value, shape))
    ? (value as AcpSessionAction)
    : null;
}
