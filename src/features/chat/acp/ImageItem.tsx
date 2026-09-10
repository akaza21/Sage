import { useId, useRef, useState } from "react";

import type { AcpItem } from "../../../domain/acp";
import { admittedImageDataUrl } from "../../../services/acpDesktopResponseImage";
import { ModalDialog } from "../../../ui/ModalDialog";

type ImageProjectionItem = Extract<AcpItem, { kind: "image" }>;
type AdmittedImage = NonNullable<ReturnType<typeof admittedImageDataUrl>>;

export function ImageItem({ item }: { item: ImageProjectionItem }) {
  const admitted = admittedImageDataUrl(
    item.payload.mediaType,
    item.payload.dataUrl,
  );
  if (!admitted) return null;

  // The key makes every projection identity transition a child unmount. Dialog
  // state can therefore never resurrect an earlier admitted image on A→B→A.
  const admissionKey = `${item.id}\u0000${admitted.dataUrl}`;
  return <AdmittedImageItem key={admissionKey} admitted={admitted} />;
}

function AdmittedImageItem({ admitted }: { admitted: AdmittedImage }) {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const titleId = useId();
  const dismiss = () => {
    setOpen(false);
    requestAnimationFrame(() => triggerRef.current?.focus());
  };

  return (
    <figure className="acp-image">
      <button
        ref={triggerRef}
        type="button"
        className="acp-image__trigger"
        aria-label="Open image viewer"
        onClick={() => setOpen(true)}
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
