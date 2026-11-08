import { isRecord } from "./guards";

/** What a review covers. The chat offers only the uncommitted changes. */
export type AcpReviewTarget = { kind: "uncommittedChanges" };

/** Runs as a Turn on an idle session that negotiated it. */
export type AcpSessionAction =
  { kind: "compact" } | { kind: "review"; target: AcpReviewTarget };

/** Actions the chat offers directly. */
export const sessionActions = {
  reviewChanges: { kind: "review", target: { kind: "uncommittedChanges" } },
  compact: { kind: "compact" },
} as const satisfies Record<string, AcpSessionAction>;

/**
 * The action a stored Turn ran, or null for one the chat does not offer,
 * which the host projects as `{ kind: "unrecognized" }`.
 */
export function decodeSessionAction(value: unknown): AcpSessionAction | null {
  if (!isRecord(value)) return null;
  // Two keys with a record target can only be `kind` and `target`.
  const size = Object.keys(value).length;
  if (value.kind === "compact" && size === 1) return sessionActions.compact;
  const { target } = value;
  return value.kind === "review" &&
    size === 2 &&
    isRecord(target) &&
    target.kind === "uncommittedChanges" &&
    Object.keys(target).length === 1
    ? sessionActions.reviewChanges
    : null;
}
