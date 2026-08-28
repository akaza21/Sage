import { create } from "zustand";

import type {
  AcpChatPage,
  AcpPageCursor,
  AcpProjectionChangedEvent,
  ProjectGeneration,
  ProjectionRevision,
  SageChatId,
  SageProjectId,
  TurnProjection,
} from "../domain/acp";
import {
  compareDecimalRevision,
  mergeProjectionPage,
  normalizeProjectionPage,
} from "./acpProjectionMerge";

export const MAX_RETAINED_TURNS = 100;
export const MAX_RETAINED_ITEMS = 1_000;
export const MAX_RETAINED_BYTES = 8 * 1024 * 1024;

const MAX_REQUEST_SEQUENCE = Number.MAX_SAFE_INTEGER;

export interface AcpProjectionScope {
  projectId: SageProjectId;
  projectGeneration: ProjectGeneration;
  chatId: SageChatId;
}

export interface AcpPageLoadIntent extends AcpProjectionScope {
  requestSequence: number;
  cursor: AcpPageCursor | null;
  expectedProjectionRevision: ProjectionRevision | null;
}

export type ProjectionCommitDisposition =
  | "accepted"
  | "ignoredStaleIntent"
  | "ignoredStaleRevision"
  | "ignoredInvalidRevision";

export type ProjectionNoticeDisposition =
  "queued" | "coalesced" | "ignoredStale" | "requiresBootstrap";

interface AcpProjectionState {
  scope: AcpProjectionScope | null;
  page: AcpChatPage | null;
  retainedBytes: number;
  retentionIncomplete: boolean;
  activeLoad: AcpPageLoadIntent | null;
  requestSequence: number;
  pendingChange: AcpProjectionChangedEvent | null;
  error: string | null;
  activateScope: (scope: AcpProjectionScope) => void;
  beginPageLoad: (cursor: AcpPageCursor | null) => AcpPageLoadIntent;
  completePageLoad: (
    intent: AcpPageLoadIntent,
    page: AcpChatPage,
  ) => ProjectionCommitDisposition;
  failPageLoad: (intent: AcpPageLoadIntent, message: string) => boolean;
  applyAuthoritativePage: (
    scope: AcpProjectionScope,
    page: AcpChatPage,
  ) => ProjectionCommitDisposition;
  enqueueProjectionChange: (
    event: AcpProjectionChangedEvent,
  ) => ProjectionNoticeDisposition;
  takeProjectionChange: () => AcpProjectionChangedEvent | null;
  clear: () => void;
}

const encoder = new TextEncoder();

function byteLength(value: unknown): number {
  return encoder.encode(JSON.stringify(value)).byteLength;
}

function sameScope(
  left: AcpProjectionScope | null,
  right: AcpProjectionScope,
): boolean {
  return (
    left?.projectId === right.projectId &&
    left.projectGeneration === right.projectGeneration &&
    left.chatId === right.chatId
  );
}

function sameNoticeTuple(
  left: AcpProjectionChangedEvent,
  right: AcpProjectionChangedEvent,
): boolean {
  return (
    left.projectId === right.projectId &&
    left.chatId === right.chatId &&
    left.sessionId === right.sessionId
  );
}

function countItems(turns: readonly TurnProjection[]): number {
  return turns.reduce((total, turn) => total + turn.items.length, 0);
}

function countActivities(turns: readonly TurnProjection[]): number {
  return turns.reduce((total, turn) => total + turn.activities.length, 0);
}

function capTurnAndItemCounts(
  turns: readonly TurnProjection[],
): TurnProjection[] {
  const retained = turns
    .slice()
    .slice(-MAX_RETAINED_TURNS)
    .map((turn) => ({
      ...turn,
      items: turn.items
        .slice()
        .sort((left, right) => left.ordinal - right.ordinal),
      activities: turn.activities.slice(),
    }));

  let overflow = countItems(retained) - MAX_RETAINED_ITEMS;
  for (let index = 0; overflow > 0 && index < retained.length; index += 1) {
    const turn = retained[index];
    const remove = Math.min(overflow, turn.items.length);
    retained[index] = { ...turn, items: turn.items.slice(remove) };
    overflow -= remove;
  }
  return retained;
}

function trimToByteLimit(page: AcpChatPage): {
  page: AcpChatPage;
  retainedBytes: number;
  retentionIncomplete: boolean;
} {
  const sourceTurnCount = page.turns.length;
  const sourceItemCount = countItems(page.turns);
  const sourceActivityCount = countActivities(page.turns);
  let turns = capTurnAndItemCounts(page.turns);
  let candidate = { ...page, turns };
  let retainedBytes = byteLength(candidate);

  while (retainedBytes > MAX_RETAINED_BYTES && turns.length > 1) {
    const excessRatio = (retainedBytes - MAX_RETAINED_BYTES) / retainedBytes;
    const remove = Math.max(1, Math.floor(turns.length * excessRatio));
    turns = turns.slice(remove);
    candidate = { ...candidate, turns };
    retainedBytes = byteLength(candidate);
  }

  while (
    retainedBytes > MAX_RETAINED_BYTES &&
    turns.length === 1 &&
    turns[0].items.length > 0
  ) {
    const turn = turns[0];
    const excessRatio = (retainedBytes - MAX_RETAINED_BYTES) / retainedBytes;
    const remove = Math.max(1, Math.floor(turn.items.length * excessRatio));
    turns = [{ ...turn, items: turn.items.slice(remove) }];
    candidate = { ...candidate, turns };
    retainedBytes = byteLength(candidate);
  }

  while (
    retainedBytes > MAX_RETAINED_BYTES &&
    turns.length === 1 &&
    turns[0].activities.length > 0
  ) {
    const turn = turns[0];
    turns = [{ ...turn, activities: turn.activities.slice(1) }];
    candidate = { ...candidate, turns };
    retainedBytes = byteLength(candidate);
  }

  return {
    page: candidate,
    retainedBytes,
    retentionIncomplete:
      turns.length < sourceTurnCount ||
      countItems(turns) < sourceItemCount ||
      countActivities(turns) < sourceActivityCount,
  };
}

