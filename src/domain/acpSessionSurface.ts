/** Live activities of a chat's current session, and its Undo. */
export type SessionActivityState =
  "running" | "completed" | "failed" | "cancelled";

export interface SessionActivity {
  /** Opaque key; the harness's own id stays in native code. */
  key: string;
  kind: string;
  label: string;
  state: SessionActivityState;
  progress: string | null;
  stoppable: boolean;
}

export interface AcpSessionSurface {
  activities: readonly SessionActivity[];
  /** The latest turn, when the running session can undo its file changes. */
  undoTurn?: string;
  /** Undo removes the turn from the conversation only; files stay. */
  undoKeepsFiles?: boolean;
}
