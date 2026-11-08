import type { AcpChatPage } from "../domain/acp";
import { compareDecimalRevision } from "./acpProjectionMerge";
import type {
  AcpPageLoadIntent,
  ProjectionCommitDisposition,
} from "./acpProjectionStore";

/** Where a load waits: a newest-page refresh and an earlier-history page each
 * have their own slot, so neither drops the other. */
export function loadSlot(
  intent: AcpPageLoadIntent,
): "activeLoad" | "historyLoad" {
  return intent.cursor ? "historyLoad" : "activeLoad";
}

/** Where a failed load's message goes, matching its slot. */
export function errorField(
  intent: AcpPageLoadIntent,
): "error" | "historyError" {
  return intent.cursor ? "historyError" : "error";
}

/**
 * Why a loaded page must not be committed, judged by revision alone. A newest
 * page older than the page shown is stale. An earlier-history page is
 * anchored by its cursor rather than the live revision, so only a malformed
 * revision rejects it: a stream moving the revision must not drop it.
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
