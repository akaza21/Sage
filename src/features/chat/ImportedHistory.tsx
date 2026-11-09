import { useEffect, useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import {
  readImportedHistory,
  type ImportedHistory as History,
} from "../../services/nativeSessionImport";
import { ItemView } from "./acp/TurnActivityItem";

// The session picker shares this chunk: both load only for an import.
export { ImportSessionDialog } from "../../workbench/ImportSessionDialog";

/** The imported session's history, read-only, above where Sage continues it. */
export function ImportedHistory({
  chatId,
  sessionId,
}: {
  chatId: string;
  sessionId: string;
}) {
  const projectId = useWorkbenchStore((state) => state.project?.id);
  const key = `${projectId}/${chatId}/${sessionId}`;
  const [read, setRead] = useState<{
    key: string;
    history: History | "failed" | null;
  } | null>(null);
  useEffect(() => {
    if (!projectId) return;
    let live = true;
    const settle = (history: History | "failed" | null) => {
      if (live) setRead({ key, history });
    };
    readImportedHistory({ projectId, chatId, sessionId, runtimeEpoch: "1" })
      .then(settle)
      .catch(() => settle("failed"));
    return () => {
      live = false;
    };
  }, [key, projectId, chatId, sessionId]);
  const history = read?.key === key ? read.history : null;
  return (
    <>
      {history === "failed" ? (
        <p className="acp-history-notice" role="status">
          The imported history couldn&apos;t be shown.
        </p>
      ) : history ? (
        <div aria-label="History from the imported session" role="group">
          {history.retentionLimited ? (
            <p className="acp-history-notice">
              Earlier history isn&apos;t shown.
            </p>
          ) : null}
          {history.turns.map((turn) => (
            <div key={turn.id} className="acp-turn__items">
              {turn.items.map((item) => (
                <ItemView
                  key={item.id}
                  item={item}
                  interactive={false}
                  neutralTextRole={
                    item.kind === "text" && item.payload.role === "system"
                  }
                />
              ))}
            </div>
          ))}
        </div>
      ) : null}
      <p className="acp-history-notice">Continued in Sage</p>
    </>
  );
}
