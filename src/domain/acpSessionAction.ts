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

const sameShape = (value: unknown, shape: unknown): boolean =>
  isRecord(value) && isRecord(shape)
    ? Object.keys(value).length === Object.keys(shape).length &&
      Object.keys(shape).every((key) => sameShape(value[key], shape[key]))
    : value === shape;

/** The action a stored Turn ran, or null for one the chat does not offer. */
export function decodeSessionAction(value: unknown): AcpSessionAction | null {
  return (
    Object.values(sessionActions).find((action) => sameShape(value, action)) ??
    null
  );
}
