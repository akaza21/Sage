import type { BrowserOperationProjection } from "../../../domain/browserOperation";

type ActionWords = { ongoing: string; completed: string; imperative: string };

/** Rust's result for a call refused while the user holds the tab (`browser_operation.rs`). */
const USER_CONTROL_RESULT = "Paused: you took over this tab";

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
    case "cdp": {
      // Rust's summary is "CDP Domain.command", the method already checked.
      const method = operation.summary.replace(/^CDP /, "");
      return {
        ongoing: onOrigin(`Running ${method}`),
        completed: onOrigin(`Ran ${method}`),
        imperative: onOrigin(`run ${method}`),
      };
    }
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

export function BrowserOperationItem({
  operation,
}: {
  operation: BrowserOperationProjection;
}) {
  const words = actionWords(operation);
  const paused =
    operation.phase === "denied" &&
    operation.resultSummary === USER_CONTROL_RESULT;
  const title =
    operation.phase === "started"
      ? `${words.ongoing}…`
      : operation.phase === "completed"
        ? words.completed
        : paused
          ? `Paused: ${words.imperative}`
          : operation.phase === "denied"
            ? `Blocked: ${words.imperative}`
            : operation.phase === "failed"
              ? `Couldn't ${words.imperative}`
              : `${words.imperative[0].toUpperCase()}${words.imperative.slice(1)} — result unknown`;
  const status =
    operation.phase === "outcomeUnknown"
      ? "Outcome unknown"
      : paused
        ? "Paused"
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
