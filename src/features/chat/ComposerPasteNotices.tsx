/**
 * The paste part's notices, under the one id the field names in
 * aria-describedby: transient notices in a polite live region, and apart
 * from it the standing reason a send is held. Each is empty while it has
 * nothing to say.
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
