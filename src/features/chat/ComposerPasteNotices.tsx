import type { StackEntry } from "../../ui/NoticeStack";

export interface PasteNotices {
  /** Announced: a paste or pick just staged, refused or held back. */
  notice: string;
  /** Why staged text waits; read with the field, never announced. */
  reason: string;
}

/**
 * The paste part's notices in the composer's stack. The polite region stays
 * mounted while the part is armed, so a new notice is announced in place.
 */
export function pasteNotices(
  id: string,
  armed: boolean,
  { notice, reason }: PasteNotices = { notice: "", reason: "" },
): StackEntry {
  return (
    armed && {
      key: id,
      severity: "info",
      quiet: !notice && !reason,
      node: (
        <div id={id}>
          <div
            className={notice ? "composer__disabled-reason" : undefined}
            aria-live="polite"
          >
            {notice}
          </div>
          <div className={reason ? "composer__disabled-reason" : undefined}>
            {reason}
          </div>
        </div>
      ),
    }
  );
}
