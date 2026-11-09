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

const TARGETS = ["uncommittedChanges", "baseBranch", "commit", "custom"];

/**
 * The action a stored Turn ran, or null for one this build does not run.
 * The host validates each target's fields when it stores the action.
 */
export function decodeSessionAction(value: unknown): AcpSessionAction | null {
  return isRecord(value) &&
    (value.kind === "compact" ||
      (value.kind === "review" &&
        isRecord(value.target) &&
        TARGETS.includes(value.target.kind as string)))
    ? (value as AcpSessionAction)
    : null;
}
