import type { HandoffSummary } from "../../../domain/acp";

export function ContextHandoffIndicator({
  summary,
}: {
  summary: HandoffSummary;
}) {
  const { state, carriedItems, carriedBytes, omissions } = summary;
  const omitted =
    omissions.unsupportedKind +
    omissions.oversize +
    omissions.scanLimit +
    omissions.budget;
  return (
    <span role="status" className="chat-subtle">
      {state === "unknown"
        ? "Delivery unknown. Prior execution may have occurred; replace the native session before retrying."
        : state === "inline"
          ? `${carriedItems} items carried (${carriedBytes} bytes); ${omitted} omitted.`
          : `Context handoff pending: ${carriedItems} selected; ${omitted} omitted.`}
    </span>
  );
}
