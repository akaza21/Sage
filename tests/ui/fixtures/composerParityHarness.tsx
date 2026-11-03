import { createRoot } from "react-dom/client";
import { flushSync } from "react-dom";
import { useRef, useState } from "react";
import { useWorkbenchStore } from "../../../src/app/store";
import { useSessionAccessModeStore } from "../../../src/app/sessionAccessModeStore";
import { snapshotPage } from "../../../src/app/agentDiff.fixture";
import type { AcpSessionConfig } from "../../../src/domain/acpSessionConfig";
import { ChatComposerRegion } from "../../../src/features/chat/ChatComposerRegion";
import { ChatTranscript } from "../../../src/features/chat/ChatTranscript";
import { textItem } from "../../../src/features/chat/acp/acpProjection.fixture";
import { agentWorkbenchLayout } from "../../../src/workbench/agentWorkbenchLayout";
// Same stylesheets, in the same order, as src/main.tsx so the fixture
// measures the composer with the cascade the app ships.
import "../../../src/styles/global.css";
import "../../../src/styles/shared-controls.css";
import "../../../src/styles/shared-dialogs.css";
import "../../../src/styles/searchable-picker-bounded-popover.css";
import "../../../src/styles/workbench-chrome.css";
import "../../../src/styles/status-surfaces.css";
import "../../../src/styles/chat-surface.css";
import "../../../src/styles/changed-files-summary.css";
import "../../../src/styles/agent-chat-panes.css";
import "../../../src/styles/composer-surface.css";
import "../../../src/styles/chat-container-controls.css";
import "../../../src/styles/composer-model-picker.css";
import "../../../src/styles/composer-image.css";
import "../../../src/features/chat/acp/acp-chat.css";
import "../../../src/workbench/workbench-window-mode.css";

export type ParityState =
  | "home"
  | "answer"
  | "files"
  | "running"
  | "stopped"
  | "unknown"
  | "no-effort"
  | "no-default"
  | "pre-turn"
  | "models"
  | "model-collisions"
  | "access";
const noop = async () => {};
const project = {
  id: "project-a",
  name: "Sage parity project with a long but authoritative workspace name",
  roots: [],
};
const harnessNames = ["Codex", "Claude Code", "Pi", "OpenCode", "Oh My Pi"];
const root = createRoot(document.getElementById("root")!);
let config: AcpSessionConfig;
let current: { name: string; state: ParityState };

export function Host({ state }: { state: ParityState }) {
  const layout = agentWorkbenchLayout({
    viewportWidth: window.innerWidth,
    artifactExpanded: false,
  });
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const [editSummaryContainer, setEditSummaryContainer] =
    useState<HTMLDivElement | null>(null);
  const transcriptRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const endRef = useRef<HTMLDivElement>(null);
  const projection = snapshotPage();
  const home = state === "home" || state === "pre-turn";
  if (home) projection.turns = [];
  else if (state !== "files") {
    const item = textItem();
    item.payload.text =
      "| Status | Evidence |\n| --- | --- |\n| Durable projection | Verified replay |\n| Long unbroken value | abcdefghijklmnopqrstuvwxyz0123456789 |";
    projection.turns[0].items = [item];
  }
  return (
    <div
      className="workbench--agent"
      style={{
        display: "grid",
        height: "100%",
        gridTemplateColumns: layout.gridTemplateColumns,
      }}
    >
      <div aria-hidden="true" />
      {layout.navigationResizable ? <div aria-hidden="true" /> : null}
      <div className="agent-chat-center">
        <div className="agent-chat-center__header" />
        <div className="agent-chat-panes">
          <div className="agent-chat-pane agent-chat-pane--focused">
            <section className="chat-surface chat-surface--agent">
              <ChatTranscript
                editSummaryContainer={editSummaryContainer}
                transcriptRef={transcriptRef}
                contentRef={contentRef}
                endRef={endRef}
                onScroll={() => {}}
                projectTransitioning={false}
                project={project}
                useNativeAcp
                page={projection}
                focused
                projectRoots={[]}
                retentionIncomplete={false}
                onOpenResource={() => {}}
                hasEarlier={false}
                loadingEarlier={false}
                historyError={null}
                onLoadEarlier={() => {}}
                messages={[]}
                developmentSimulator={false}
                showJumpToLatest={false}
                onJumpToLatest={() => {}}
              />
              <ChatComposerRegion
                onEditSummaryMount={setEditSummaryContainer}
                composerRef={composerRef}
                fieldId="message"
                dictationScope="chat-a"
                sessionId={
                  state === "unknown" || state === "pre-turn"
                    ? null
                    : "session-a"
                }
                descriptionId="message-description"
                disabledReasonId="message-disabled"
                text=""
                attachedContext={[]}
                enabled
                actionBusy={false}
                activeTurn={state === "running"}
                projectTransitioning={false}
                workbenchError={null}
                chatError={null}
                availableBinding
                canPrompt
                discoveryPhase="ready"
                sessionHarnessId={state === "pre-turn" ? null : "fixture"}
                sessionModelId={null}
                selectedHarnessId="fixture"
                selectedModelId={
                  state === "pre-turn" ? "claude-sonnet-4-5-20250929" : null
                }
                sessionPromptUnavailable={false}
                sessionConfig={
                  state === "unknown" || state === "pre-turn" ? null : config
                }
                modePending={false}
                modeError={null}
                focused
                onSetSessionMode={async (id) => {
                  config = {
                    ...config,
                    modes: { ...config.modes!, currentModeId: id },
                  };
                  draw();
                }}
                onSetSessionConfigOption={async (id, value) => {
                  if ("value" in value)
                    config = {
                      ...config,
                      configOptions: config.configOptions.map((option) =>
                        option.id === id && option.type === "select"
                          ? { ...option, currentValue: value.value }
                          : option,
                      ),
                    };
                  draw();
                }}
                onSelectHarness={noop}
                onSelectModel={() => {}}
                missingBindingNotice={false}
                disabledReason={null}
                project={home ? null : project}
                projectScopeName={home ? project.name : null}
                usage={null}
                goal={{ kind: "unsupported" }}
                onSubmit={(event) => event.preventDefault()}
                onTextChange={() => {}}
                onRemoveContext={() => {}}
                onCancelTurn={state === "running" ? () => {} : null}
              />
            </section>
          </div>
        </div>
      </div>
    </div>
  );
}
function draw() {
  flushSync(() => root.render(<Host state={current.state} />));
}

