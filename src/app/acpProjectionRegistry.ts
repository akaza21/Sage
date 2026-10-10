import {
  createAcpProjectionStore,
  useAcpProjectionStore,
  type AcpProjectionStore,
} from "./acpProjectionStore";

export const MAX_VISIBLE_ACP_PROJECTION_SLOTS = 2;

const slots = new Map<
  string,
  { store: AcpProjectionStore; unsubscribe: () => void }
>();
const pinnedIds = new Set<string>();
const activityListeners = new Set<() => void>();
let membershipNotificationPending = false;

function notifyTurnActivity(): void {
  for (const listener of activityListeners) listener();
}

function notifyMembershipChange(): void {
  if (membershipNotificationPending) return;
  membershipNotificationPending = true;
  queueMicrotask(() => {
    membershipNotificationPending = false;
    notifyTurnActivity();
  });
}

useAcpProjectionStore.subscribe(notifyTurnActivity);

export function subscribeAcpTurnActivity(listener: () => void): () => void {
  activityListeners.add(listener);
  return () => activityListeners.delete(listener);
}

export function pinAcpProjectionIds(ids: readonly string[]): void {
  const nextIds = new Set<string>();
  for (const id of ids) {
    if (nextIds.size >= MAX_VISIBLE_ACP_PROJECTION_SLOTS) break;
    nextIds.add(id);
  }
  if (
    nextIds.size === pinnedIds.size &&
    Array.from(nextIds).every((id) => pinnedIds.has(id))
  )
    return;
  pinnedIds.clear();
  for (const id of nextIds) pinnedIds.add(id);
  notifyMembershipChange();
}

function evictUnpinnedSlot(requestedId: string): boolean {
  for (const id of slots.keys()) {
    if (id !== requestedId && !pinnedIds.has(id)) {
      releaseAcpProjectionStore(id);
      return true;
    }
  }
  return false;
}

export function acpProjectionStoreFor(
  chatId?: string | null,
): AcpProjectionStore {
  if (!chatId) return useAcpProjectionStore;
  const existing = slots.get(chatId);
  if (existing) return existing.store;
  if (slots.size >= MAX_VISIBLE_ACP_PROJECTION_SLOTS) {
    if (!evictUnpinnedSlot(chatId)) {
      return useAcpProjectionStore;
    }
  }
  const store = createAcpProjectionStore();
  slots.set(chatId, {
    store,
    unsubscribe: store.subscribe(notifyTurnActivity),
  });
  notifyMembershipChange();
  return store;
}

export function occupiedAcpProjectionSlotCount(): number {
  return slots.size;
}

export function hasActiveAcpTurn(
  projectId: string | null,
  projectGeneration: number,
): boolean {
  return [
    useAcpProjectionStore,
    ...Array.from(pinnedIds, (id) => slots.get(id)?.store),
  ].some((store) => {
    if (!store) return false;
    const { page, scope } = store.getState();
    return (
      page?.projectId === projectId &&
      scope?.projectId === projectId &&
      scope.projectGeneration === String(projectGeneration) &&
      page.turns.some(
        (turn) =>
          turn.sessionId === page.currentSessionId &&
          ![
            "completed",
            "interrupted",
            "cancelled",
            "failed",
            "deliveryUnknown",
          ].includes(turn.state),
      )
    );
  });
}

export function releaseAcpProjectionStore(chatId: string): void {
  const slot = slots.get(chatId);
  if (!slot) return;
  slot.unsubscribe();
  slots.delete(chatId);
  notifyMembershipChange();
}

export function clearAcpProjectionRegistry(): void {
  for (const slot of slots.values()) slot.unsubscribe();
  slots.clear();
  pinnedIds.clear();
  notifyMembershipChange();
}
