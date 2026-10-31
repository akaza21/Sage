import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import { useInheritedHistory } from "./useInheritedHistory";

export function InheritedChatHistory({
  parentChatId,
  forkTurnId,
}: {
  parentChatId: string;
  forkTurnId?: string;
}) {
  const inherited = useInheritedHistory(parentChatId, forkTurnId);
  return (
    <>
      {inherited.page ? (
        // Parent Turns are read-only except answer actions; the fork owns the sole navigator, find and virtual list.
        <div
          className="chat-fork-inherited"
          aria-label="History from the parent chat"
          role="group"
        >
          {inherited.earlier ? (
            <p className="acp-history-notice">
              Earlier history isn&apos;t shown.{" "}
              <button
                type="button"
                disabled={inherited.loading}
                onClick={inherited.loadEarlier}
              >
                Load earlier parent history
              </button>
            </p>
          ) : null}
          <LazyModuleBoundary
            label="Inherited history"
            load={loadInheritedTurns}
            render={(InheritedTurns) => (
              <InheritedTurns page={inherited.page!} />
            )}
          />
        </div>
      ) : null}
      {inherited.loading ? (
        <p role="status">Loading inherited history…</p>
      ) : inherited.error ? (
        <p role="alert">
          Could not load inherited history: {inherited.error}{" "}
          <button type="button" onClick={inherited.loadEarlier}>
            Retry inherited history
          </button>
        </p>
      ) : inherited.missing ? (
        <p role="status">
          Parent history or fork point is no longer available.
        </p>
      ) : !inherited.page && inherited.earlier ? (
        <p role="status">
          The fork point is in earlier parent history.{" "}
          <button type="button" onClick={inherited.loadEarlier}>
            Continue finding fork point
          </button>
        </p>
      ) : null}
    </>
  );
}
const loadInheritedTurns = () =>
  import("./acp/AcpChatPresentation").then(({ InheritedTurns }) => ({
    default: InheritedTurns,
  }));
