// Twice the 24px chip tile, so the preview stays sharp on high-DPI screens.
export const IMAGE_THUMBNAIL_EDGE_PX = 48;

// An <img> reports its size after any EXIF rotation and without decoding the
// pixels, so the crop below is exact and the only bitmap allocated is the
// thumbnail itself, whatever the source's orientation or aspect ratio.
async function loadImage(file: Blob): Promise<HTMLImageElement> {
  const url = URL.createObjectURL(file);
  try {
    const image = new Image();
    await new Promise((resolve, reject) => {
      image.onload = resolve;
      image.onerror = reject;
      image.src = url;
    });
    return image;
  } finally {
    URL.revokeObjectURL(url);
  }
}

export async function createImageThumbnail(file: Blob): Promise<Blob | null> {
  const edge = IMAGE_THUMBNAIL_EDGE_PX;
  let bitmap: ImageBitmap | undefined;
  try {
    const image = await loadImage(file);
    const side = Math.min(image.naturalWidth, image.naturalHeight);
    bitmap = await createImageBitmap(
      image,
      (image.naturalWidth - side) / 2,
      (image.naturalHeight - side) / 2,
      side,
      side,
      { resizeWidth: edge, resizeHeight: edge, resizeQuality: "high" },
    );
    const canvas = document.createElement("canvas");
    canvas.width = edge;
    canvas.height = edge;
    canvas.getContext("2d")!.drawImage(bitmap, 0, 0);
    return await new Promise<Blob | null>((resolve) =>
      canvas.toBlob(resolve, "image/png"),
    );
  } catch {
    return null;
  } finally {
    bitmap?.close();
  }
}
