import type {
  EntityRevision,
  ProjectGeneration,
  RuntimeEpoch,
  SageAgentSessionId,
  SageChatId,
  SageProjectId,
  SageRootId,
} from "./acp";

export type PreviewPhase =
  | "idle"
  | "discovering"
  | "starting"
  | "waitingForOrigin"
  | "healthChecking"
  | "active"
  | "healthFailed"
  | "exited"
  | "stopping"
  | "stopped";
export type PreviewFailure =
  | "scriptMissing"
  | "originNotLoopback"
  | "startupTimeout"
  | "healthTimeout"
  | "exitedEarly"
  | "conflict"
  | "internal";
export interface PreviewProjectionOwner {
  projectId: SageProjectId;
  projectGeneration: ProjectGeneration;
  transitionEpoch: number;
  chatId: SageChatId;
  sessionId: SageAgentSessionId;
  runtimeEpoch: RuntimeEpoch;
}
export interface PreviewTargetRef extends Omit<
  PreviewProjectionOwner,
  "transitionEpoch"
> {
  expectedTransitionEpoch: number;
  rootId: SageRootId;
  scriptName: "dev" | "start";
}
export interface PreviewProjection extends PreviewProjectionOwner {
  previewRevision: EntityRevision;
  phase: PreviewPhase;
  failure: PreviewFailure | null;
  target: PreviewTargetRef | null;
  origin: string | null;
}
export interface PreviewProjectionChangedEvent {
  kind: "preview";
  preview: PreviewProjection;
}
