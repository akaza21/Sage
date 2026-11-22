import { useEffect, useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import {
  readImportedHistory,
  type ImportedHistory as History,
} from "../../services/nativeSessionImport";
import { NoticePart } from "./lazyParts";

export { ImportSessionDialog } from "../../workbench/ImportSessionDialog";

export function ImportedHistory({ chatId }: { chatId: string }) {
  const projectId = useWorkbenchStore((state) => state.project?.id);
  const key = `${projectId}/${chatId}`;
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
    readImportedHistory(projectId, chatId)
      .then(settle)
      .catch(() => settle("failed"));
    return () => {
      live = false;
    };
  }, [key, projectId, chatId]);
  const history = read?.key === key ? read.history : null;
  return (
    <>
      {history === "failed" ? (
        <p className="acp-history-notice" role="status">
          The imported history couldn&apos;t be shown.
        </p>
      ) : (
        history && (
          <NoticePart
            part="ConversationView"
            view="ImportedHistoryView"
            chatId={chatId}
            history={history}
          />
        )
      )}
      <p className="acp-history-notice">Continued in Sage</p>
    </>
  );
}
