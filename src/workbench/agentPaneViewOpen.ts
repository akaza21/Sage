import { selectAgentPaneView, useAgentPaneViews, type AgentPaneView } from "./agentPaneViews";

export const MAX_PANE_VIEWS_PER_CHAT = 8;
const MAX_PANE_VIEWS = 32;

/** Opens (or reselects) a view; each Chat keeps its newest eight. */
export function openAgentPaneView(
  view: Omit<AgentPaneView, "id">,
  key: string,
) {
  const id = `${view.chatId}\u0000${view.kind}\u0000${key}`;
  useAgentPaneViews.setState((state) => {
    const kept = state.views.filter((entry) => entry.id !== id);
    const own = kept.filter((entry) => entry.chatId === view.chatId);
    const evicted =
      own.length >= MAX_PANE_VIEWS_PER_CHAT
        ? own[0].id
        : kept.length >= MAX_PANE_VIEWS
          ? kept[0].id
          : null;
    return {
      views: [...kept.filter((entry) => entry.id !== evicted), { ...view, id }],
      opened: state.opened + 1,
    };
  });
  selectAgentPaneView(id);
}
