import { createRoot } from "react-dom/client";
import type { AcpItem } from "../../../src/domain/acp";
import { AcpChatPresentation } from "../../../src/features/chat/acp/AcpChatPresentation";
import { ChatTranscriptFooter } from "../../../src/features/chat/ChatTranscriptFooter";
import {
  page,
  textItem,
} from "../../../src/features/chat/acp/acpProjection.fixture";
import { parityTool } from "../../../src/features/chat/acp/activityParity.fixture";
import { snapshotPage } from "../../../src/app/agentDiff.fixture";
import "../../../src/styles/global.css";
import "../../../src/styles/changed-files-summary.css";
import "../../../src/features/chat/acp/acp-chat.css";
import "../../../src/features/chat/acp/acp-chat-activities.css";
import "../../../src/features/chat/acp/acp-activity-cards.css";
import "../../../src/features/chat/acp/acp-activity-details.css";
import "../../../src/features/chat/acp/acp-child-activity.css";
import "../../../src/features/chat/chat-transcript-polish.css";

const scenario = new URLSearchParams(location.search).get("scenario");
const stopped = scenario === "stopped";
const running = scenario === "running" || scenario === "thinking";
const projection =
  scenario === "edits"
    ? snapshotPage()
    : page(
        stopped ? "cancelled" : running ? "running" : "completed",
        stopped ? { kind: "cancelledByUser" } : undefined,
      );
projection.turns[0].createdAt = 1_000;
projection.turns[0].terminalAt = running ? null : 6_000;
const answer = {
  ...textItem(),
  ...(scenario === "mermaid"
    ? {
        payload: {
          role: "assistant",
          text: "```mermaid\nflowchart LR\nBinding --> Pump\nPump --> SQLite\n```",
        },
      }
    : {}),
  ordinal: 20,
  lifecycle: stopped ? "cancelled" : "completed",
} as AcpItem;
const prompt = {
  ...textItem(),
  id: "prompt",
  ordinal: 0,
  payload: { role: "user", text: "Inspect this project" },
} as AcpItem;
const command = parityTool("execute", 1, running ? "inProgress" : "completed");
if (
  scenario === "wrapped-command" &&
  command.payload.presentation.kind === "execute"
) {
  command.payload.presentation.summary =
    '[path] -NoProfile -Command "Get-Content -LiteralPath README.md --token [redacted]"';
  command.payload.inputSummary = command.payload.presentation.summary;
  command.payload.outputSummary =
    "PowerShell profile could not be loaded.\n# README";
}
if (running && command.payload.presentation.kind === "execute") {
  command.payload.presentation.summary =
    "git diff --stat -- src/features/chat/acp/PrimaryActivities.tsx";
}
const notice = {
  ...textItem(),
  id: "notice",
  ordinal: 2,
  kind: "compactionNotice",
  lifecycle: "failed",
  payload: {},
} as AcpItem;
const children = ["Reviewer", "Builder"].map((label, index) => ({
  ...textItem(),
  id: `agent-${index}`,
  ordinal: index + 1,
  kind: "collaboration",
  lifecycle: "completed",
  projectId: projection.projectId,
  chatId: projection.chatId,
  sessionId: "session-1",
  turnId: "turn-1",
  runtimeEpoch: "2",
  origin: "primary",
  activityId: null,
  payload: {
    label: "spawnAgent",
    summary: "",
    activityId: null,
    operation: "spawnAgent",
    agentKeys: [`agent-${index}`],
    receiverLabels: [label],
    agentStates: [[`agent-${index}`, "completed"]],
  },
})) as AcpItem[];
const activities =
  scenario === "notice"
    ? [
        {
          ...textItem(),
          id: "browser-notice",
          ordinal: 1,
          kind: "error",
          lifecycle: "completed",
          payload: {
            code: "browserMcpUnavailable",
            message:
              "Browser tools are unavailable. You can continue chatting.",
            recoverable: false,
          },
        } as AcpItem,
      ]
    : scenario === "edits"
      ? Array.from({ length: 5 }, (_, index) => {
          const saved = snapshotPage().turns[0].items[0];
          if (saved.kind !== "fileChangeDiff")
            throw new Error("Expected edit fixture");
          return {
            ...saved,
            id: `edit-${index}`,
            ordinal: index + 1,
            payload: {
              ...saved.payload,
              changes: [
                {
                  ...saved.payload.changes[0],
                  resource: {
                    kind: "resource",
                    resource: {
                      rootId: "root-a",
                      relativePath: `src/file-${index}.ts`,
                    },
                  },
                  additions: index + 1,
                  deletions: index,
                },
              ],
            },
          } as AcpItem;
        })
      : scenario === "thinking"
        ? [
            {
              ...textItem(),
              id: "prose",
              ordinal: 1,
              lifecycle: "streaming",
              payload: { role: "assistant", text: "Checking the project now." },
            } as AcpItem,
            {
              ...textItem(),
              id: "thought",
              ordinal: 2,
              kind: "thoughtSummary",
              lifecycle: "streaming",
              payload: { summary: "Inspecting the dependencies." },
            } as AcpItem,
          ]
        : scenario === "mermaid"
          ? []
          : scenario === "reads"
            ? [parityTool("read"), parityTool("read", 2), parityTool("read", 3)]
            : scenario === "compaction"
              ? [notice]
              : scenario === "subagents"
                ? children
                : scenario === "commands"
                  ? [
                      command,
                      parityTool("execute", 2),
                      parityTool("execute", 3),
                    ]
                  : scenario === "simple" || stopped
                    ? []
                    : [command];
projection.turns[0].items = [
  prompt,
  ...activities,
  ...(running || scenario === "tool-only" ? [] : [answer]),
];
createRoot(document.getElementById("root")!).render(
  <div
    className="chat-surface message-list__content"
    style={
      {
        padding: 16,
        boxSizing: "border-box",
        width: "min(100%, 660px)",
        "--reading-measure": "660px",
      } as React.CSSProperties
    }
  >
    <AcpChatPresentation
      page={projection}
      hasEarlier={false}
      loadingEarlier={false}
      historyError={null}
      projectRoots={[]}
      retentionIncomplete={false}
      onOpenResource={(resource, target) => {
        document.getElementById("opened-diff")!.textContent =
          `${target}:${resource.relativePath}`;
      }}
    />
    <ChatTranscriptFooter
      useNativeAcp
      page={projection}
      messages={[]}
      onRetry={() => undefined}
    />
    <output id="opened-diff" aria-label="Opened diff" />
  </div>,
);
