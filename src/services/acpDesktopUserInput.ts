import {
  USER_INPUT_MAX_OPTIONS,
  USER_INPUT_MAX_QUESTIONS,
  USER_INPUT_TEXT_BYTES,
  type UserInputKind,
  type UserInputLimits,
  type UserInputOption,
  type UserInputQuestion,
  type UserInputRequest,
  type UserInputUrl,
} from "../domain/acpPermission";
import { isRecord, safeText } from "./acpDesktopGuards";

const MAX_AUTO_RESOLUTION_MS = 24 * 60 * 60 * 1000;
const KINDS = new Set<UserInputKind>([
  "text",
  "multiline",
  "number",
  "integer",
  "boolean",
]);

function safeOption(value: unknown): UserInputOption | null {
  if (!isRecord(value)) return null;
  const label = safeText(value.label, 256);
  const description = safeText(value.description, 1024);
  return label && description !== null ? { label, description } : null;
}

function safeLimits(value: unknown): UserInputLimits | null | undefined {
  if (value === undefined) return undefined;
  if (!isRecord(value)) return null;
  for (const [key, limit] of Object.entries(value)) {
    const valid = /^m(in|ax)(Length|Items)$/u.test(key)
      ? Number.isSafeInteger(limit) && (limit as number) >= 0
      : /^m(in|ax)imum$/u.test(key) && Number.isFinite(limit);
    if (!valid) return null;
  }
  return value as UserInputLimits;
}

const flag = (value: unknown) =>
  value === undefined ? false : typeof value === "boolean" ? value : null;

function safeQuestion(value: unknown): UserInputQuestion | null {
  if (!isRecord(value)) return null;
  const id = safeText(value.id, 128);
  const header = safeText(value.header, 256);
  const question = safeText(value.question, 2048);
  const kind = (value.kind ?? "text") as UserInputKind;
  const flags = [
    flag(value.allowsOther),
    flag(value.secret),
    flag(value.multiSelect),
    flag(value.optional),
  ];
  const initialText =
    value.initialText === undefined
      ? undefined
      : safeText(value.initialText, USER_INPUT_TEXT_BYTES);
  const limits = safeLimits(value.limits);
  if (
    !id ||
    header === null ||
    question === null ||
    !KINDS.has(kind) ||
    flags.includes(null) ||
    initialText === null ||
    limits === null
  ) {
    return null;
  }
  const [allowsOther, secret, multiSelect, optional] = flags as boolean[];
  const base: UserInputQuestion = {
    id,
    header,
    question,
    allowsOther,
    secret,
    kind,
    multiSelect,
    optional,
    ...(initialText === undefined ? {} : { initialText }),
    ...(limits === undefined ? {} : { limits }),
  };
  if (value.options === undefined || value.options === null) return base;
  if (
    !Array.isArray(value.options) ||
    value.options.length > USER_INPUT_MAX_OPTIONS
  ) {
    return null;
  }
  const options = value.options.map(safeOption);
  return options.every((option) => option !== null)
    ? { ...base, options: options as UserInputOption[] }
    : null;
}

function safeUrl(value: unknown): UserInputUrl | null | undefined {
  if (value === undefined || value === null) return null;
  if (!isRecord(value)) return undefined;
  const url = safeText(value.url, 2048);
  const host = safeText(value.host, 256);
  // The native side sends the normalized link, which is printable ASCII.
  return url && host && /^https?:\/\/[\x21-\x7e]+$/iu.test(url)
    ? { url, host }
    : undefined;
}

/**
 * Decodes the native question contract. `null` means the item carries no
 * questions; `undefined` means the value is malformed and the item must be
 * refused. A link request carries no questions; any other carries some.
 */
export function safeUserInputRequest(
  value: unknown,
): UserInputRequest | null | undefined {
  if (value === undefined || value === null) return null;
  if (
    !isRecord(value) ||
    !Array.isArray(value.questions) ||
    value.questions.length > USER_INPUT_MAX_QUESTIONS
  ) {
    return undefined;
  }
  const url = safeUrl(value.url);
  if (url === undefined || (url === null) === (value.questions.length === 0))
    return undefined;
  const questions = value.questions.map(safeQuestion);
  if (questions.some((question) => question === null)) return undefined;
  const request: UserInputRequest = {
    questions: questions as UserInputQuestion[],
    ...(url ? { url } : {}),
  };
  const window = value.autoResolutionMs;
  if (window === undefined || window === null) return request;
  return typeof window === "number" &&
    Number.isSafeInteger(window) &&
    window >= 0
    ? { ...request, autoResolutionMs: Math.min(window, MAX_AUTO_RESOLUTION_MS) }
    : undefined;
}
