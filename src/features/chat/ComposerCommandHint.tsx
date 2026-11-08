/**
 * A picked command's input hint, drawn after the typed text where a
 * placeholder would be, and read with the field's description.
 */
export function ComposerCommandHint({
  hint,
  hintId,
  text,
}: {
  hint: string | null;
  hintId: string;
  text: string;
}) {
  return hint ? (
    <>
      <div className="composer__command-hint" aria-hidden="true">
        <span className="composer__command-hint-typed">{text}</span>
        {hint}
      </div>
      <span className="sr-only" id={hintId}>
        {`${text.trim()} expects ${hint}`}
      </span>
    </>
  ) : null;
}
