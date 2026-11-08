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
 * Why a loaded page must not be committed, judged by revision alone: it is
 * older than the page shown, or (for an earlier-history page) it was not cut
 * from the revision it was requested at. Null when the revision is fine.
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
  if (shownRevision) {
    const comparison = compareDecimalRevision(
      page.projectionRevision,
      shownRevision,
    );
    if (comparison === null) return "ignoredInvalidRevision";
    if (comparison < 0) return "ignoredStaleRevision";
  }
  if (intent.cursor && intent.expectedProjectionRevision) {
    const comparison = compareDecimalRevision(
      page.projectionRevision,
      intent.expectedProjectionRevision,
    );
    if (comparison === null) return "ignoredInvalidRevision";
    if (comparison !== 0) return "ignoredStaleRevision";
  }
  return null;
}
