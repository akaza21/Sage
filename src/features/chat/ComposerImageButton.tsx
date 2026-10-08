import { useRef } from "react";
import { Image } from "../../ui/leanIcons";
import { imageDisabledReason } from "./useComposerImageInput";

export function ComposerImageButton({
  available,
  disabled,
  uploading,
  count,
  onChoose,
}: {
  available: boolean;
  disabled: boolean;
  uploading: boolean;
  count: number;
  onChoose: (files: File[]) => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const title = available ? "Add PNG or JPEG images" : imageDisabledReason;
  return (
    <>
      <input
        ref={inputRef}
        className="sr-only"
        type="file"
        aria-label="Choose images"
        accept="image/png,image/jpeg"
        multiple
        tabIndex={-1}
        disabled={!available || disabled || uploading}
        onChange={(event) => {
          onChoose(Array.from(event.target.files ?? []));
          event.target.value = "";
        }}
      />
      <span title={title}>
        <button
          className="composer__image-button"
          type="button"
          aria-label="Add images"
          title={title}
          disabled={!available || disabled || count >= 4 || uploading}
          onClick={() => inputRef.current?.click()}
        >
          <Image size={16} aria-hidden="true" />
        </button>
      </span>
    </>
  );
}
