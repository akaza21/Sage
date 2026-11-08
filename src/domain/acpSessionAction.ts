import { isRecord } from "./guards";

/** What a review covers, as the host's `ReviewTarget` names it. */
export type AcpReviewTarget =
  | { kind: "uncommittedChanges" }
  | { kind: "baseBranch"; branch: string }
  | { kind: "commit"; sha: string; title?: string }
  | { kind: "custom"; instructions: string };

/** Runs as a Turn on an idle session that negotiated it. */
export type AcpSessionAction =
  { kind: "compact" } | { kind: "review"; target: AcpReviewTarget };

/** Actions the chat offers directly. */
export const sessionActions = {
  reviewChanges: { kind: "review", target: { kind: "uncommittedChanges" } },
  compact: { kind: "compact" },
} as const satisfies Record<string, AcpSessionAction>;

const review = (target: AcpReviewTarget) => ({ kind: "review", target });
/** Every action the host runs; "" stands for any non-empty text. */
const shapes = [
  ...Object.values(sessionActions),
  review({ kind: "baseBranch", branch: "" }),
  review({ kind: "commit", sha: "" }),
  review({ kind: "commit", sha: "", title: "" }),
  review({ kind: "custom", instructions: "" }),
];

const sameShape = (value: unknown, shape: unknown): boolean =>
  isRecord(value) && isRecord(shape)
    ? Object.keys(value).length === Object.keys(shape).length &&
      Object.keys(shape).every((key) => sameShape(value[key], shape[key]))
    : shape === ""
      ? typeof value === "string" && value !== ""
      : value === shape;

/** The action a stored Turn ran, or null for one this build does not run. */
export function decodeSessionAction(value: unknown): AcpSessionAction | null {
  return shapes.some((shape) => sameShape(value, shape))
    ? (value as AcpSessionAction)
    : null;
}
