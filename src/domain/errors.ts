import { truncateUtf8 } from "./text";

export type ErrorBoundUnit = "utf16" | "utf8";

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function boundedError(
  error: unknown,
  maximum: number,
  unit: ErrorBoundUnit,
): string {
  const message = errorMessage(error);
  return unit === "utf16"
    ? message.slice(0, maximum)
    : truncateUtf8(message, maximum);
}
