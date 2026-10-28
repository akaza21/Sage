import {
  createContext,
  useCallback,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { useWorkbenchStore } from "../app/store";
import { lazyParts } from "../features/chat/lazyParts";
import type * as NewTabChunk from "../features/browser/BrowserNewTab";
import {
  pinnedAgentChatOwner,
  usePinnedAgentChatOwner,
} from "../app/agentChatOwner";
import { keymapBinding, matchesKeymapChord } from "../domain/keymapRegistry";
import { isTauriRuntime } from "../ports/nativeInvoke";

export type AgentTool = "changes" | "terminal" | "files" | "sideChat";

// Outside Agent, no Chat owns New-tab launchers: disable all.
export const AgentToolsContext = createContext<{
  launch: (tool: AgentTool, chatId?: string) => void;
  chatAvailable: boolean;
  terminalAvailable: boolean;
}>({
  launch: () => undefined,
  chatAvailable: false,
  terminalAvailable: false,
});

// Nested project actions close the header menu and restore trigger focus.
export const AgentHeaderMenuContext = createContext<{
  close: () => void;
  returnFocus: () => void;
} | null>(null);

/** A single launch, run by the lazy New-tab host, including Chat activation/picker or side chat. */
export type AgentToolLaunch = {
  tool: Exclude<AgentTool, "terminal">;
  chatId: string;
  focus: HTMLElement | null;
  seq: number;
};
export type AgentToolRequest = {
  tool: "changes" | "files";
  projectId: string;
  chatId: string;
  generation: number;
};

export const AgentToolsPart = lazyParts<typeof NewTabChunk>(
  () => import("../features/browser/BrowserNewTab"),
);
const TOOLS: readonly AgentTool[] = [
  "changes",
  "terminal",
  "files",
  "sideChat",
];
let launchSeq = 0;

/** Launch owner; null without an open owning Chat. */
function agentToolOwner(explicitId?: string): string | null {
  const state = useWorkbenchStore.getState();
  const owner = explicitId ?? pinnedAgentChatOwner();
  return state.project &&
    !state.projectTransitioning &&
    state.tabs.some((tab) => tab.id === owner && tab.kind === "chat")
    ? owner
    : null;
}

export function AgentToolsProvider({
  children,
  onOpenTerminal,
}: {
  children: ReactNode;
  onOpenTerminal?: () => void;
}) {
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const chatId = usePinnedAgentChatOwner();
  const [launched, setLaunched] = useState<AgentToolLaunch | null>(null);
  const terminalAvailable = isTauriRuntime() && Boolean(onOpenTerminal);
  const launch = useCallback(
    (tool: AgentTool, explicitId?: string) => {
      const owner = agentToolOwner(explicitId);
      if (!owner) return;
      if (tool === "terminal") {
        if (terminalAvailable) onOpenTerminal?.();
        return;
      }
      setLaunched({
        tool,
        chatId: owner,
        focus:
          document.activeElement instanceof HTMLElement
            ? document.activeElement
            : null,
        seq: ++launchSeq,
      });
    },
    [onOpenTerminal, terminalAvailable],
  );
  useEffect(() => {
    const keyDown = (event: KeyboardEvent) => {
      if (
        event.defaultPrevented ||
        event.isComposing ||
        document.querySelector("dialog[open]")
      )
        return;
      const tool = TOOLS.find((candidate) =>
        matchesKeymapChord(keymapBinding(`agent.${candidate}`), event),
      );
      if (!tool) return;
      event.preventDefault();
      launch(tool);
    };
    window.addEventListener("keydown", keyDown);
    return () => window.removeEventListener("keydown", keyDown);
  }, [launch]);
  const chatAvailable = Boolean(chatId) && !transitioning;
  // Stable context avoids unrelated header/New-tab renders.
  const value = useMemo(
    () => ({ launch, chatAvailable, terminalAvailable }),
    [launch, chatAvailable, terminalAvailable],
  );
  return (
    <AgentToolsContext value={value}>
      {children}
      {launched ? (
        <AgentToolsPart
          part="AgentToolHost"
          key={launched.seq}
          launch={launched}
          fallback={<p role="status">Loading chat resources…</p>}
        />
      ) : null}
    </AgentToolsContext>
  );
}
