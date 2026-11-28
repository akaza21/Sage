import { useSyncExternalStore } from "react";
import { useWorkbenchStore } from "../app/store";
import type { HarnessDefinition } from "../domain/harness";
import { HarnessLoginAction } from "../features/harness/HarnessSignInAction";
import {
  harnessSignInRevision,
  harnessSignInStatus,
  subscribeHarnessSignIn,
  type HarnessSignInState,
} from "../features/harness/harnessSignIn";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import { Bot, Settings } from "../ui/leanIcons";
import ArchivedChatList from "./ArchivedChatList";

const SIGN_IN_LABELS: Partial<Record<HarnessSignInState, string>> = {
  signedIn: "Signed in",
  signedOut: "Needs sign-in",
  checking: "Checking sign-in",
  refused: "Needs sign-in",
};

/** The rail's Archived chats list: every project's archived chats, open. */
export function AgentArchivedPanel() {
  const projects = useWorkbenchStore((state) => state.projects);
  const unscoped = useWorkbenchStore((state) => state.unscopedChats);
  const currentId = useWorkbenchStore((state) => state.project?.id);
  const current = useWorkbenchStore((state) => state.archivedChats);
  const groups = [...unscoped, ...projects]
    .map((project) => ({
      id: project.id,
      name: project.name,
      chats: project.id === currentId ? current : (project.railArchived ?? []),
    }))
    .filter((group) => group.chats.length > 0);
  return (
    <section className="agent-rail-section" aria-label="Archived chats">
      <h2 className="agent-rail-section__label">Archived chats</h2>
      {groups.length === 0 ? (
        <p className="project-empty-copy">No archived chats.</p>
      ) : (
        groups.map((group) => (
          <div key={group.id} className="agent-rail-panel__group">
            <h3 className="agent-rail-panel__group-name">{group.name}</h3>
            <ArchivedChatList projectId={group.id} chats={group.chats} open />
          </div>
        ))
      )}
    </section>
  );
}

function harnessStatusLabel(harness: HarnessDefinition) {
  if (harness.installState !== "available") return "Not installed";
  return SIGN_IN_LABELS[harnessSignInStatus(harness.id).state] ?? "Installed";
}

/** The rail's Harnesses list: each harness Sage knows with its sign-in state;
 *  details and installs stay in Settings. */
export function AgentHarnessPanel() {
  useSyncExternalStore(subscribeHarnessSignIn, harnessSignInRevision);
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const openSettings = () => useAgentSettingsState.getState().open("Harnesses");
  return (
    <section className="agent-rail-section" aria-label="Harnesses">
      <div className="agent-rail-section__header">
        <h2 className="agent-rail-section__label">Harnesses</h2>
        <div className="agent-rail-section__actions">
          <button
            type="button"
            aria-label="Harness settings"
            title="Harness settings"
            onClick={openSettings}
          >
            <Settings size={14} aria-hidden="true" />
          </button>
        </div>
      </div>
      <ul className="agent-rail-harnesses">
        {harnesses.map((harness) => {
          const status = harnessStatusLabel(harness);
          return (
            <li key={harness.id} className="agent-rail-harness">
              <button
                type="button"
                className="agent-rail-harness__open"
                aria-label={`${harness.name}: ${status}`}
                onClick={openSettings}
              >
                <Bot size={16} aria-hidden="true" />
                <span className="agent-rail-harness__copy">
                  <span>{harness.name}</span>
                  <span className="agent-rail-harness__status">{status}</span>
                </span>
              </button>
              {status === "Needs sign-in" ? (
                <HarnessLoginAction harness={harness} />
              ) : null}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
