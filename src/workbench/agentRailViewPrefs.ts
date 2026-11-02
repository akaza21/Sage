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

/** Load the stored layout once; a failed load keeps the defaults. */
export function restoreAgentRailViewPrefs(): Promise<void> {
  if (restored || !isTauriRuntime()) return Promise.resolve();
  restoring ??= invokeNative("agent_rail_view")
    .then((stored) => {
      restored = true;
      const next = admitPrefs(stored);
      writes.persisted(next);
      prefs = { ...prefs, ...next };
      emit();
    })
    .catch(() => undefined)
    .finally(() => {
      restoring = null;
    });
  return restoring;
}

function update(next: AgentRailViewPrefs) {
  prefs = next;
  emit();
  if (!isTauriRuntime()) return;
  const view = { organize: next.organize, ordering: next.ordering };
  writes.write(
    view,
    (value) => invokeNative("agent_rail_set_view", { view: value }),
    (persisted) => {
      prefs = { ...prefs, ...persisted };
      emit();
    },
  );
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
  writes.persisted(DEFAULT_PREFS);
  collapsed.clear();
  emit();
}

export function setAgentRailOrganize(organize: AgentRailOrganizeMode) {
  if (prefs.organize === organize) return;
  update({ ...prefs, organize });
}

export function setAgentRailOrdering(ordering: AgentRailOrdering) {
  if (prefs.ordering === ordering) return;
  update({ ...prefs, ordering });
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
