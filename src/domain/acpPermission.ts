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

/** How an answer is entered; choices are typed by the native side. */
export type UserInputKind =
  "text" | "multiline" | "number" | "integer" | "boolean";

export interface UserInputQuestion {
  id: string;
  header: string;
  question: string;
  /** Absent for a free-text question. */
  options?: readonly UserInputOption[];
  /** Free text is admitted beside the listed options. */
  allowsOther: boolean;
  /** Render masked; the answer is never retained. */
  secret: boolean;
  kind: UserInputKind;
  /** Several options may be chosen. */
  multiSelect: boolean;
  /** May be left unanswered. */
  optional: boolean;
  /** Text the answer field starts with. */
  initialText?: string;
  /** Limits on the answer; absent when none apply. */
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

/** A link the harness asks the user to open, typically to sign in. */
export interface UserInputUrl {
  url: string;
  host: string;
}

/** Questions a harness asks; carried on a `permissionRequest` item. */
export interface UserInputRequest {
  questions: readonly UserInputQuestion[];
  /** The request resolves itself, unanswered, this long after it arrived. */
  autoResolutionMs?: number;
  /** Present for "open this link"; such a request has no questions. */
  url?: UserInputUrl;
}

export interface UserInputAnswer {
  questionId: string;
  selectedOption?: string;
  selectedOptions?: readonly string[];
  text?: string;
}

/** Cancel a question request with `resolvePermissionDecision` and `deny`. */
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
