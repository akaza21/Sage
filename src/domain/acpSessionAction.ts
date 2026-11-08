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

/**
 * The action a stored Turn ran. Only the chat starts actions, so anything
 * other than one it offers is malformed.
 */
export function decodeSessionAction(value: unknown): AcpSessionAction | null {
  const json = JSON.stringify(value);
  return (
    Object.values(sessionActions).find(
      (action) => JSON.stringify(action) === json,
    ) ?? null
  );
}
