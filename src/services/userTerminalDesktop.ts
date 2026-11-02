import { invokeNative } from "../ports/nativeInvoke";
import { utf8ByteLength } from "../domain/text";

/**
 * Native limits, mirrored from `src-tauri/src/user_terminal.rs`. Both sides
 * test the same literal bounds, so a change to one side fails its tests.
 */
export const TERMINAL_MAX_COLUMNS = 500;
export const TERMINAL_MAX_ROWS = 300;
export const TERMINAL_INPUT_BYTES = 4_096;

function validateDimensions(columns: number, rows: number): void {
  if (
    !Number.isInteger(columns) ||
    columns < 1 ||
    columns > TERMINAL_MAX_COLUMNS ||
    !Number.isInteger(rows) ||
    rows < 1 ||
    rows > TERMINAL_MAX_ROWS
  ) {
    throw new Error("Invalid terminal dimensions");
  }
}

export interface UserTerminalScope {
  projectId: string;
  projectGeneration: string;
  chatId: string;
}

export interface UserTerminalHandle {
  sessionId: string;
  revision: string;
}

export interface UserTerminalSession extends UserTerminalHandle {
  shellPath: string;
}

export interface UserTerminalPage {
  bytes: Uint8Array;
  nextCursor: string;
  truncated: boolean;
}

/** The native error's `code`, when it carries one. */
export function terminalErrorCode(error: unknown): unknown {
  return error && typeof error === "object" && "code" in error
    ? (error as { code?: unknown }).code
    : undefined;
}

export function terminalErrorLabel(error: unknown): string {
  const code = terminalErrorCode(error);
  if (code === "unavailable")
    return "Terminal is unavailable on this platform.";
  if (code === "capacity")
    return "Terminal limit reached for this chat or project.";
  if (code === "invalid_scope" || code === "unknown_session")
    return "This terminal is no longer attached to the current chat.";
  if (code === "ended") return "Session ended";
  if (code === "busy")
    return "Input was not sent: the program in the terminal is not reading it.";
  return "Terminal operation failed. The native session may need cleanup.";
}

const handlePattern = /^[\da-f]{8}(?:-[\da-f]{4}){3}-[\da-f]{12}$/u;
const decimalPattern = /^(0|[1-9]\d{0,19})$/u;
const base64Pattern =
  /^(?:[A-Za-z\d+/]{4})*(?:[A-Za-z\d+/]{2}==|[A-Za-z\d+/]{3}=)?$/u;

function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Invalid terminal response");
  }
  return value as Record<string, unknown>;
}

function decimal(value: unknown): string {
  if (typeof value !== "string" || !decimalPattern.test(value)) {
    throw new Error("Invalid terminal cursor or revision");
  }
  return value;
}

function validHandle(handle: UserTerminalHandle): void {
  if (
    !handlePattern.test(handle.sessionId) ||
    decimal(handle.revision) === "0"
  ) {
    throw new Error("Invalid terminal handle");
  }
}

function wireHandle(handle: UserTerminalHandle): UserTerminalHandle {
  validHandle(handle);
  return { sessionId: handle.sessionId, revision: handle.revision };
}

export async function createUserTerminal(
  scope: UserTerminalScope,
  columns: number,
  rows: number,
): Promise<UserTerminalSession> {
  validateDimensions(columns, rows);
  const response = record(
    await invokeNative<unknown>("terminal_create", {
      request: { scope, columns, rows },
    }),
  );
  const session: UserTerminalSession = {
    sessionId: String(response.sessionId ?? ""),
    revision: decimal(response.revision),
    shellPath: String(response.shellPath ?? ""),
  };
  validHandle(session);
  if (session.shellPath.length === 0 || session.shellPath.length > 32_767) {
    throw new Error("Invalid terminal shell metadata");
  }
  return session;
}

export async function writeUserTerminal(
  scope: UserTerminalScope,
  handle: UserTerminalHandle,
  text: string,
): Promise<void> {
  const exactHandle = wireHandle(handle);
  if (!text || utf8ByteLength(text) > TERMINAL_INPUT_BYTES) {
    throw new Error("Terminal input must be 1–4096 UTF-8 bytes");
  }
  await invokeNative<void>("terminal_write", {
    request: { scope, handle: exactHandle, text },
  });
}

export async function resizeUserTerminal(
  scope: UserTerminalScope,
  handle: UserTerminalHandle,
  columns: number,
  rows: number,
): Promise<void> {
  const exactHandle = wireHandle(handle);
  validateDimensions(columns, rows);
  await invokeNative<void>("terminal_resize", {
    request: { scope, handle: exactHandle, columns, rows },
  });
}

export async function readUserTerminal(
  scope: UserTerminalScope,
  handle: UserTerminalHandle,
  cursor: string,
): Promise<UserTerminalPage> {
  const exactHandle = wireHandle(handle);
  decimal(cursor);
  const response = record(
    await invokeNative<unknown>("terminal_read", {
      request: { scope, handle: exactHandle, cursor },
    }),
  );
  const encoded = response.bytesBase64;
  if (
    typeof encoded !== "string" ||
    encoded.length > 64_000 ||
    !base64Pattern.test(encoded) ||
    typeof response.truncated !== "boolean"
  ) {
    throw new Error("Invalid terminal output page");
  }
  // The validated base64 is at most 64,000 characters: at most 48,000 bytes.
  return {
    bytes: Uint8Array.from(atob(encoded), (character) =>
      character.charCodeAt(0),
    ),
    nextCursor: decimal(response.nextCursor),
    truncated: response.truncated,
  };
}

export async function closeUserTerminal(
  scope: UserTerminalScope,
  handle: UserTerminalHandle,
): Promise<void> {
  await invokeNative<void>("terminal_close", {
    request: { scope, handle: wireHandle(handle) },
  });
}
