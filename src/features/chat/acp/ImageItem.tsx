import type { AcpItem } from "../../../domain/acp";
import { admittedImageDataUrl } from "../../../services/acpDesktopResponseImage";

type ImageProjectionItem = Extract<AcpItem, { kind: "image" }>;

export function ImageItem({ item }: { item: ImageProjectionItem }) {
  const admitted = admittedImageDataUrl(
    item.payload.mediaType,
    item.payload.dataUrl,
  );
  if (!admitted) return null;
  return (
    <figure className="acp-image">
      <img className="acp-image__src" alt="" src={admitted.dataUrl} />
    </figure>
  );
}