const initialState = {
  scope: null,
  page: null,
  retainedBytes: 0,
  retentionIncomplete: false,
  activeLoad: null,
  requestSequence: 0,
  pendingChange: null,
  error: null,
} satisfies Pick<
  AcpProjectionState,
  | "scope"
  | "page"
  | "retainedBytes"
  | "retentionIncomplete"
  | "activeLoad"
  | "requestSequence"
  | "pendingChange"
  | "error"
>;

export function createAcpProjectionStore() {
  return create<AcpProjectionState>((set, get) => ({
    ...initialState,

    activateScope: (scope) => {
      if (sameScope(get().scope, scope)) return;
      set({ ...initialState, scope, requestSequence: get().requestSequence });
    },

    beginPageLoad: (cursor) => {
      const scope = get().scope;
      if (!scope) throw new Error("ACP projection scope is not active");
      const sequence = get().requestSequence;
      if (!Number.isSafeInteger(sequence) || sequence >= MAX_REQUEST_SEQUENCE) {
        throw new Error("ACP projection request limit reached; restart Sage");
      }
      const intent: AcpPageLoadIntent = {
        ...scope,
        requestSequence: sequence + 1,
        cursor,
        expectedProjectionRevision: get().page?.projectionRevision ?? null,
      };
      set({
        activeLoad: intent,
        requestSequence: intent.requestSequence,
        error: null,
      });
      return intent;
    },

    completePageLoad: (intent, page) => {
      const state = get();
      if (
        state.activeLoad?.requestSequence !== intent.requestSequence ||
        !sameScope(state.scope, intent) ||
        page.projectId !== intent.projectId ||
        page.chatId !== intent.chatId
      ) {
        return "ignoredStaleIntent";
      }

      const currentRevision = state.page?.projectionRevision ?? null;
      if (currentRevision) {
        const comparison = compareDecimalRevision(
          page.projectionRevision,
          currentRevision,
        );
        if (comparison === null) {
          set({ activeLoad: null, error: null });
          return "ignoredInvalidRevision";
        }
        if (comparison < 0) {
          set({ activeLoad: null, error: null });
          return "ignoredStaleRevision";
        }
      }

      if (intent.cursor && intent.expectedProjectionRevision) {
        const comparison = compareDecimalRevision(
          page.projectionRevision,
          intent.expectedProjectionRevision,
        );
        if (comparison === null) {
          set({ activeLoad: null, error: null });
          return "ignoredInvalidRevision";
        }
        if (comparison !== 0) {
          set({ activeLoad: null, error: null });
          return "ignoredStaleRevision";
        }
      }

      const source =
        intent.cursor && state.page
          ? mergeProjectionPage(state.page, page)
          : normalizeProjectionPage(page);
      const retained = trimToByteLimit(source);
      set({
        page: retained.page,
        retainedBytes: retained.retainedBytes,
        retentionIncomplete:
          state.retentionIncomplete || retained.retentionIncomplete,
        activeLoad: null,
        error: null,
      });
      return "accepted";
    },

    failPageLoad: (intent, message) => {
      const state = get();
      if (
        state.activeLoad?.requestSequence !== intent.requestSequence ||
        !sameScope(state.scope, intent)
      ) {
        return false;
      }
      set({ activeLoad: null, error: message });
      return true;
    },

    applyAuthoritativePage: (scope, page) => {
      const state = get();
      if (
        !sameScope(state.scope, scope) ||
        page.projectId !== scope.projectId ||
        page.chatId !== scope.chatId
      ) {
        return "ignoredStaleIntent";
      }
      if (state.page) {
        const comparison = compareDecimalRevision(
          page.projectionRevision,
          state.page.projectionRevision,
        );
        if (comparison === null) return "ignoredInvalidRevision";
        if (comparison < 0) return "ignoredStaleRevision";
      }
      const retained = trimToByteLimit(normalizeProjectionPage(page));
      set({
        page: retained.page,
        retainedBytes: retained.retainedBytes,
        retentionIncomplete:
          state.retentionIncomplete || retained.retentionIncomplete,
        activeLoad: null,
        error: null,
      });
      return "accepted";
    },

    enqueueProjectionChange: (event) => {
      const state = get();
      if (
        state.scope?.projectId !== event.projectId ||
        state.scope.chatId !== event.chatId
      ) {
        return "ignoredStale";
      }
      const pending = state.pendingChange;
      if (!pending) {
        set({ pendingChange: event });
        return "queued";
      }
      if (!sameNoticeTuple(pending, event)) return "requiresBootstrap";
      const comparison = compareDecimalRevision(
        event.projectionRevision,
        pending.projectionRevision,
      );
      if (comparison === null || comparison <= 0) return "ignoredStale";
      set({ pendingChange: event });
      return "coalesced";
    },

    takeProjectionChange: () => {
      const pending = get().pendingChange;
      if (pending) set({ pendingChange: null });
      return pending;
    },

    clear: () =>
      set({ ...initialState, requestSequence: get().requestSequence }),
  }));
}

export type AcpProjectionStore = ReturnType<typeof createAcpProjectionStore>;

export const useAcpProjectionStore = createAcpProjectionStore();
