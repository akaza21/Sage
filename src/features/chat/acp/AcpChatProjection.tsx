import type {
  AcpChatPage,
  AcpItem,
  FileChangeProjection,
  TurnProjection,
} from "../../../domain/acp";
import type { ResourceRef } from "../../../domain/workbench";

import { CollaborationItem } from "./CollaborationItem";
import { HarnessActivity } from "./HarnessActivity";
import { PermissionItem } from "./PermissionItem";
import { ToolItem, type OpenResourceTarget } from "./ToolItem";
import { UnknownItem } from "./UnknownItem";
import "./acp-chat.css";

interface AcpChatProjectionProps {
  page: AcpChatPage;
  active: boolean;
  onOpenResource?: (resource: ResourceRef, target: OpenResourceTarget) => void;
  hasEarlier?: boolean;
  loadingEarlier?: boolean;
  historyError?: string | null;
  onLoadEarlier?: () => void;
}

const turnStateLabels = {
  queued: "Queued",
  sending: "Sending",
  running: "Running",
  waitingModel: "Waiting for model",
  waitingTool: "Waiting for tool",
  waitingApproval: "Waiting for approval",
  waitingUser: "Waiting for user",
  cancelling: "Cancelling",
  completed: "Completed",
  interrupted: "Interrupted",
  cancelled: "Cancelled",
  failed: "Failed",
  deliveryUnknown: "Delivery unknown",
} as const;

function ResourceChange({
  change,
  onOpenResource,
}: {
  change: FileChangeProjection;
  onOpenResource?: AcpChatProjectionProps["onOpenResource"];
}) {
  const resource = change.resource;
  return (
    <li>
      {resource.kind === "resource" ? (
        onOpenResource ? (
          <button
            type="button"
            className="acp-resource-action"
            aria-label={`Open diff ${resource.resource.relativePath}`}
            onClick={() => onOpenResource(resource.resource, "diff")}
          >
            {resource.resource.relativePath}
          </button>
        ) : (
          <span>{resource.resource.relativePath}</span>
        )
      ) : (
        <span className="acp-resource-unavailable">
          {resource.label} · unavailable
        </span>
      )}
      <span className="acp-change-operation">{change.operation}</span>
      <span>{change.summary}</span>
    </li>
  );
}

function ItemView({
  item,
  onOpenResource,
}: {
  item: AcpItem;
  onOpenResource?: AcpChatProjectionProps["onOpenResource"];
}) {
  switch (item.kind) {
    case "text":
      return (
        <article className={`acp-message acp-message--${item.payload.role}`}>
          <span className="acp-message__role">
            {item.payload.role === "assistant"
              ? "Agent"
              : item.payload.role === "user"
                ? "You"
                : "System"}
          </span>
          <p>{item.payload.text}</p>
        </article>
      );
    case "thoughtSummary":
      return (
        <details className="acp-details acp-thought">
          <summary>Thought summary</summary>
          <p>{item.payload.summary}</p>
        </details>
      );
    case "plan":
      return (
        <section className="acp-plan" aria-label="Agent plan">
          <strong>Plan</strong>
          <ol>
            {item.payload.entries.map((entry, index) => (
              <li key={`${index}:${entry.label}`}>
                <span>{entry.label}</span>
                <span>{entry.state}</span>
              </li>
            ))}
          </ol>
        </section>
      );
    case "toolCall":
    case "toolUpdateResult":
      return <ToolItem item={item} onOpenResource={onOpenResource} />;
    case "fileChangeDiff":
      return (
        <section className="acp-file-changes" aria-label="Files changed">
          <strong>Changed files</strong>
          <ul>
            {item.payload.changes.map((change, index) => (
              <ResourceChange
                key={`${index}:${change.summary}`}
                change={change}
                onOpenResource={onOpenResource}
              />
            ))}
          </ul>
          {item.payload.truncated ? <span>Summary truncated</span> : null}
        </section>
      );
    case "usage":
      return (
        <p className="acp-usage">
          Usage: {item.payload.totalTokens?.toLocaleString() ?? "not reported"}{" "}
          tokens
        </p>
      );
    case "permissionRequest":
      return <PermissionItem item={item} />;
    case "error":
      return (
        <article className="acp-error" role="alert">
          <strong>{item.payload.code}</strong>
          <p>{item.payload.message}</p>
        </article>
      );
    case "unsupported":
      return <UnknownItem item={item} />;
    case "collaboration":
      return <CollaborationItem item={item} />;
  }
}

function Turn({
  turn,
  onOpenResource,
}: {
  turn: TurnProjection;
  onOpenResource?: AcpChatProjectionProps["onOpenResource"];
}) {
  const primaryItems = turn.items.filter((item) => item.origin === "primary");
  const status = turnStateLabels[turn.state];
  return (
    <section className="acp-turn" aria-label={`Turn ${turn.ordinal}`}>
      <header className="acp-turn__header">
        <span
          className="acp-state"
          role="status"
          aria-label={status}
          aria-live="polite"
          aria-atomic="true"
          data-turn-status
          tabIndex={-1}
        >
          {status}
        </span>
      </header>
      <div className="acp-turn__items">
        {primaryItems.map((item) => (
          <ItemView key={item.id} item={item} onOpenResource={onOpenResource} />
        ))}
        {turn.activities.map((activity) => (
          <HarnessActivity
            key={activity.id}
            activity={activity}
            items={turn.items}
            renderItem={(item) => (
              <ItemView item={item} onOpenResource={onOpenResource} />
            )}
          />
        ))}
      </div>
    </section>
  );
}

export function AcpChatProjection({
  page,
  active,
  onOpenResource,
  hasEarlier = false,
  loadingEarlier = false,
  historyError = null,
  onLoadEarlier,
}: AcpChatProjectionProps) {
  if (!active) return null;
  return (
    <section
      className="acp-chat-projection"
      aria-label={`Agent chat ${page.title}`}
      data-projection-revision={page.projectionRevision}
    >
      <h2 className="acp-chat-projection__title">{page.title}</h2>
      {hasEarlier && onLoadEarlier ? (
        <button
          type="button"
          className="acp-history-action"
          disabled={loadingEarlier}
          onClick={onLoadEarlier}
        >
          {loadingEarlier ? "Loading earlier history…" : "Load earlier history"}
        </button>
      ) : null}
      {historyError ? (
        <p className="acp-history-error" role="alert">
          Earlier history could not be loaded: {historyError}
        </p>
      ) : null}
      {page.turns.length === 0 ? (
        <p className="acp-empty">No turns in this chat yet.</p>
      ) : (
        page.turns.map((turn) => (
          <Turn key={turn.id} turn={turn} onOpenResource={onOpenResource} />
        ))
      )}
    </section>
  );
}
