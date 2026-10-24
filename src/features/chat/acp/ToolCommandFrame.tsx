export function ToolCommandFrame({
  label,
  commandLine,
  output,
  descriptionId,
  outcome,
  danger = false,
}: {
  label: string;
  commandLine: string | null;
  output: string | null;
  descriptionId?: string;
  outcome?: string | null;
  danger?: boolean;
}) {
  return (
    <div className="acp-tool-command-frame">
      <p className="acp-tool-command__disclosure" role="note">
        {label}
      </p>
      {commandLine ? (
        <pre
          className="acp-tool-command acp-tool-command--line"
          aria-label="Command line"
          tabIndex={0}
        >
          {commandLine}
        </pre>
      ) : null}
      {output !== null ? (
        <pre
          id={descriptionId}
          className="acp-tool-command acp-tool-command--output"
          aria-label={label === "Shell" ? "Command output" : "Tool output"}
          tabIndex={0}
        >
          {output || "No output"}
        </pre>
      ) : null}
      {outcome ? (
        <p
          className={`acp-tool-command__outcome${danger ? " acp-tool-command__outcome--danger" : ""}`}
          role="status"
          aria-live="off"
        >
          {outcome}
        </p>
      ) : null}
    </div>
  );
}
