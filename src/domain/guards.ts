import { utf8ByteLength } from "./text";

export const MAX_DECIMAL_U64 = "9223372036854775807";

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

export function safeOrdinal(value: unknown): number | null {
  return Number.isSafeInteger(value) && (value as number) >= 0
    ? (value as number)
    : null;
}

export function isShortId(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    value.length <= 512 &&
    !value.includes("\0")
  );
}

export function boundedIdentifier(
  value: unknown,
  maximumBytes = 512,
): string | null {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.length > maximumBytes ||
    value.includes("\0") ||
    utf8ByteLength(value) > maximumBytes
  ) {
    return null;
  }
  return value;
}

export function decimalU64Counter(
  value: unknown,
  options: { allowZero: boolean },
): string | null {
  if (typeof value !== "string" || !/^(0|[1-9][0-9]{0,18})$/.test(value)) {
    return null;
  }
  if (value.length === 19 && value > MAX_DECIMAL_U64) return null;
  return options.allowZero || value !== "0" ? value : null;
}
