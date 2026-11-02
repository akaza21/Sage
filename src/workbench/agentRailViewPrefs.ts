import { useEffect, useSyncExternalStore } from "react";

import { createSettingWriter } from "../app/settingWriteQueue";
import { invokeNative, isTauriRuntime } from "../ports/nativeInvoke";
import {
  AGENT_RAIL_ORDERINGS,
  AGENT_RAIL_ORGANIZE_MODES,
  type AgentRailOrdering,
  type AgentRailOrganizeMode,
} from "./agentRailStatusView";

export interface AgentRailViewPrefs {
  organize: AgentRailOrganizeMode;
  ordering: AgentRailOrdering;
}

const DEFAULT_PREFS: AgentRailViewPrefs = {
  organize: "project",
  ordering: "recency",
};

const listeners = new Set<() => void>();
const collapsed = new Set<string>();

let prefs: AgentRailViewPrefs = DEFAULT_PREFS;
// Rust stores the layout choice (settings_store/agent_rail_view.rs); this
// module holds only its rendered projection.
const writes =
  createSettingWriter<Pick<AgentRailViewPrefs, "organize" | "ordering">>(
    DEFAULT_PREFS,
  );
let restoring: Promise<void> | null = null;
let restored = false;

function oneOf<T extends string>(
  values: ReadonlyArray<T>,
  value: unknown,
): value is T {
  return values.some((candidate) => candidate === value);
}

function admitPrefs(input: unknown): AgentRailViewPrefs {
  const { organize, ordering } =
    typeof input === "object" && input !== null
      ? (input as Record<string, unknown>)
      : {};
  return {
    organize: oneOf(AGENT_RAIL_ORGANIZE_MODES, organize)
      ? organize
      : DEFAULT_PREFS.organize,
    ordering: oneOf(AGENT_RAIL_ORDERINGS, ordering)
      ? ordering
      : DEFAULT_PREFS.ordering,
  };
}

type AgentRailViewField = "organize" | "ordering";
// Choices made while the stored layout is still loading; they win over it.
const editedWhileLoading = new Set<AgentRailViewField>();

/**
 * Load the stored layout once; a failed load keeps the defaults. A choice made
 * before the load returns is kept, and the merged layout is stored.
 */
export function restoreAgentRailViewPrefs(): Promise<void> {
  if (restored || !isTauriRuntime()) return Promise.resolve();
  restoring ??= invokeNative("agent_rail_view")
    .then((stored) => {
      restored = true;
      const next = admitPrefs(stored);
      writes.persisted(next);
      const merged = {
        organize: editedWhileLoading.has("organize")
          ? prefs.organize
          : next.organize,
        ordering: editedWhileLoading.has("ordering")
          ? prefs.ordering
          : next.ordering,
      };
      const reconcile = editedWhileLoading.size > 0;
      editedWhileLoading.clear();
      prefs = { ...prefs, ...merged };
      emit();
      if (reconcile) persist(merged);
    })
    .catch(() => {
      // The stored layout is unreadable. A choice already made must not be
      // lost on restart, so store it with the defaults for the rest; with no
      // pending choice, the next read tries again.
      if (editedWhileLoading.size === 0) return;
      restored = true;
      editedWhileLoading.clear();
      persist({ organize: prefs.organize, ordering: prefs.ordering });
    })
    .finally(() => {
      restoring = null;
    });
  return restoring;
}

function persist(view: Pick<AgentRailViewPrefs, AgentRailViewField>) {
  writes.write(
    view,
    (value) => invokeNative("agent_rail_set_view", { view: value }),
    (persisted) => {
      prefs = { ...prefs, ...persisted };
      emit();
    },
  );
}

function update(next: AgentRailViewPrefs, field: AgentRailViewField) {
  prefs = next;
  emit();
  if (!isTauriRuntime()) return;
  // Until the stored layout arrives, hold the write: the load merges this
  // choice and stores the result.
  if (!restored) {
    editedWhileLoading.add(field);
    void restoreAgentRailViewPrefs();
    return;
  }
  persist({ organize: next.organize, ordering: next.ordering });
}

function emit() {
  for (const listener of listeners) listener();
}

export function subscribeAgentRailViewPrefs(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function agentRailViewPrefs(): AgentRailViewPrefs {
  return prefs;
}

export function resetAgentRailViewPrefs() {
  prefs = DEFAULT_PREFS;
  restored = false;
  editedWhileLoading.clear();
  writes.persisted(DEFAULT_PREFS);
  collapsed.clear();
  emit();
}

export function setAgentRailOrganize(organize: AgentRailOrganizeMode) {
  if (prefs.organize === organize) return;
  update({ ...prefs, organize }, "organize");
}

export function setAgentRailOrdering(ordering: AgentRailOrdering) {
  if (prefs.ordering === ordering) return;
  update({ ...prefs, ordering }, "ordering");
}

export function agentRailSectionCollapsed(id: string): boolean {
  return collapsed.has(id);
}

export function toggleAgentRailSection(id: string) {
  if (collapsed.has(id)) collapsed.delete(id);
  else collapsed.add(id);
  prefs = { ...prefs };
  emit();
}

export function expandAgentRailSection(id: string) {
  if (!collapsed.delete(id)) return;
  prefs = { ...prefs };
  emit();
}

export function useAgentRailViewPrefs(): AgentRailViewPrefs {
  useEffect(() => void restoreAgentRailViewPrefs(), []);
  return useSyncExternalStore(
    subscribeAgentRailViewPrefs,
    agentRailViewPrefs,
    agentRailViewPrefs,
  );
}
