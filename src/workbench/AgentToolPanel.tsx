import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { hideBrowser } from "../ports/nativeBrowserPort";
import { isTauriRuntime } from "../ports/nativeInvoke";
import { useStore } from "zustand";
import { acpProjectionStoreFor } from "../app/acpProjectionRegistry";
import { useAgentArtifactAdmissionStore } from "../app/agentArtifactAdmissionStore";
import { useAgentArtifactFollowStore } from "../app/agentArtifactFollowStore";
import {
  agentArtifactOwnerIsCurrent,
  openAgentArtifact,
} from "../app/openAgentArtifact";
import { useWorkbenchStore } from "../app/store";
import {
  ownerFromFocusedPage,
  validFocusedSubagentPage,
} from "../app/subagentPaneStore";
import { ModalDialog } from "../ui/ModalDialog";
import { resourceKey } from "../domain/resourceIdentity";
import type { AcpChatPage } from "../domain/acp";
import {
  sameAgentArtifactOwner,
  type AgentArtifactAdmission,
  type AgentArtifactOpenIntent,
  type AgentArtifactOwner,
} from "../domain/agentArtifact";
import type { ResourceRef, WorkbenchTab } from "../domain/workbench";
import { projectAgentEditSummary } from "../features/chat/acp/agentEditSummary";
import {
  pinAgentChatOwner,
  pinnedAgentChatOwner,
  usePinnedAgentChatOwner,
} from "../app/agentChatOwner";
import { useSideChatState } from "./sideChatState";
import type { AgentToolLaunch, AgentToolRequest } from "./AgentToolsProvider";
import "./agent-tool-panel.css";

// Lazy tool launch host: scoped Changes/Files picker, bounded resources and side chat.

export type AgentToolResource = {
  resource: ResourceRef;
  label: string;
  intent?: AgentArtifactOpenIntent;
  tabId?: string;
};
export function agentToolResources(
  tool: "changes" | "files",
  page: AcpChatPage | null,
  owner: AgentArtifactOwner | null,
  tabs: readonly WorkbenchTab[],
  admissions: readonly AgentArtifactAdmission[],
): AgentToolResource[] {
  if (
    !page ||
    !owner ||
    page.projectId !== owner.projectId ||
    page.chatId !== owner.chatId ||
    page.currentSessionId !== owner.sessionId
  )
    return [];
  const entries = new Map<string, AgentToolResource>();
  for (const turn of page.turns.slice(-32).reverse()) {
    if (turn.sessionId !== owner.sessionId) continue;
    for (const change of projectAgentEditSummary(page, turn).changes) {
      if (change.resource.kind !== "resource") continue;
      const resource = change.resource.resource;
      const key = resourceKey(resource);
      if (entries.has(key)) continue;
      const available =
        tool === "changes"
          ? Boolean(change.diffRef)
          : change.operation !== "deleted";
      entries.set(key, {
        resource,
        label: resource.relativePath,
        ...(available
          ? {
              intent: {
                owner,
                target: tool === "changes" ? "diff" : "file",
                resource,
                ...(tool === "changes" && change.diffRef
                  ? { diffRef: change.diffRef }
                  : {}),
              } as AgentArtifactOpenIntent,
            }
          : {}),
      });
      if (entries.size === 128) return [...entries.values()];
    }
  }
  for (const admission of admissions.slice(-32)) {
    if (!sameAgentArtifactOwner(admission.owner, owner)) continue;
    const tab = tabs.find(
      (candidate) =>
        candidate.id === admission.tabId &&
        candidate.kind === (tool === "changes" ? "diff" : "file"),
    );
    if (!tab?.resource || entries.has(resourceKey(tab.resource))) continue;
    entries.set(resourceKey(tab.resource), {
      resource: tab.resource,
      label: tab.resource.relativePath,
      tabId: tab.id,
    });
    if (entries.size === 128) break;
  }
  return [...entries.values()];
}

