import { chatDisplayTitle } from "../domain/chatTitle";
import { useEffect, useRef, useState } from "react";
import { useWorkbenchStore } from "../app/store";
import {
  useAgentSettingsState,
  type AgentSettingsPage,
} from "../features/settings/agentSettingsState";
import { useSideChatState } from "./sideChatState";
import { navigateAgentHome } from "./agentRailNavigation";

export const AGENT_HISTORY_LIMIT = 32;
interface AgentHistory {
  scope: string;
  entries: string[];
  index: number;
}
const emptyHistory: AgentHistory = { scope: "", entries: [], index: -1 };

/** Disposable history of destinations already admitted in the current project. */
export function useAgentNavigationHistory(enabled: boolean) {
  const [history, setHistory] = useState(emptyHistory);
  const [busy, setBusy] = useState(false);
  const navigating = useRef(false);
  useEffect(() => {
    if (!enabled) return;
    const record = () => {
      if (navigating.current) return;
      const state = useWorkbenchStore.getState();
      if (!state.project || state.projectTransitioning) {
        setHistory(emptyHistory);
        return;
      }
      const scope = `${state.project.id}:${state.projectGeneration}:${state.nativeTransitionEpoch}`;
      const chats = state.tabs.filter(
        (tab) => tab.kind === "chat" && !tab.id.startsWith("side-"),
      );
      const settings = useAgentSettingsState.getState().page;
      const chat = chats.find(
        (tab) =>
          tab.id ===
          titleBarFocusedChatId(
            chats.map((candidate) => candidate.id),
            state.activeTabId,
          ),
      );
      const destination = settings
        ? `settings:${settings}`
        : chat
          ? `chat:${chat.id}`
          : null;
      setHistory((previous) => {
        const current =
          previous.scope === scope ? previous : { ...emptyHistory, scope };
        const valid = (entry: string) =>
          entry.startsWith("settings:") ||
          chats.some((tab) => `chat:${tab.id}` === entry);
        const before = current.entries
          .slice(0, current.index + 1)
          .filter(valid);
        const after = current.entries.slice(current.index + 1).filter(valid);
        if (!destination || before.at(-1) === destination) {
          if (before.length + after.length === current.entries.length)
            return current;
          return {
            scope,
            entries: [...before, ...after],
            index: before.length - 1,
          };
        }
        const entries = [...before, destination].slice(-AGENT_HISTORY_LIMIT);
        return { scope, entries, index: entries.length - 1 };
      });
    };
    record();
    const unsubscribeWorkbench = useWorkbenchStore.subscribe(record);
    const unsubscribeSettings = useAgentSettingsState.subscribe(record);
    return () => {
      unsubscribeWorkbench();
      unsubscribeSettings();
    };
  }, [enabled]);
  const travel = async (delta: number) => {
    const index = history.index + delta;
    const entry = history.entries[index];
    const state = useWorkbenchStore.getState();
    if (
      !enabled ||
      navigating.current ||
      !entry ||
      state.projectTransitioning ||
      history.scope !==
        `${state.project?.id}:${state.projectGeneration}:${state.nativeTransitionEpoch}`
    )
      return;
    if (
      entry.startsWith("chat:") &&
      !state.tabs.some(
        (tab) => tab.kind === "chat" && tab.id === entry.slice(5),
      )
    )
      return;
    navigating.current = true;
    setBusy(true);
    try {
      if (entry.startsWith("settings:"))
        useAgentSettingsState
          .getState()
          .open(entry.slice(9) as AgentSettingsPage);
      else {
        useSideChatState.setState({ pairedId: null });
        useAgentSettingsState.getState().close();
        await state.activateTab(entry.slice(5));
      }
      const current = useWorkbenchStore.getState();
      if (
        current.projectGeneration !== state.projectGeneration ||
        current.projectTransitioning ||
        current.project?.id !== state.project?.id
      ) {
        setHistory(emptyHistory);
        return;
      }
      setHistory({ ...history, index });
      requestAnimationFrame(() => {
        if (entry.startsWith("settings:"))
          document.querySelector<HTMLElement>(".agent-settings input")?.focus();
        else navigateAgentHome();
      });
    } catch {
      /* Activation already reports its native error through the workbench. */
    } finally {
      navigating.current = false;
      setBusy(false);
    }
  };
  const label = (index: number) => {
    const entry = history.entries[index];
    if (!entry) return undefined;
    return entry.startsWith("settings:")
      ? `Settings / ${entry.slice(9)}`
      : (titleBarSessionTitle(
          useWorkbenchStore
            .getState()
            .tabs.find((tab) => tab.id === entry.slice(5))?.title,
        ) ?? undefined);
  };
  return {
    canBack: enabled && !busy && history.index > 0,
    canForward: enabled && !busy && history.index + 1 < history.entries.length,
    backLabel: label(history.index - 1),
    forwardLabel: label(history.index + 1),
    travel,
  };
}

const TITLE_BAR_LABEL_CAP = 48;

function hasControlCharacter(value: string): boolean {
  return value.split("").some((character) => {
    const code = character.charCodeAt(0);
    return code < 32 || code === 127;
  });
}

export function titleBarSessionTitle(
  tabTitle: string | null | undefined,
): string | null {
  if (!tabTitle) return null;
  const name = chatDisplayTitle(tabTitle).trim();
  if (!name || hasControlCharacter(name)) return null;
  if (name.length <= TITLE_BAR_LABEL_CAP) return name;
  return `${name.slice(0, TITLE_BAR_LABEL_CAP - 1)}…`;
}

export function titleBarFocusedChatId(
  chatIds: readonly string[],
  activeTabId: string | null,
): string | null {
  if (activeTabId && chatIds.includes(activeTabId)) return activeTabId;
  return chatIds[0] ?? null;
}
