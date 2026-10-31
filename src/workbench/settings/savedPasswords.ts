import {
  passwordsImportCsv,
  passwordsOverview,
  passwordsReveal,
} from "../../ports/nativePasswordsPort";

export interface SavedPassword {
  id: string;
  origin: string;
  username: string;
}

export type PasswordsOverview =
  | {
      status: "ready";
      total: number;
      matching: number;
      entries: SavedPassword[];
    }
  | { status: "unsupported" };

export interface PasswordImportReport {
  added: number;
  updated: number;
  unchanged: number;
  skipped: number;
  fileName: string;
}

/** How long a revealed password stays on screen. */
export const REVEAL_MS = 30_000;
/** Rust returns at most this many rows; search narrows the rest. */
export const MAX_LISTED = 200;

function record(input: unknown): Record<string, unknown> {
  return typeof input === "object" && input !== null
    ? (input as Record<string, unknown>)
    : {};
}

function count(value: unknown): number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0
    ? value
    : 0;
}

function admitEntry(input: unknown): SavedPassword | null {
  const { id, origin, username } = record(input);
  return typeof id === "string" &&
    typeof origin === "string" &&
    typeof username === "string"
    ? { id, origin, username }
    : null;
}

export function admitOverview(input: unknown): PasswordsOverview {
  const source = record(input);
  if (source.status !== "ready") return { status: "unsupported" };
  const entries = Array.isArray(source.entries)
    ? source.entries
        .slice(0, MAX_LISTED)
        .map(admitEntry)
        .filter((entry): entry is SavedPassword => entry !== null)
    : [];
  return {
    status: "ready",
    total: count(source.total),
    matching: count(source.matching),
    entries,
  };
}

export function admitImportReport(input: unknown): PasswordImportReport | null {
  if (input === null) return null;
  const source = record(input);
  return {
    added: count(source.added),
    updated: count(source.updated),
    unchanged: count(source.unchanged),
    skipped: count(source.skipped),
    fileName: typeof source.fileName === "string" ? source.fileName : "",
  };
}

export async function loadPasswords(query: string) {
  return admitOverview(await passwordsOverview(query));
}

export async function importPasswordsCsv() {
  return admitImportReport(await passwordsImportCsv());
}

export async function revealPassword(id: string): Promise<string> {
  const password = await passwordsReveal(id);
  if (typeof password !== "string")
    throw new Error("Sage could not show that password.");
  return password;
}

/** The site as people recognise it: the host for HTTPS, the whole origin
 * otherwise so an insecure site stays visibly insecure. */
export function siteLabel(origin: string): string {
  try {
    const url = new URL(origin);
    return url.protocol === "https:" ? url.host : origin;
  } catch {
    return origin;
  }
}

export function importSummary(report: PasswordImportReport): string {
  const parts = [`${report.added} added`];
  if (report.updated) parts.push(`${report.updated} updated`);
  if (report.unchanged) parts.push(`${report.unchanged} already saved`);
  if (report.skipped) parts.push(`${report.skipped} skipped`);
  return `Imported ${report.fileName || "the file"}: ${parts.join(", ")}.`;
}

export function errorText(cause: unknown): string {
  return String(cause instanceof Error ? cause.message : cause).slice(0, 512);
}
