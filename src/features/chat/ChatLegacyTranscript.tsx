import { Check } from "../../ui/leanIcons";

import type { ChatMessage } from "../../domain/chat";
import { EditSummary } from "./EditSummary";
import { chatTimestamp } from "./chatTimestamp";

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
