/** The ids of the paste notices, which the field names in aria-describedby. */
export interface PasteNoticeIds {
  /** Transient notices, in a polite live region. */
  notice: string;
  /** The standing reason a send is held, apart from the live notices. */
  reason: string;
}

/**
 * Both notice elements, always present while the paste part is (loading,
 * loaded or failed), so the field never names a missing element; each is
 * empty while it has nothing to say.
 */
export function ComposerPasteNotices({
  ids,
  notice = "",
  reason = "",
}: {
  ids: PasteNoticeIds;
  notice?: string;
  reason?: string;
}) {
  return (
    <>
      <div
        id={ids.notice}
        className={notice ? "composer__disabled-reason" : undefined}
        aria-live="polite"
      >
        {notice}
      </div>
      <div
        id={ids.reason}
        className={reason ? "composer__disabled-reason" : undefined}
      >
        {reason}
      </div>
    </>
  );
}
