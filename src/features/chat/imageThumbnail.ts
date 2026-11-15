import type { ImageAttachedContext } from "../../domain/attachedContext";

// Twice the 24px chip tile, so the preview stays sharp on high-DPI screens.
export const IMAGE_THUMBNAIL_EDGE_PX = 48;

export async function createImageThumbnail(
  file: Blob,
  image: Pick<ImageAttachedContext, "width" | "height">,
): Promise<Blob | null> {
  const edge = IMAGE_THUMBNAIL_EDGE_PX;
  const portrait = image.width <= image.height;
  let bitmap: ImageBitmap;
  try {
    bitmap = await createImageBitmap(file, {
      ...(portrait ? { resizeWidth: edge } : { resizeHeight: edge }),
      resizeQuality: "high",
    });
  } catch {
    return null;
  }
  try {
    const canvas = document.createElement("canvas");
    canvas.width = edge;
    canvas.height = edge;
    const context = canvas.getContext("2d");
    if (!context) return null;
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
  } finally {
    bitmap.close();
  }
}
