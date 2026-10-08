import { invokeNative } from "../ports/nativeInvoke";
import { utf8ByteLength } from "../domain/text";

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
  truncations: string;
  truncated: boolean;
}

const handlePattern =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/u;
const decimalPattern = /^(0|[1-9][0-9]{0,19})$/u;
const base64Pattern =
  /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/u;

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
  if (
    !Number.isInteger(columns) ||
    columns < 1 ||
    columns > 500 ||
    !Number.isInteger(rows) ||
    rows < 1 ||
    rows > 300
  ) {
    throw new Error("Invalid terminal dimensions");
  }
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
  const byteLength = utf8ByteLength(text);
  if (byteLength < 1 || byteLength > 4_096) {
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
  if (
    !Number.isInteger(columns) ||
    columns < 1 ||
    columns > 500 ||
    !Number.isInteger(rows) ||
    rows < 1 ||
    rows > 300
  ) {
    throw new Error("Invalid terminal dimensions");
  }
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
  const raw = atob(encoded);
  if (raw.length > 48_000)
    throw new Error("Terminal output page exceeded limit");
  const bytes = Uint8Array.from(raw, (character) => character.charCodeAt(0));
  return {
    bytes,
    nextCursor: decimal(response.nextCursor),
    truncations: decimal(response.truncations),
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