export async function launchSideChat(
  chatId: string,
  projectId: string,
  generation: number,
) {
  const state = useWorkbenchStore.getState();
  if (
    state.project?.id !== projectId ||
    state.projectGeneration !== generation ||
    state.projectTransitioning ||
    pinnedAgentChatOwner() !== chatId
  )
    return;
  const existing = useSideChatState
    .getState()
    .sides.find(
      (side) =>
        side.projectId === projectId &&
        (side.parentChatId === chatId || side.id === chatId),
    );
  if (existing && state.tabs.some((tab) => tab.id === existing.id)) {
    useSideChatState.setState({ pairedId: existing.id });
    await state.activateTab(existing.id);
    return;
  }
  const { readTranscriptQuote, openSideChat } =
    await import("../features/chat/TranscriptInteractions");
  const current = useWorkbenchStore.getState();
  if (
    current.project?.id !== projectId ||
    current.projectGeneration !== generation ||
    current.projectTransitioning ||
    pinnedAgentChatOwner() !== chatId
  )
    return;
  const content = Array.from(
    document.querySelectorAll<HTMLElement>(".agent-chat-pane[data-chat-id]"),
  ).find((pane) => pane.dataset.chatId === chatId);
  const selected =
    content && readTranscriptQuote(window.getSelection(), content);
  if (!selected)
    throw new Error("Select text in an answer before starting a side chat.");
  await openSideChat(chatId, selected.quote);
}

function launchRequest(launch: AgentToolLaunch): AgentToolRequest | null {
  const state = useWorkbenchStore.getState();
  return launch.tool === "sideChat" || !state.project
    ? null
    : {
        tool: launch.tool,
        chatId: launch.chatId,
        projectId: state.project.id,
        generation: state.projectGeneration,
      };
}

/** Activate the launch's Chat before its picker; retain side-chat start errors until dismissed. */
export function AgentToolHost({ launch }: { launch: AgentToolLaunch }) {
  const projectId = useWorkbenchStore((state) => state.project?.id);
  const generation = useWorkbenchStore((state) => state.projectGeneration);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const chatId = usePinnedAgentChatOwner();
  const [request, setRequest] = useState(() =>
    launch.chatId === pinnedAgentChatOwner() ? launchRequest(launch) : null,
  );
  const [error, setError] = useState<{
    projectId: string;
    generation: number;
    message: string;
  } | null>(null);
  useEffect(() => {
    const owner = launch.chatId;
    const state = useWorkbenchStore.getState();
    if (!state.project) return;
    const project = state.project.id;
    const epoch = state.projectGeneration;
    if (launch.tool === "sideChat") {
      launchSideChat(owner, project, epoch).catch((cause: unknown) => {
        const current = useWorkbenchStore.getState();
        if (
          current.project?.id === project &&
          current.projectGeneration === epoch
        )
          setError({
            projectId: project,
            generation: epoch,
            message: String(cause).slice(0, 512),
          });
      });
      return;
    }
    if (owner === pinnedAgentChatOwner()) return;
    const next = launchRequest(launch);
    void state
      .activateTab(owner)
      .then(() => {
        const current = useWorkbenchStore.getState();
        if (
          !next ||
          current.project?.id !== next.projectId ||
          current.projectGeneration !== next.generation ||
          current.activeTabId !== owner ||
          current.projectTransitioning
        )
          return;
        pinAgentChatOwner(owner);
        setRequest(next);
      })
      .catch(() => undefined);
  }, [launch]);
  const currentRequest =
    request &&
    !transitioning &&
    request.projectId === projectId &&
    request.generation === generation &&
    request.chatId === chatId
      ? request
      : null;
  if (request && !currentRequest) setRequest(null);
  // Restore launcher focus only after unmount closes the modal dialog's inertness.
  const restoreFocus = useRef(false);
  useEffect(() => {
    if (currentRequest || !restoreFocus.current) return;
    restoreFocus.current = false;
    launch.focus?.focus();
  }, [currentRequest, launch]);
  return (
    <>
      {currentRequest ? (
        <AgentToolPanel
          request={currentRequest}
          onDismiss={() => {
            restoreFocus.current = true;
            setRequest(null);
          }}
        />
      ) : null}
      {error &&
      error.projectId === projectId &&
      error.generation === generation ? (
        <div role="alert">
          {error.message}
          <button type="button" onClick={() => setError(null)}>
            Dismiss
          </button>
        </div>
      ) : null}
    </>
  );
}

