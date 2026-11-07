import type { EntityRevision, SageAgentSessionId } from "./acp";

/** Caps mirrored from the native session-scoped event contract. */
export const ACP_SESSION_SURFACE_LIMITS = {
  notices: 16,
  activities: 16,
  titleBytes: 256,
  noticeTextBytes: 1024,
  noticeSourceBytes: 128,
  activityKindBytes: 64,
  activityLabelBytes: 256,
  activityProgressBytes: 512,
  modelIdBytes: 512,
} as const;

export type AcpSessionNoticeLevel = "info" | "warning" | "error";

export type AcpSessionActivityState =
  "running" | "completed" | "failed" | "cancelled";

export interface AcpSessionNotice {
  level: AcpSessionNoticeLevel;
  text: string;
  source?: string;
}

export interface AcpSessionActivity {
  /** Opaque, session-scoped key; never the harness identity. */
  key: string;
  kind: string;
  label: string;
  state: AcpSessionActivityState;
  progress?: string;
}

/**
 * Session-scoped state of the chat's current session: the harness title and
 * newest notices, plus the live model and activity rows of the running epoch.
 */
export interface AcpSessionSurface {
  sessionId: SageAgentSessionId;
  title?: string;
  notices: readonly AcpSessionNotice[];
  modelId?: string;
  activities: readonly AcpSessionActivity[];
  revision: EntityRevision;
}
