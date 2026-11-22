import { useState } from "react";
import { createRoot } from "react-dom/client";
import { ChatTranscript } from "../../../src/features/chat/ChatTranscript";
import { SessionNoticeStrip } from "../../../src/features/chat/acp/SessionNoticeStrip";
import {
  page,
  textItem,
} from "../../../src/features/chat/acp/acpProjection.fixture";
import { safeSessionNotices } from "../../../src/services/acpSessionNotices";
import type { AcpItem } from "../../../src/domain/acp";
import "../../../src/styles/global.css";
import "../../../src/styles/chat-surface.css";
import "../../../src/styles/changed-files-summary.css";
import "../../../src/styles/shared-controls.css";
import "./noticeTranscriptFixture.css";

const scenario = new URLSearchParams(location.search).get("scenario");
const projection = page("failed", {
  kind: "harnessExited",
  message: "The harness exited unexpectedly.",
});
const turn = projection.turns[0];
turn.items = [
  { ...textItem(), payload: { role: "user", text: "Inspect this project" } },
  {
    ...textItem(),
    id: "failure-item",
    kind: "error",
    origin: "primary",
    lifecycle: "failed",
    payload: {
      code: "harnessExited",
      message: "The harness exited unexpectedly.",
      recoverable: true,
    },
  },
] as AcpItem[];
if (scenario === "boundary") projection.nextCursor = "earlier-page" as never;

function Fixture() {
  const [reloading, setReloading] = useState(false);
  return (
    <main className="notice-fixture">
      {scenario === "session" ? (
        <SessionNoticeStrip
          session="fixture-session"
          notices={safeSessionNotices([
            {
              identity: "mcp-start:calendar",
              level: "info",
              text: "Calendar tools are unavailable.",
              source: "runtime",
            },
            {
              identity: "mcp-start:calendar",
              level: "warning",
              text: "Calendar tools are unavailable.",
              source: "adapter",
            },
            {
              identity: "mcp-start:mail",
              level: "info",
              text: "Mail tools are unavailable.",
              source: "runtime",
            },
          ])}
        />
      ) : (
        <ChatTranscript
          transcriptRef={{ current: null }}
          contentRef={{ current: null }}
          endRef={{ current: null }}
          onScroll={() => {}}
          projectTransitioning={scenario === "transition"}
          useNativeAcp
          page={
            scenario === "history" || scenario === "loading" ? null : projection
          }
          historyLoading={scenario === "loading" || reloading}
          firstPageError={
            scenario === "history" && !reloading ? "History unavailable" : null
          }
          focused
          projectRoots={[]}
          retentionIncomplete={scenario === "boundary"}
          onOpenResource={() => {}}
          hasEarlier={scenario === "boundary"}
          loadingEarlier={false}
          historyError={null}
          onLoadEarlier={() => setReloading(true)}
          messages={[]}
          onRetry={() => setReloading(true)}
          developmentSimulator={false}
          showJumpToLatest={false}
          onJumpToLatest={() => {}}
        />
      )}
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<Fixture />);
