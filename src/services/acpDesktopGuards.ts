import {
  boundedIdentifier as safeIdentifier,
  decimalU64Counter,
  isRecord,
  safeOrdinal,
} from "../domain/guards";
import { utf8ByteLength } from "../domain/text";

export function safeCounter(value: unknown, allowZero: boolean): string | null {
  return decimalU64Counter(value, { allowZero });
}

export { isRecord, safeIdentifier, safeOrdinal };

export function isOneOf<const Value extends string>(
  value: unknown,
  allowed: readonly Value[],
): value is Value {
  return typeof value === "string" && allowed.includes(value as Value);
}

export function safeText(value: unknown, maximumBytes: number): string | null {
  return typeof value === "string" && utf8ByteLength(value) <= maximumBytes
    ? value
    : null;
}

export function safeMessage(value: unknown): string | null {
  if (typeof value !== "string" || value.length === 0 || value.length > 8192) {
    return null;
  }
  return utf8ByteLength(value) <= 8192 ? value : null;
}

export function safeTimestamp(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value)
    ? value
    : null;
}

export function safeNullableIdentifier(
  value: unknown,
): string | null | undefined {
  return value === null ? null : (safeIdentifier(value) ?? undefined);
}

export function safeNullableCounter(
  value: unknown,
  allowZero: boolean,
): string | null | undefined {
  return value === null ? null : (safeCounter(value, allowZero) ?? undefined);
}

export function safeNullableText(
  value: unknown,
  maximumBytes: number,
): string | null | undefined {
  return value === null ? null : (safeText(value, maximumBytes) ?? undefined);
}
