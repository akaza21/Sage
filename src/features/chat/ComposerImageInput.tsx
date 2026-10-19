import type { RefObject } from "react";

export function ComposerImageInput({
  inputRef,
  disabled,
  onChoose,
}: {
  inputRef: RefObject<HTMLInputElement | null>;
  disabled: boolean;
  onChoose: (files: File[]) => void;
}) {
  return (
    <input
      ref={inputRef}
      className="sr-only"
      type="file"
      aria-label="Choose images"
      accept="image/png,image/jpeg"
      multiple
      tabIndex={-1}
      disabled={disabled}
      onChange={(event) => {
        onChoose(Array.from(event.target.files ?? []));
        event.target.value = "";
      }}
    />
  );
}
