import { chatDisplayTitle } from "../domain/chatTitle";
import type { WorkbenchTab } from "../domain/workbench";
import { renderRegisteredCanvas } from "./surfaceRegistry";

export function AgentChatPane({
  tab,
  focused,
  onFocus,
}: {
  tab: WorkbenchTab;
  focused: boolean;
  onFocus: () => void;
}) {
  const title = chatDisplayTitle(tab.title);

  // Under Agent chrome the pane has no header of its own: the task header
  // names the focused Chat, and a side chat's tab sits in the center
  // header row (Codex s2/f0244).
  return (
    <section
      className={`agent-chat-pane${focused ? " agent-chat-pane--focused" : ""}`}
      aria-label={`Chat pane ${title}`}
      data-chat-id={tab.id}
      onFocus={onFocus}
    >
      {renderRegisteredCanvas("chat", tab)}
    </section>
  );
}
