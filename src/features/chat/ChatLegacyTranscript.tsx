import { Check, Files, ChevronRight } from "../../ui/leanIcons";
import { type ChatMessage } from "../../domain/chat";
import { chatTimestamp } from "./chatTimestamp";
import { useWorkbenchStore } from "../../app/store";
import { resourceKey } from "../../domain/resourceIdentity";

export function ChatLegacyTranscript({
  messages,
  developmentSimulator,
}: {
  messages: readonly ChatMessage[];
  developmentSimulator: boolean;
}) {
  return (
    <>
      {developmentSimulator ? (
        <div className="project-intro">
          <span className="project-intro__icon">
            <Check size={17} aria-hidden="true" />
          </span>
          <div>
            <strong>Development simulator</strong>
            <p>
              This test-only harness exercises the workbench without connecting
              a production agent.
            </p>
          </div>
        </div>
      ) : null}
      {messages.map((message) => {
        const timestamp = chatTimestamp(message.createdAt);
        return (
          <article
            className={`message message--${message.role}`}
            key={message.id}
          >
            <header className="message__meta">
              <span className="message__role">
                {message.role === "user" ? "You" : "Sage"}
              </span>
              {timestamp ? (
                <time dateTime={timestamp.dateTime} title={timestamp.dateTime}>
                  {timestamp.label}
                </time>
              ) : null}
            </header>
            <p>{message.text}</p>
          </article>
        );
      })}
      {import.meta.env.DEV && developmentSimulator ? <EditSummary /> : null}
    </>
  );
}

function EditSummary() {
  const changes = useWorkbenchStore((state) => state.turnChanges);
  const openDiff = useWorkbenchStore((state) => state.openDiff);

  if (changes.length === 0) {
    return null;
  }

  const additions = changes.reduce(
    (total, change) => total + (change.additions ?? 0),
    0,
  );
  const deletions = changes.reduce(
    (total, change) => total + (change.deletions ?? 0),
    0,
  );

  return (
    <section className="edit-summary" aria-label="Files edited in this turn">
      <div className="edit-summary__header">
        <span className="edit-summary__icon">
          <Files size={14} aria-hidden="true" />
        </span>
        <strong>
          Edited {changes.length} {changes.length === 1 ? "file" : "files"}
        </strong>
        <span className="edit-summary__totals" aria-label="Line changes">
          <span>+{additions}</span>
          <span>-{deletions}</span>
        </span>
      </div>
      <div className="edit-summary__files">
        {changes.map((change) => (
          <button
            className="edit-file"
            key={resourceKey(change.resource)}
            type="button"
            aria-label={`Open diff for ${change.resource.relativePath}`}
            onClick={() => openDiff(change.resource)}
          >
            <span className="edit-file__path">
              {change.resource.relativePath}
            </span>
            <span className="edit-file__counts">
              <span>
                {change.additions === null ? "—" : `+${change.additions}`}
              </span>
              <span>
                {change.deletions === null ? "—" : `-${change.deletions}`}
              </span>
            </span>
            <ChevronRight size={14} aria-hidden="true" />
          </button>
        ))}
      </div>
    </section>
  );
}
