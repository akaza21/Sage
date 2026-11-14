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
  undoTurn?: string;
  undoKeepsFiles?: boolean;
}
