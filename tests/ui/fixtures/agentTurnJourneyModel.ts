import type {
  AcpCapabilitySet,
  AcpChatPage,
  AcpMutationResult,
  AcpProjectionChangedEvent,
  CancelAcpTurnRequest,
  CuratedBindingId,
  CuratedModelId,
  EntityRevision,
  ProjectionRevision,
  RuntimeEpoch,
  SageAgentSessionId,
  SageChatId,
  SageItemId,
  SageProjectId,
  SageTurnId,
  SendAcpTurnRequest,
  TurnProjection,
  TurnState,
} from "../../../src/domain/acp";

export {
  MAX_CAPTURED_REQUESTS,
  MAX_COMPOSER_HISTORY,
  MAX_DEFERRED_CALLS,
  MAX_PROJECTION_LISTENERS,
} from "./agentTurnJourneyLimits";

export const JOURNEY_IDS = {
  projectA: "journey-project-a" as SageProjectId,
  projectB: "journey-project-b" as SageProjectId,
  alpha: "journey-chat-alpha" as SageChatId,
  beta: "journey-chat-beta" as SageChatId,
  gamma: "journey-chat-gamma" as SageChatId,
  binding: "journey-binding" as CuratedBindingId,
  model: "journey-model" as CuratedModelId,
} as const;

export type JourneyChat = "alpha" | "beta" | "gamma";
export type JourneyEvent =
  | { type: "noop" }
  | {
      type: "resolveSend";
      chat: JourneyChat;
      state: "sending" | "deliveryUnknown";
    }
  | { type: "rejectSend"; chat: JourneyChat; certainty: "definite" | "unknown" }
  | {
      type: "publish";
      chat: JourneyChat;
      state: TurnState;
      assistantText?: string;
    }
  | { type: "resolveCancel"; chat: JourneyChat }
  | { type: "seedHistory"; chat: JourneyChat; turns: number }
  | { type: "stalePublication"; source: JourneyChat; target: JourneyChat };

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: unknown) => void;
}

export interface PendingSend extends Deferred<AcpMutationResult> {
  request: SendAcpTurnRequest;
}

export interface PendingCancel extends Deferred<AcpMutationResult> {
  request: CancelAcpTurnRequest;
}

export interface ProjectionAudit {
  projectId: string;
  projectGeneration: string | null;
  chatId: string;
  sessionId: string | null;
  sessionRevision: string | null;
  turnId: string | null;
  turnRevision: string | null;
  turnState: TurnState | null;
  runtimeEpoch: string | null;
  projectionRevision: string;
  targetProjectId: string | null;
  targetChatId: string | null;
  disposition: string;
}

export interface FixtureState {
  eventCount: number;
  eventHistory: string[];
  pages: Map<SageChatId, AcpChatPage>;
  sendRequests: SendAcpTurnRequest[];
  cancelRequests: CancelAcpTurnRequest[];
  pendingSends: Map<SageChatId, PendingSend>;
  pendingCancels: Map<SageChatId, PendingCancel>;
  listeners: Set<(event: AcpProjectionChangedEvent) => void>;
  publications: ProjectionAudit[];
  composerHistory: Array<{ chatId: string; text: string }>;
  activeProjectId: SageProjectId;
  activeChatByProject: Map<SageProjectId, SageChatId>;
  transitionEpoch: number;
  disposeStoreSubscription: () => void;
  restore: () => void;
}

const fixedNow = Date.UTC(2026, 8, 4, 12, 0, 0);
const capabilities: AcpCapabilitySet = {
  schemaVersion: 1,
  protocolVersion: "1.0",
  newSession: true,
  resumeSession: true,
  loadSession: true,
  closeSession: true,
  prompt: true,
  cancel: true,
  nativeSessionFork: false,
  activeTurnSteer: false,
  childConversationProjection: false,
  childTranscriptRead: false,
  collaborationEventProjection: false,
  multiRootExtensionSchemaId: null,
  modelCount: 1,
  configCount: 0,
};

export function chatId(chat: JourneyChat): SageChatId {
  return JOURNEY_IDS[chat];
}

export function projectIdFor(chat: JourneyChat): SageProjectId {
  return chat === "gamma" ? JOURNEY_IDS.projectB : JOURNEY_IDS.projectA;
}

export function sessionId(chat: JourneyChat): SageAgentSessionId {
  return `journey-session-${chat}` as SageAgentSessionId;
}

