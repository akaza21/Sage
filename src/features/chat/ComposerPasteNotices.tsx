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
