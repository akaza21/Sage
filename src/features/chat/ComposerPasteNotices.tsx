import type { StackEntry } from "../../ui/NoticeStack";

/** What the paste part puts in the composer's notice stack. */
export type PasteNotices = StackEntry;

/**
 * The paste part's notices. The polite region stays mounted while the part
 * is, so a new notice is announced in place.
 */
export function ComposerPasteNotices({
  id,
  notice,
  reason,
}: {
  id: string;
  notice: string;
  reason: string;
}) {
  return (
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
  );
}
