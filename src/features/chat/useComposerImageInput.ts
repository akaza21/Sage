import { useRef, useState, type DragEvent, type ClipboardEvent } from "react";
import type { AcpChatPage, AgentSessionProjection } from "../../domain/acp";
import {
  MAX_IMAGE_UPLOAD_BYTES,
  MAX_PROMPT_IMAGES,
  imageUploadLimitMessage,
  type AttachedContext,
} from "../../domain/attachedContext";

export const imageDisabledReason =
  "Image input is unavailable for the selected binding.";

/**
 * Images are a draft action of a native ACP binding. They follow the
 * negotiated prompt capability of the session the next send resumes,
 * whatever state it was left in, else of the binding's latest negotiated
 * session in this chat. With neither, native admission checks the session
 * that send starts, as it does for attached files.
 */
export function composerImageAvailability(
  nextSession: Pick<AgentSessionProjection, "capabilities"> | null | undefined,
  page: Pick<AcpChatPage, "sessions"> | null | undefined,
  bindingId: string | null,
): { available: boolean; reason: string | null } {
  if (!bindingId) return { available: false, reason: imageDisabledReason };
  const bindingSessions = (page?.sessions ?? []).filter(
    (session) => session.bindingId === bindingId && !session.legacy,
  );
  // A session whose activation never completed keeps placeholder capabilities.
  const negotiated = [nextSession, ...bindingSessions.reverse()].find(
    (session) => session && session.capabilities.protocolVersion !== "pending",
  );
  if (negotiated && negotiated.capabilities.promptImages !== true)
    return {
      available: false,
      reason: "This harness does not advertise image input.",
    };
  return { available: true, reason: null };
}

/** Choosing images for the draft; `reason` says why images are unavailable. */
export interface ImageAction {
  disabled: boolean;
  reason?: string | null;
  onChoose: () => void;
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
  // An error is kept with the availability it was raised under, so one
  // about images being unavailable clears once the session takes them.
  const [raised, setRaised] = useState<[string, boolean] | null>(null);
  const setError = (text: string) => setRaised([text, available]);
  // Dropped, not hidden, so it cannot return if availability flips back.
  if (raised && raised[1] !== available) setRaised(null);
  const error = raised?.[1] === available ? raised[0] : null;
  const chooserRef = useRef<HTMLInputElement>(null);
  const images = contexts.filter((context) => context.kind === "image");
  const count = images.length;
  const sendBlocked = count > 0 && !available;
  const reason = unavailableReason ?? imageDisabledReason;
  const accept = (files: File[]) => {
    if (files.length === 0) return;
    if (!available || disabled) return setError(reason);
    if (count + files.length > MAX_PROMPT_IMAGES)
      return setError(
        `A message can include at most ${MAX_PROMPT_IMAGES} images.`,
      );
    if (
      files.some(
        (file) =>
          !["image/png", "image/jpeg"].includes(file.type) ||
          file.size === 0 ||
          file.size > MAX_IMAGE_UPLOAD_BYTES,
      )
    )
      return setError(imageUploadLimitMessage);
    setRaised(null);
    onAddImages?.(files);
  };
  return {
    chooserRef,
    action: onAddImages
      ? ({
          disabled:
            !available || disabled || uploading || count >= MAX_PROMPT_IMAGES,
          reason: available ? null : reason,
          onChoose: () => chooserRef.current?.click(),
        } satisfies ImageAction)
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
