import { useEffect, useId, useRef, useState } from "react";

import type { AcpItem } from "../../../domain/acp";
import { admittedImageDataUrl } from "../../../services/acpDesktopResponseImage";
import { ModalDialog } from "../../../ui/ModalDialog";

type ImageProjectionItem = Extract<AcpItem, { kind: "image" }>;

export function ImageItem({ item }: { item: ImageProjectionItem }) {
  const admitted = admittedImageDataUrl(
    item.payload.mediaType,
    item.payload.dataUrl,
  );
  const [openFor, setOpenFor] = useState<string | null>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const titleId = useId();

  // An image admission is owner-bound through the projection item. Clear the
  // remembered dialog identity before a changed item or URL can reappear.
  useEffect(() => {
    setOpenFor(null);
  }, [item.id, admitted?.dataUrl]);

  if (!admitted) return null;

  const open = openFor === admitted.dataUrl;
  const dismiss = () => {
    setOpenFor(null);
    requestAnimationFrame(() => triggerRef.current?.focus());
  };

  return (
    <figure className="acp-image">
      <button
        ref={triggerRef}
        type="button"
        className="acp-image__trigger"
        aria-label="Open image viewer"
        onClick={() => setOpenFor(admitted.dataUrl)}
      >
        <img
          className="acp-image__src"
          alt="Image provided by the agent"
          src={admitted.dataUrl}
        />
      </button>
      {open ? (
        <ModalDialog
          className="acp-image__dialog"
          labelledBy={titleId}
          onDismiss={dismiss}
        >
          <section className="acp-image__viewer">
            <header>
              <h2 id={titleId}>Agent image</h2>
              <button type="button" onClick={dismiss}>
                Close image viewer
              </button>
            </header>
            <img
              className="acp-image__expanded"
              alt="Image provided by the agent"
              src={admitted.dataUrl}
            />
          </section>
        </ModalDialog>
      ) : null}
    </figure>
  );
}
