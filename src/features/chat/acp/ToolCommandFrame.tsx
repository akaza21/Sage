export function ToolCommandFrame({
  label,
  commandLine,
  output,
  descriptionId,
  outcome,
}: {
  label: string;
  commandLine: string | null;
  output: string | null;
  descriptionId?: string;
  outcome?: string | null;
}) {
  return (
    <div className="acp-tool-command-frame">
      <p className="acp-tool-command__disclosure" role="note">
        {label}
        {outcome ? (
          <span
            className="acp-tool-command__outcome"
            role="status"
            aria-live="off"
          >
            {outcome}
          </span>
        ) : null}
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
    </div>
  );
}
