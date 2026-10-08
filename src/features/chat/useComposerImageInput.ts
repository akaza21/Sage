import { useState, type DragEvent, type ClipboardEvent } from "react";
import type { AttachedContext } from "../../domain/attachedContext";

export const imageDisabledReason =
  "Image input is unavailable for the selected binding.";

export function useComposerImageInput({
  contexts,
  available,
  disabled,
  onAddImages,
}: {
  contexts: readonly AttachedContext[];
  available: boolean;
  disabled: boolean;
  onAddImages?: (files: File[]) => void;
}) {
  const [error, setError] = useState<string | null>(null);
  const count = contexts.filter((context) => context.kind === "image").length;
  const sendBlocked = count > 0 && !available;
  const accept = (files: File[]) => {
    if (files.length === 0) return;
    if (!available || disabled) return setError(imageDisabledReason);
    if (count + files.length > 4)
      return setError("A message can include at most four images.");
    if (
      files.some(
        (file) =>
          !["image/png", "image/jpeg"].includes(file.type) ||
          file.size === 0 ||
          file.size > 4 * 1024 * 1024,
      )
    )
      return setError("Choose PNG or JPEG images no larger than 4 MiB each.");
    setError(null);
    onAddImages?.(files);
  };
  return {
    count,
    sendBlocked,
    accept,
    onDragOver(event: DragEvent<HTMLFormElement>) {
      if (event.dataTransfer.types.includes("Files")) event.preventDefault();
    },
    onDrop(event: DragEvent<HTMLFormElement>) {
      if (!event.dataTransfer.types.includes("Files")) return;
      event.preventDefault();
      accept(Array.from(event.dataTransfer.files));
    },
    onPaste(event: ClipboardEvent<HTMLFormElement>) {
      const files = Array.from(event.clipboardData.files);
      if (files.length === 0) return;
      event.preventDefault();
      accept(files);
    },
    status:
      error ??
      (sendBlocked
        ? `${imageDisabledReason} Remove the images or select a supported binding.`
        : null),
  };
}