export function makeDeferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((accept, decline) => {
    resolve = accept;
    reject = decline;
  });
  return { promise, resolve, reject };
}

export function initialPage(chat: JourneyChat): AcpChatPage {
  const id = chatId(chat);
  const session = sessionId(chat);
  return {
    projectId: projectIdFor(chat),
    chatId: id,
    title: ({ alpha: "Alpha", beta: "Beta", gamma: "Gamma" } as const)[chat],
    projectionRevision: "1" as ProjectionRevision,
    currentSessionId: session,
    sessions: [
      {
        id: session,
        ordinal: 1,
        bindingId: JOURNEY_IDS.binding,
        modelId: JOURNEY_IDS.model,
        state: "active",
        residency: "hot",
        runtimeEpoch: ({ alpha: "11", beta: "22", gamma: "33" } as const)[
          chat
        ] as RuntimeEpoch,
        nextRuntimeEpoch: null,
        capabilities,
        promptContent: { schemaVersion: 1, kinds: {} },
        revision: "1" as EntityRevision,
        projectionRevision: "1" as ProjectionRevision,
        legacy: false,
      },
    ],
    turns: [],
    nextCursor: null,
  };
}

function nextRevision(page: AcpChatPage): ProjectionRevision {
  return String(Number(page.projectionRevision) + 1) as ProjectionRevision;
}

function item(
  page: AcpChatPage,
  turn: TurnProjection,
  ordinal: number,
  role: "user" | "assistant",
  text: string,
) {
  return {
    id: `${turn.id}-item-${ordinal}` as SageItemId,
    projectId: page.projectId,
    chatId: page.chatId,
    sessionId: turn.sessionId,
    turnId: turn.id,
    runtimeEpoch: turn.runtimeEpoch,
    ordinal,
    origin: "primary" as const,
    activityId: null,
    lifecycle: "completed" as const,
    revision: "1" as EntityRevision,
    createdAt: fixedNow + turn.ordinal * 1_000 + ordinal,
    updatedAt: fixedNow + turn.ordinal * 1_000 + ordinal,
    terminalAt: fixedNow + turn.ordinal * 1_000 + ordinal,
    kind: "text" as const,
    toolState: null,
    payload: { role, text },
  };
}

export function appendTurn(
  page: AcpChatPage,
  prompt: string,
  state: TurnState,
): AcpChatPage {
  const ordinal = page.turns.length + 1;
  const turn: TurnProjection = {
    id: `${page.chatId}-turn-${ordinal}` as SageTurnId,
    sessionId: page.currentSessionId!,
    runtimeEpoch: page.sessions[0].runtimeEpoch!,
    ordinal,
    state,
    revision: "1" as EntityRevision,
    createdAt: fixedNow + ordinal * 1_000,
    updatedAt: fixedNow + ordinal * 1_000,
    terminalAt: state === "deliveryUnknown" ? fixedNow + ordinal * 1_000 : null,
    items: [],
    activities: [],
  };
  const revision = nextRevision(page);
  return {
    ...page,
    projectionRevision: revision,
    sessions: page.sessions.map((session) => ({
      ...session,
      projectionRevision: revision,
    })),
    turns: [
      ...page.turns,
      { ...turn, items: [item(page, turn, 1, "user", prompt)] },
    ],
  };
}

export function updateLatestTurn(
  page: AcpChatPage,
  state: TurnState,
  assistantText?: string,
): AcpChatPage {
  const current = page.turns.at(-1);
  if (!current) throw new Error(`No Turn exists for ${page.chatId}`);
  const revision = nextRevision(page);
  const terminal = [
    "completed",
    "interrupted",
    "cancelled",
    "failed",
    "deliveryUnknown",
  ].includes(state);
  const updated: TurnProjection = {
    ...current,
    state,
    revision: String(Number(current.revision) + 1) as EntityRevision,
    updatedAt: fixedNow + Number(revision) * 1_000,
    terminalAt: terminal ? fixedNow + Number(revision) * 1_000 : null,
    items: assistantText
      ? [
          ...current.items,
          item(
            page,
            current,
            current.items.length + 1,
            "assistant",
            assistantText,
          ),
        ]
      : current.items,
  };
  return {
    ...page,
    projectionRevision: revision,
    sessions: page.sessions.map((session) => ({
      ...session,
      projectionRevision: revision,
    })),
    turns: [...page.turns.slice(0, -1), updated],
  };
}