Object.assign(window, {
  renderComposerParity(name: string, state: ParityState) {
    current = { name, state };
    useWorkbenchStore.setState({
      project,
      projects: [{ ...project, rootCount: 0, lastOpenedAt: 1 }],
      selectedHarnessId: "fixture",
      models:
        state === "pre-turn"
          ? [
              {
                id: "claude-sonnet-4-5-20250929",
                name: "Claude Sonnet 4 5 20250929",
              },
            ]
          : [],
      harnesses: harnessNames.map((label) => ({
        id: label === name ? "fixture" : label,
        name: label,
        description: "",
        source: "builtIn",
        installState: "available",
        transport: "acp",
        bindingId: "fixture",
        accountModelDiscovery: false,
        accessModes: [
          ...(state === "access"
            ? [
                {
                  id: "workspaceWrite" as const,
                  label: "Workspace edits",
                  description:
                    "Edits and commands run without asking inside the workspace and temp folders; no network access.",
                },
              ]
            : []),
          {
            id: "readOnly",
            label: "Read-only",
            description: "Read workspace files.",
            fallback: true,
          },
        ],
      })),
    });
    useSessionAccessModeStore.setState({
      modes: { fixture: "readOnly" },
      ready: true,
      error: null,
    });
    config = {
      sageAgentSessionId: "session-a",
      runtimeEpoch: "3",
      configRevision: "1",
      confirmedModel: { id: "confirmed", name: "Confirmed session model" },
      modes: {
        currentModeId: "default",
        availableModes: [
          { id: "default", name: "Default" },
          { id: "plan", name: "Plan" },
        ],
      },
      configOptions:
        state === "no-effort"
          ? []
          : [
              {
                id: "thinking",
                category: "effort",
                name: "Thinking",
                type: "select",
                currentValue: "high",
                ...(state === "no-default" ? {} : { defaultValue: "low" }),
                options: [
                  { value: "low", name: "Low" },
                  { value: "high", name: "High" },
                ],
              },
            ],
    };
    if (state === "models" || state === "model-collisions")
      config.configOptions.push({
        id: "model",
        name: "Model",
        type: "select",
        currentValue:
          state === "models"
            ? "claude-sonnet-4-5-20250929"
            : "claude-sonnet-4-20250514",
        options: [
          {
            group: "default",
            name: "Recommended by harness",
            options: [
              {
                value:
                  state === "models"
                    ? "claude-sonnet-4-5-20250929"
                    : "claude-sonnet-4-20250514",
                name:
                  state === "models"
                    ? "Claude Sonnet 4 5 20250929"
                    : "Claude Sonnet 4 20250514",
              },
            ],
          },
          {
            group: "other",
            name: "Other models",
            options:
              state === "models"
                ? [{ value: "gpt-5.4-mini", name: "gpt-5.4-mini" }]
                : [
                    {
                      value: "claude-sonnet-4-20250601",
                      name: "claude-sonnet-4-20250601",
                    },
                    {
                      value:
                        "private/claude-sonnet-4/workspace-specific-model-with-a-long-identifier",
                      name: "Claude Sonnet 4",
                    },
                  ],
          },
        ],
      });
    draw();
  },
});
