import { type BrowserOperationProjection } from "../../../domain/browserOperation";
import { useContext } from "react";
import { WorkedActivityVisibility } from "./WorkedActivityVisibility";

const MAX_VISIBLE_BROWSER_OPERATIONS = 32;

export function BrowserOperations({
  operations,
  collapsed,
}: {
  operations: readonly BrowserOperationProjection[];
  collapsed: number;
}) {
  const expanded = useContext(WorkedActivityVisibility);
  if (!expanded) return null;
  const visible = operations.slice(-MAX_VISIBLE_BROWSER_OPERATIONS);
  const earlier = Math.max(0, collapsed) + operations.length - visible.length;
  if (visible.length === 0 && earlier === 0) return null;
  return (
    <section className="acp-browser-operations" aria-label="Browser actions">
      <h3 className="sr-only">Browser actions</h3>
      {earlier > 0 ? (
        <p className="acp-browser-operations__earlier">
          +{earlier} earlier browser action{earlier === 1 ? "" : "s"}
        </p>
      ) : null}
      {visible.map((operation) => (
        <BrowserOperationItem
          key={operation.operationId}
          operation={operation}
        />
      ))}
    </section>
  );
}

type ActionWords = { ongoing: string; completed: string; imperative: string };

function actionWords(operation: BrowserOperationProjection): ActionWords {
  let host = operation.origin;
  try {
    host = new URL(operation.origin).host;
  } catch {
    // Keep the backend's bounded origin label when no URL is available.
  }
  const onOrigin = (verb: string) =>
    host === "unavailable" ? verb : `${verb} on ${host}`;
  switch (operation.operation) {
    case "unsupported":
      return {
        ongoing: "Unsupported browser operation",
        completed: "Unsupported browser operation",
        imperative: "perform unsupported browser operation",
      };
    case "openTab":
      return {
        ongoing: `Opening tab for ${host}`,
        completed: `Opened tab for ${host}`,
        imperative: `open tab for ${host}`,
      };
    case "navigate": {
      return {
        ongoing: `Navigating to ${host}`,
        completed: `Navigated to ${host}`,
        imperative: `navigate to ${host}`,
      };
    }
    case "snapshotPage":
      return {
        ongoing: onOrigin("Reading page"),
        completed: onOrigin("Read page"),
        imperative: onOrigin("read page"),
      };
    case "screenshot":
      return {
        ongoing: onOrigin("Capturing page"),
        completed: onOrigin("Captured page"),
        imperative: onOrigin("capture page"),
      };
    case "readConsole":
      return {
        ongoing: onOrigin("Reading console"),
        completed: onOrigin("Read console"),
        imperative: onOrigin("read console"),
      };
    case "readNetwork":
      return {
        ongoing: onOrigin("Reading network"),
        completed: onOrigin("Read network"),
        imperative: onOrigin("read network"),
      };
    case "waitFor":
      return {
        ongoing: onOrigin("Waiting for page condition"),
        completed: onOrigin("Waited for page condition"),
        imperative: onOrigin("wait for page condition"),
      };
    case "act": {
      const clickLabel = operation.summary.match(
        /^Click "([^"\r\n]{1,80})"$/,
      )?.[1];
      const verbs =
        {
          "Click element": [
            "Clicking element",
            "Clicked element",
            "click element",
          ],
          "Fill field": [
            "Typing into field",
            "Typed into field",
            "type into field",
          ],
          "Press key": ["Pressing key", "Pressed key", "press key"],
          "Scroll page": ["Scrolling page", "Scrolled page", "scroll page"],
        }[operation.summary] ??
        (clickLabel
          ? [
              `Clicking "${clickLabel}"`,
              `Clicked "${clickLabel}"`,
              `click "${clickLabel}"`,
            ]
          : operation.summary.startsWith("Fill field")
            ? ["Typing into field", "Typed into field", "type into field"]
            : ["Using page", "Used page", "use page"]);
      return {
        ongoing: onOrigin(verbs[0]),
        completed: onOrigin(verbs[1]),
        imperative: onOrigin(verbs[2]),
      };
    }
  }
}

function BrowserOperationItem({
  operation,
}: {
  operation: BrowserOperationProjection;
}) {
  const words = actionWords(operation);
  const title =
    operation.phase === "started"
      ? `${words.ongoing}…`
      : operation.phase === "completed"
        ? words.completed
        : operation.phase === "denied"
          ? `Blocked: ${words.imperative}`
          : operation.phase === "failed"
            ? `Couldn't ${words.imperative}`
            : `${words.imperative[0].toUpperCase()}${words.imperative.slice(1)} — result unknown`;
  const status =
    operation.phase === "outcomeUnknown"
      ? "Outcome unknown"
      : operation.phase === "denied"
        ? "Denied"
        : operation.phase === "started"
          ? "In progress"
          : operation.phase === "failed"
            ? "Failed"
            : "Completed";
  const isFill =
    operation.operation === "act" && operation.summary.startsWith("Fill field");
  const isScreenshot = operation.operation === "screenshot";
  const detailSummary = isFill
    ? "Fill field"
    : isScreenshot
      ? "Capture screenshot"
      : operation.operation === "navigate"
        ? "Navigate to origin"
        : operation.summary;
  const resultSummary = isScreenshot
    ? /^Screenshot captured \(\d{1,4}×\d{1,4}\)$/.test(
        operation.resultSummary ?? "",
      )
      ? operation.resultSummary
      : status
    : isFill
      ? operation.resultSummary === "Action not applied"
        ? "Action not applied"
        : operation.resultSummary === "Action applied"
          ? "Action applied"
          : status
      : operation.resultSummary;
  return (
    <article
      className="acp-activity-card acp-activity-card--browser"
      role="group"
      aria-label={`${title} · ${status}`}
      data-activity-kind="browser"
      data-activity-state={operation.phase}
    >
      <details className="acp-browser-operation acp-details">
        <summary>
          <strong>{title}</strong>
          <span
            className="acp-state"
            role="status"
            aria-label={
              operation.phase === "outcomeUnknown"
                ? "Warning: Outcome unknown"
                : status
            }
            aria-live="off"
          >
            {status}
          </span>
        </summary>
        <div className="acp-browser-operation__detail">
          <p>Tab {operation.tabId}</p>
          <p>Origin {operation.origin}</p>
          <p>{detailSummary}</p>
          {resultSummary ? <p>{resultSummary}</p> : null}
        </div>
      </details>
    </article>
  );
}
