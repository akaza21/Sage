import type { AcpChatPage } from "../domain/acp";
import { compareDecimalRevision } from "./acpProjectionMerge";
import type {
  AcpPageLoadIntent,
  ProjectionCommitDisposition,
} from "./acpProjectionStore";

export function loadSlot(
  intent: AcpPageLoadIntent,
): "activeLoad" | "historyLoad" {
  return intent.cursor ? "historyLoad" : "activeLoad";
}

export function errorField(
  intent: AcpPageLoadIntent,
): "error" | "historyError" {
  return intent.cursor ? "historyError" : "error";
}

/**
 * An earlier-history page is anchored by its cursor rather than the live
 * revision, so a stream moving the revision must not drop it.
 */
export function rejectedRevision(
  shown: AcpChatPage | null,
  intent: AcpPageLoadIntent,
  page: AcpChatPage,
): Extract<
  ProjectionCommitDisposition,
  "ignoredInvalidRevision" | "ignoredStaleRevision"
> | null {
  const shownRevision = shown?.projectionRevision;
  if (!shownRevision) return null;
  const comparison = compareDecimalRevision(
    page.projectionRevision,
    shownRevision,
  );
  if (comparison === null) return "ignoredInvalidRevision";
  if (comparison < 0 && !intent.cursor) return "ignoredStaleRevision";
  return null;
}
