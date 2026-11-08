import {
  USER_INPUT_MAX_OPTIONS,
  USER_INPUT_MAX_QUESTIONS,
  type UserInputOption,
  type UserInputQuestion,
  type UserInputRequest,
} from "../domain/acpPermission";
import { isRecord, safeText } from "./acpDesktopGuards";

const MAX_AUTO_RESOLUTION_MS = 24 * 60 * 60 * 1000;

function safeOption(value: unknown): UserInputOption | null {
  if (!isRecord(value)) return null;
  const label = safeText(value.label, 256);
  const description = safeText(value.description, 1024);
  return label && description !== null ? { label, description } : null;
}

function safeQuestion(value: unknown): UserInputQuestion | null {
  if (!isRecord(value)) return null;
  const id = safeText(value.id, 128);
  const header = safeText(value.header, 256);
  const question = safeText(value.question, 2048);
  const allowsOther = value.allowsOther ?? false;
  const secret = value.secret ?? false;
  if (
    !id ||
    header === null ||
    question === null ||
    typeof allowsOther !== "boolean" ||
    typeof secret !== "boolean"
  ) {
    return null;
  }
  if (value.options === undefined || value.options === null) {
    return { id, header, question, allowsOther, secret };
  }
  if (
    !Array.isArray(value.options) ||
    value.options.length > USER_INPUT_MAX_OPTIONS
  ) {
    return null;
  }
  const options = value.options.map(safeOption);
  return options.every((option) => option !== null)
    ? {
        id,
        header,
        question,
        options: options as UserInputOption[],
        allowsOther,
        secret,
      }
    : null;
}

/**
 * Decodes the native question contract. `null` means the item carries no
 * questions; `undefined` means the value is malformed and the item must be
 * refused.
 */
export function safeUserInputRequest(
  value: unknown,
): UserInputRequest | null | undefined {
  if (value === undefined || value === null) return null;
  if (
    !isRecord(value) ||
    !Array.isArray(value.questions) ||
    value.questions.length === 0 ||
    value.questions.length > USER_INPUT_MAX_QUESTIONS
  ) {
    return undefined;
  }
  const questions = value.questions.map(safeQuestion);
  if (questions.some((question) => question === null)) return undefined;
  const window = value.autoResolutionMs;
  if (window === undefined || window === null) {
    return { questions: questions as UserInputQuestion[] };
  }
  return typeof window === "number" &&
    Number.isSafeInteger(window) &&
    window >= 0
    ? {
        questions: questions as UserInputQuestion[],
        autoResolutionMs: Math.min(window, MAX_AUTO_RESOLUTION_MS),
      }
    : undefined;
}
