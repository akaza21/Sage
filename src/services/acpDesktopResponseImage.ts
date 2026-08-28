import type { AcpImageMediaType, AcpImagePayload } from "../domain/acpImage";

const ADMITTED_MEDIA_TYPES = ["image/png", "image/jpeg", "image/webp"] as const;

export const IMAGE_DATA_URL_MAX_BYTES = 262_144;

export function admittedImageDataUrl(
  mediaType: unknown,
  dataUrl: unknown,
): AcpImagePayload | null {
  if (
    typeof mediaType !== "string" ||
    typeof dataUrl !== "string" ||
    !isAdmittedMediaType(mediaType)
  ) {
    return null;
  }
  if (
    dataUrl.length > IMAGE_DATA_URL_MAX_BYTES ||
    new TextEncoder().encode(dataUrl).byteLength > IMAGE_DATA_URL_MAX_BYTES
  ) {
    return null;
  }
  const prefix = `data:${mediaType};base64,`;
  if (!dataUrl.startsWith(prefix)) return null;
  const data = dataUrl.slice(prefix.length);
  if (!data || !isStdBase64(data)) return null;
  return { mediaType, dataUrl };
}

export function isAdmittedMediaType(value: string): value is AcpImageMediaType {
  return (ADMITTED_MEDIA_TYPES as readonly string[]).includes(value);
}

function isStdBase64(value: string): boolean {
  if (value.length % 4 !== 0) return false;
  return /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(
    value,
  );
}