export function AgentToolPanel({
  request,
  onDismiss,
}: {
  request: AgentToolRequest;
  onDismiss: () => void;
}) {
  const titleId = useId();
  const input = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [error, setError] = useState("");
  // Select tab/project/transition state only; ignore unrelated store writes.
  const tabs = useWorkbenchStore((value) => value.tabs);
  const activeTabId = useWorkbenchStore((value) => value.activeTabId);
  const project = useWorkbenchStore((value) => value.project);
  const projectTransitioning = useWorkbenchStore(
    (value) => value.projectTransitioning,
  );
  const activateTab = useWorkbenchStore((value) => value.activateTab);
  const browserId = tabs.find(
    (tab) => tab.id === activeTabId && tab.kind === "browser",
  )?.id;
  useEffect(() => {
    if (!browserId || !isTauriRuntime()) return;
    const hidden = hideBrowser(browserId);
    void hidden.catch(() =>
      setError("The browser could not be hidden for this dialog."),
    );
    return () => {
      void hidden
        .finally(() => window.dispatchEvent(new Event("resize")))
        .catch(() => undefined);
    };
  }, [browserId]);
  const page = useStore(
    acpProjectionStoreFor(request.chatId),
    (value) => value.page,
  );
  const scope = useStore(
    acpProjectionStoreFor(request.chatId),
    (value) => value.scope,
  );
  const admissions = useAgentArtifactAdmissionStore(
    (value) => value.admissions,
  );
  const valid = validFocusedSubagentPage(
    page,
    scope,
    request.projectId,
    request.chatId,
    request.generation,
    projectTransitioning,
  );
  const owner = ownerFromFocusedPage(page, valid);
  const resources = agentToolResources(
    request.tool,
    page,
    owner,
    tabs,
    admissions,
  );
  const rows = resources.filter((entry) =>
    entry.label.toLocaleLowerCase().includes(query.toLocaleLowerCase()),
  );
  useLayoutEffect(() => {
    input.current?.focus();
  }, []);
  async function select(entry: AgentToolResource) {
    if (!owner || !agentArtifactOwnerIsCurrent(owner, request.generation))
      return;
    try {
      if (entry.intent) {
        if (!(await openAgentArtifact(entry.intent))) {
          setError("This resource is no longer available for this Chat.");
          return;
        }
      } else if (entry.tabId) {
        useAgentArtifactFollowStore
          .getState()
          .pin(owner, { kind: "tab", tabId: entry.tabId });
        await activateTab(entry.tabId);
      }
      onDismiss();
    } catch (cause) {
      setError(String(cause).slice(0, 512));
    }
  }
  return (
    <ModalDialog
      labelledBy={titleId}
      className="agent-tool-panel"
      onDismiss={onDismiss}
    >
      <h2 id={titleId}>{request.tool === "changes" ? "Changes" : "Files"}</h2>
      <p>Only this Chat's resources · up to 128 recent files</p>
      <input
        ref={input}
        aria-label="Filter chat files"
        placeholder="Search chat files"
        maxLength={256}
        value={query}
        onChange={(event) => setQuery(event.currentTarget.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            const first = rows.find((entry) => entry.intent || entry.tabId);
            if (first) void select(first);
          }
        }}
      />
      <div
        className="agent-tool-panel__list"
        role="group"
        aria-label="Chat files"
      >
        {rows.map((entry) => (
          <button
            type="button"
            key={resourceKey(entry.resource)}
            disabled={!entry.intent && !entry.tabId}
            title={
              !entry.intent && !entry.tabId
                ? "No saved resource is available"
                : entry.label
            }
            onClick={() => void select(entry)}
          >
            {(project?.roots.length ?? 0) > 1
              ? `${project?.roots.find((root) => root.id === entry.resource.rootId)?.name ?? entry.resource.rootId} / `
              : ""}
            {entry.label}
          </button>
        ))}
        {rows.length === 0 ? (
          <p>
            {resources.length === 0
              ? "No resources available for this Chat yet."
              : "No matching chat files."}
          </p>
        ) : null}
      </div>
      {error ? <p role="alert">{error}</p> : null}
      <button type="button" onClick={onDismiss}>
        Close
      </button>
    </ModalDialog>
  );
}
