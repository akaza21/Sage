import { LoaderCircle } from "../../../ui/leanIcons";

export function ToolRunningIndicator() {
  return (
    <span
      className="acp-tool-running"
      role="status"
      aria-label="Running"
      aria-live="off"
    >
      <LoaderCircle size={12} aria-hidden="true" />
    </span>
  );
}
