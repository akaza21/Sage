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
export const USER_INPUT_MAX_QUESTIONS = 4;
export const USER_INPUT_MAX_OPTIONS = 8;

export interface UserInputOption {
  label: string;
  description: string;
}

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
}

/** Questions a harness asks; carried on a `permissionRequest` item. */
export interface UserInputRequest {
  questions: readonly UserInputQuestion[];
  /** The request resolves itself, unanswered, this long after it arrived. */
  autoResolutionMs?: number;
}

export interface UserInputAnswer {
  questionId: string;
  selectedOption?: string;
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
