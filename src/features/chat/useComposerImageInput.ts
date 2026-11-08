import { useRef, useState, type DragEvent, type ClipboardEvent } from "react";
import type { AgentSessionProjection } from "../../domain/acp";
import {
  MAX_OUTGOING_IMAGE_BYTES,
  MAX_PROMPT_IMAGE_TOTAL_BYTES,
  MAX_PROMPT_IMAGES,
  type AttachedContext,
} from "../../domain/attachedContext";

export const imageDisabledReason =
  "Image input is unavailable for the selected binding.";

/**
 * Image input follows the selected session's negotiated ACP prompt
 * capability; a harness that has not advertised it never gets images.
 */
export function composerImageAvailability(
  useNativeAcp: boolean,
  session:
    Pick<AgentSessionProjection, "state" | "capabilities"> | null | undefined,
): { available: boolean; reason: string | null } {
  if (!useNativeAcp) return { available: false, reason: imageDisabledReason };
  if (!session || ["closed", "failed", "incompatible"].includes(session.state))
    return {
      available: false,
      reason:
        "Image input is available once this harness's session starts and advertises it. Send a message first.",
    };
  if (session.capabilities.promptImages !== true)
    return {
      available: false,
      reason: "This harness does not advertise image input.",
    };
  return { available: true, reason: null };
}

export function useComposerImageInput({
  contexts,
  available,
  unavailableReason = null,
  disabled,
  uploading = false,
  onAddImages,
}: {
  contexts: readonly AttachedContext[];
  available: boolean;
  unavailableReason?: string | null;
  disabled: boolean;
  uploading?: boolean;
  onAddImages?: (files: File[]) => void;
}) {
  const [error, setError] = useState<string | null>(null);
  const chooserRef = useRef<HTMLInputElement>(null);
  const images = contexts.filter((context) => context.kind === "image");
  const count = images.length;
  const sendBlocked = count > 0 && !available;
  const reason = unavailableReason ?? imageDisabledReason;
  const accept = (files: File[]) => {
    if (files.length === 0) return;
    if (!available || disabled) return setError(reason);
    if (count + files.length > MAX_PROMPT_IMAGES)
      return setError("A message can include at most four images.");
    if (
      files.some(
        (file) =>
          !["image/png", "image/jpeg"].includes(file.type) ||
          file.size === 0 ||
          file.size > MAX_OUTGOING_IMAGE_BYTES,
      )
    )
      return setError("Choose PNG or JPEG images no larger than 4 MiB each.");
    const total =
      images.reduce((sum, image) => sum + image.byteLength, 0) +
      files.reduce((sum, file) => sum + file.size, 0);
    if (total > MAX_PROMPT_IMAGE_TOTAL_BYTES)
      return setError(
        "Images in one message can total at most 512 KiB. Shrink or crop the image and try again.",
      );
    setError(null);
    onAddImages?.(files);
  };
  return {
    chooserRef,
    action:
      available && onAddImages
        ? {
            disabled: disabled || uploading || count >= MAX_PROMPT_IMAGES,
            onChoose: () => chooserRef.current?.click(),
          }
        : undefined,
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
        ? `${reason} Remove the images or select a supported binding.`
        : null),
  };
}
