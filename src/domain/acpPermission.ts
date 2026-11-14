export type PermissionDecision =
  | "ask"
  | "allowOnce"
  | "allowAlways"
  | "allowForOriginSession"
  | "allowDeveloperMode"
  | "deny";

export type PermissionState =
  | "received"
  | "asked"
  | "allowPending"
  | "allowCommitted"
  | "declinePending"
  | "declineCommitted"
  | "responseQueued"
  | "resolvedAllowed"
  | "resolvedDeclined"
  | "cancelled";

export interface ResolvePermissionDecisionRequest {
  projectId: string;
  chatId: string;
  sessionId: string;
  turnId: string;
  runtimeEpoch: string;
  requestId: string;
  decision: PermissionDecision;
}

/** Bounds of the harness-neutral question contract (native caps). */
export const USER_INPUT_MAX_QUESTIONS = 8;
export const USER_INPUT_MAX_OPTIONS = 8;
/** Longest answer or prefilled text, in UTF-8 bytes. */
export const USER_INPUT_TEXT_BYTES = 16_384;

export interface UserInputOption {
  label: string;
  description: string;
}

export type UserInputKind =
  "text" | "multiline" | "number" | "integer" | "boolean";

export interface UserInputQuestion {
  id: string;
  header: string;
  question: string;
  options?: readonly UserInputOption[];
  allowsOther: boolean;
  /** Render masked; the answer is never retained. */
  secret: boolean;
  kind: UserInputKind;
  multiSelect: boolean;
  optional: boolean;
  initialText?: string;
  limits?: UserInputLimits;
}

/**
 * Limits a form puts on an answer, each optional: text length in characters
 * (code points), an inclusive number range, and how many options a
 * multi-select answer chooses.
 */
export interface UserInputLimits {
  minLength?: number;
  maxLength?: number;
  minimum?: number;
  maximum?: number;
  minItems?: number;
  maxItems?: number;
}

export interface UserInputUrl {
  url: string;
  host: string;
}

export interface UserInputRequest {
  questions: readonly UserInputQuestion[];
  autoResolutionMs?: number;
  url?: UserInputUrl;
}

export interface UserInputAnswer {
  questionId: string;
  selectedOption?: string;
  selectedOptions?: readonly string[];
  text?: string;
}

export interface AnswerUserInputRequest {
  projectId: string;
  chatId: string;
  sessionId: string;
  turnId: string;
  runtimeEpoch: string;
  requestId: string;
  answers: readonly UserInputAnswer[];
}

export interface AnswerUserInputResponse {
  projectId: string;
  chatId: string;
  sessionId: string;
  turnId: string;
  runtimeEpoch: string;
  requestId: string;
  decision: "answer";
  state: "allowPending";
}

export interface ResolvePermissionDecisionResponse {
  source?: "browser";
  projectId: string;
  chatId: string;
  sessionId: string;
  turnId: string;
  runtimeEpoch: string;
  requestId: string;
  decision: PermissionDecision;
  state: PermissionState;
}
