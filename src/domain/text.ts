const encoder = new TextEncoder();
const decoder = new TextDecoder();

export function capitalizeLabel(value: string): string {
  const first = [...value][0] ?? "";
  return first.toLocaleUpperCase() + value.slice(first.length);
}

export function utf8Bytes(value: string): Uint8Array {
  return encoder.encode(value);
}

export function utf8ByteLength(value: string): number {
  return utf8Bytes(value).byteLength;
}

export function truncateUtf8(value: string, maximumBytes: number): string {
  const bytes = utf8Bytes(value);
  if (bytes.byteLength <= maximumBytes) return value;
  let end = maximumBytes;
  while (end > 0 && (bytes[end] & 0xc0) === 0x80) end -= 1;
  return decoder.decode(bytes.slice(0, end));
}
