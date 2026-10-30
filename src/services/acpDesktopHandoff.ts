import type { HandoffSummary } from "../domain/acp";
import {
  boundedIdentifier,
  decimalU64Counter,
  isRecord,
} from "../domain/guards";

export function decodeHandoffs(
  value: unknown,
): readonly HandoffSummary[] | null {
  if (value === undefined) return [];
  if (!Array.isArray(value) || value.length > 32) return null;
  const result: HandoffSummary[] = [];
  const ids = new Set<string>();
  for (const entry of value) {
    if (!isRecord(entry) || !isRecord(entry.omissions)) return null;
    const id = boundedIdentifier(entry.id);
    const targetSession = boundedIdentifier(entry.targetSession);
    const turnId =
      entry.turnId === null ? null : boundedIdentifier(entry.turnId);
    const revision = decimalU64Counter(entry.revision, { allowZero: true });
    if (
      !id ||
      !targetSession ||
      (entry.turnId !== null && !turnId) ||
      !revision ||
      ids.has(id)
    )
      return null;
    if (
      !["prepared", "pending", "inline", "unknown"].includes(
        entry.state as string,
      )
    )
      return null;
    if (
      ![
        entry.carriedItems,
        entry.carriedBytes,
        ...Object.values(entry.omissions),
      ].every(
        (count) =>
          typeof count === "number" &&
          Number.isSafeInteger(count) &&
          count >= 0,
      )
    )
      return null;
    if (
      (entry.carriedItems as number) > 512 ||
      (entry.carriedBytes as number) > 64_000
    )
      return null;
    const { unsupportedKind, oversize, scanLimit, budget } = entry.omissions;
    if (
      [unsupportedKind, oversize, scanLimit, budget].some(
        (count) => typeof count !== "number",
      )
    )
      return null;
    ids.add(id);
    result.push({
      id,
      targetSession,
      turnId: turnId ?? null,
      revision,
      state: entry.state as HandoffSummary["state"],
      carriedItems: entry.carriedItems as number,
      carriedBytes: entry.carriedBytes as number,
      omissions: {
        unsupportedKind,
        oversize,
        scanLimit,
        budget,
      } as HandoffSummary["omissions"],
    });
  }
  return result;
}
