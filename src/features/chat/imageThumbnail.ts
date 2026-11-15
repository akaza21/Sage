import type { ImageAttachedContext } from "../../domain/attachedContext";

// Twice the 24px chip tile, so the preview stays sharp on high-DPI screens.
export const IMAGE_THUMBNAIL_EDGE_PX = 48;
// The decode is bounded on its long edge, not its short one, so an extreme
// aspect ratio cannot blow the intermediate bitmap up past this square.
const DECODE_EDGE_PX = IMAGE_THUMBNAIL_EDGE_PX * 8;

export async function createImageThumbnail(
  file: Blob,
  image: Pick<ImageAttachedContext, "width" | "height">,
): Promise<Blob | null> {
  const edge = IMAGE_THUMBNAIL_EDGE_PX;
  const longEdge = Math.min(
    DECODE_EDGE_PX,
    Math.max(image.width, image.height),
  );
  let bitmap: ImageBitmap | undefined;
  try {
    bitmap = await createImageBitmap(file, {
      ...(image.width >= image.height
        ? { resizeWidth: longEdge }
        : { resizeHeight: longEdge }),
      resizeQuality: "high",
    });
    const canvas = document.createElement("canvas");
    canvas.width = edge;
    canvas.height = edge;
    const context = canvas.getContext("2d");
    if (!context) return null;
    context.imageSmoothingQuality = "high";
    const side = Math.min(bitmap.width, bitmap.height);
    context.drawImage(
      bitmap,
      (bitmap.width - side) / 2,
      (bitmap.height - side) / 2,
      side,
      side,
      0,
      0,
      edge,
      edge,
    );
    return await new Promise<Blob | null>((resolve) =>
      canvas.toBlob(resolve, "image/png"),
    );
  } catch {
    return null;
  } finally {
    bitmap?.close();
  }
}
