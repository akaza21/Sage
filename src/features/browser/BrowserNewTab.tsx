import { useEffect, useId, useRef, useState } from "react";
import {
  CirclePlus,
  FileDiff,
  FileText,
  Folders,
  MoreHorizontal,
  TerminalSquare,
} from "../../ui/leanIcons";
import { formatKeymapChords, keymapBinding } from "../../domain/keymapRegistry";
import type { AgentTool } from "../../workbench/AgentToolsProvider";
import { MenuPopover } from "../../ui/MenuPopover";
import type { BrowserSuggestedSites } from "./browserSuggestions";
import { BrowserFavicon } from "./browserFavicon";
import "./BrowserNewTab.css";
import { useWorkbenchStore } from "../../app/store";
import { usePinnedAgentChatOwner } from "../../app/agentChatOwner";
import { useSideChatState } from "../../workbench/sideChatState";
export { AgentToolHost } from "../../workbench/AgentToolPanel";

type NewTabProps = {
  suggestions: BrowserSuggestedSites;
  onNavigate: (url: string) => void;
  onNewTab: () => void;
  onTool: (tool: AgentTool) => void;
  /** False outside the Agent body, where no Chat owns the launchers. */
  chatAvailable: boolean;
  /** Defaults to a selected answer or an open side chat of this Chat. */
  sideChatAvailable?: boolean;
  terminalAvailable: boolean;
};

export function BrowserNewTab({
  suggestions,
  onNavigate,
  onNewTab,
  onTool,
  chatAvailable,
  sideChatAvailable,
  terminalAvailable,
}: NewTabProps) {
  const toolsId = useId();
  const suggestedId = useId();
  const terminalMenuId = useId();
  const terminalTrigger = useRef<HTMLButtonElement>(null);
  const [terminalMenu, setTerminalMenu] = useState(false);
  const apple = /Mac|iPhone|iPad/.test(navigator.platform);
  const sideChat = useSideChatAvailable(chatAvailable);
  const tools = [
    ["changes", "Changes", FileDiff, chatAvailable],
    [
      "terminal",
      "Terminal",
      TerminalSquare,
      chatAvailable && terminalAvailable,
    ],
    ["files", "Files", Folders, chatAvailable],
    [
      "sideChat",
      "Side chat",
      CirclePlus,
      chatAvailable && (sideChatAvailable ?? sideChat),
    ],
  ] as const;
  return (
    <div className="browser-new-tab">
      <section aria-labelledby={toolsId}>
        <h2 id={toolsId}>Tools</h2>
        <div className="browser-new-tab__tools">
          {tools.map(([tool, label, Icon, available]) => {
            const binding = keymapBinding(`agent.${tool}`);
            const ariaShortcut = [false, true]
              .map((mac) =>
                formatKeymapChords(binding, mac)[0].replace(
                  mac ? "Cmd" : "Ctrl",
                  mac ? "Meta" : "Control",
                ),
              )
              .join(" ");
            return (
              <div className="browser-new-tab__tool" key={tool}>
                <button
                  type="button"
                  aria-label={label}
                  aria-keyshortcuts={ariaShortcut}
                  disabled={!available}
                  title={
                    available
                      ? undefined
                      : !chatAvailable
                        ? "Open a Chat in Agent mode first"
                        : tool === "sideChat"
                          ? "Select an answer or open a side chat first"
                          : "Terminal requires the desktop app"
                  }
                  onMouseDown={
                    tool === "sideChat"
                      ? (event) => event.preventDefault()
                      : undefined
                  }
                  onClick={() => onTool(tool)}
                >
                  <Icon size={14} aria-hidden="true" />
                  <span>{label}</span>
                  <kbd aria-hidden="true">
                    {formatKeymapChords(binding, apple)[0]}
                  </kbd>
                </button>
                {tool === "terminal" ? (
                  <button
                    ref={terminalTrigger}
                    className="browser-new-tab__terminal-more"
                    type="button"
                    aria-label="Terminal options"
                    aria-haspopup="menu"
                    aria-controls={terminalMenuId}
                    aria-expanded={terminalMenu}
                    disabled={!available}
                    onClick={() => setTerminalMenu((open) => !open)}
                  >
                    <MoreHorizontal size={14} aria-hidden="true" />
                  </button>
                ) : null}
              </div>
            );
          })}
          <button type="button" onClick={onNewTab}>
            <FileText size={14} aria-hidden="true" />
            New page
          </button>
        </div>
      </section>
      <MenuPopover
        id={terminalMenuId}
        open={terminalMenu}
        triggerRef={terminalTrigger}
        ariaLabel="Terminal options"
        onDismiss={() => setTerminalMenu(false)}
      >
        <button
          type="button"
          role="menuitem"
          className="menu-item"
          onClick={() => {
            setTerminalMenu(false);
            onTool("terminal");
          }}
        >
          Open terminal dock
        </button>
      </MenuPopover>
      {suggestions.length > 0 ? (
        <section aria-labelledby={suggestedId}>
          <h2 id={suggestedId}>Suggested</h2>
          <div className="browser-new-tab__suggestions">
            {suggestions.slice(0, 4).map(({ url, title, favicon }) => (
              <button
                key={url}
                type="button"
                title={title?.trim() || new URL(url).host}
                onClick={() => onNavigate(url)}
              >
                <span aria-hidden="true">
                  <BrowserFavicon
                    key={favicon ?? ""}
                    value={favicon}
                    fallback="◎"
                  />
                </span>
                {title?.trim() || new URL(url).host}
              </button>
            ))}
          </div>
        </section>
      ) : null}
    </div>
  );
}

/**
 * Whether Side chat can start for the pinned Chat: one of its answers holds
 * a selection, or it already has a side chat (or is one) in this project.
 */
export function useSideChatAvailable(chatAvailable: boolean) {
  const projectId = useWorkbenchStore((state) => state.project?.id);
  const chatId = usePinnedAgentChatOwner();
  const sides = useSideChatState((state) => state.sides);
  const [selectedChat, setSelectedChat] = useState(selectedAnswerChat);
  useEffect(() => {
    const selectionChanged = () => setSelectedChat(selectedAnswerChat());
    document.addEventListener("selectionchange", selectionChanged);
    return () =>
      document.removeEventListener("selectionchange", selectionChanged);
  }, []);
  return (
    chatAvailable &&
    (selectedChat === chatId ||
      sides.some(
        (side) =>
          side.projectId === projectId &&
          (side.parentChatId === chatId || side.id === chatId),
      ))
  );
}

function selectedAnswerChat(): string | null {
  const selection = window.getSelection();
  const pane =
    selection?.anchorNode?.parentElement?.closest<HTMLElement>(
      "[data-chat-id]",
    );
  const answer = selection?.anchorNode?.parentElement?.closest(
    "[data-selectable-answer]",
  );
  const endAnswer = selection?.focusNode?.parentElement?.closest(
    "[data-selectable-answer]",
  );
  return selection && !selection.isCollapsed && answer && answer === endAnswer
    ? (pane?.dataset.chatId ?? null)
    : null;
}
