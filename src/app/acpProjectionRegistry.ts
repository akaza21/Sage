import {
  createAcpProjectionStore,
  useAcpProjectionStore,
  type AcpProjectionStore,
} from "./acpProjectionStore";

export const MAX_VISIBLE_ACP_PROJECTION_SLOTS = 2;

const slots = new Map<string, AcpProjectionStore>();
const pinnedIds = new Set<string>();

export function pinAcpProjectionIds(ids: readonly string[]): void {
  pinnedIds.clear();
  for (const id of ids) {
    if (pinnedIds.size >= MAX_VISIBLE_ACP_PROJECTION_SLOTS) break;
    pinnedIds.add(id);
  }
}

function evictUnpinnedSlot(requestedId: string): boolean {
  for (const id of slots.keys()) {
    if (id !== requestedId && !pinnedIds.has(id)) {
      slots.delete(id);
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
  if (existing) return existing;
  if (slots.size >= MAX_VISIBLE_ACP_PROJECTION_SLOTS) {
    if (!evictUnpinnedSlot(chatId)) {
      return useAcpProjectionStore;
    }
  }
  const store = createAcpProjectionStore();
  slots.set(chatId, store);
  return store;
}

export function occupiedAcpProjectionSlotCount(): number {
  return slots.size;
}

export function releaseAcpProjectionStore(chatId: string): void {
  slots.delete(chatId);
}

export function clearAcpProjectionRegistry(): void {
  slots.clear();
  pinnedIds.clear();
}
