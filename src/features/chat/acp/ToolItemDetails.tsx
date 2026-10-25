import { useAgentArtifactFollowStore } from "../../../app/agentArtifactFollowStore";
import type { SafeResourceLocation } from "../../../domain/acp";
import { harnessBrowseOwner } from "../../preview/harnessBrowse";
import type { HarnessBrowseUrl } from "../../preview/harnessBrowse";
import { ChatFileChip } from "./FileChip";
import { chatFileChipBasename } from "./chatFileChip";
import { CURATED_BROWSE_TOOL_NAME, invokeBrowseTool } from "./toolHost";
import type { ToolItemProps, OpenResourceTarget } from "./ToolItem";
import { ChatCodeBlock } from "./ChatCodeBlock";

/**
 * An integration result: a failure shows only the harness's message; a
 * result shows as a bounded code block, `json` when it parses as JSON.
 */
function ToolResult({
  item,
  output = item.payload.outputSummary,
  plain = false,
}: Pick<ToolItemProps, "item"> & { output?: string | null; plain?: boolean }) {
  if (!output) return null;
  if (item.toolState === "failed")
    return (
      <div className="acp-tool-error" role="alert">
        {output}
      </div>
    );
  let language = "plaintext";
  try {
    if (!plain && /^[[{]/.test(output.trim()) && JSON.parse(output))
      language = "json";
  } catch {
    // Not JSON (or clipped); shown as plain text.
  }
  return (
    <div className="acp-tool-result">
      <ChatCodeBlock language={language} source={output} />
    </div>
  );
}
function ResourceLocations({
  locations,
  target,
  onOpenResource,
}: {
  locations: readonly SafeResourceLocation[];
  target: OpenResourceTarget;
  onOpenResource?: ToolItemProps["onOpenResource"];
}) {
  if (locations.length === 0) return null;
  return (
    <ul className="acp-resource-list" aria-label="Referenced resources">
      {locations.map((location, index) => (
        <li key={`${location.kind}:${index}`}>
          {location.kind === "resource" ? (
            chatFileChipBasename(location.resource.relativePath) ? (
              onOpenResource ? (
                <button
                  type="button"
                  className="acp-resource-action"
                  aria-label={`Open ${target} ${location.resource.relativePath}`}
                  data-resource-root-id={location.resource.rootId}
                  data-resource-relative-path={location.resource.relativePath}
                  onClick={() => onOpenResource(location.resource, target)}
                >
                  <ChatFileChip path={location.resource.relativePath} />
                </button>
              ) : (
                <ChatFileChip path={location.resource.relativePath} />
              )
            ) : (
              <span className="acp-resource-unavailable">unavailable</span>
            )
          ) : (
            <span className="acp-resource-unavailable">
              {location.reason === "missing"
                ? "File location unavailable"
                : `${location.label} · unavailable`}
            </span>
          )}
        </li>
      ))}
    </ul>
  );
}

export function ToolItemDetails({
  item,
  browse,
  allowBrowse,
  onOpenResource,
}: Pick<ToolItemProps, "item" | "allowBrowse" | "onOpenResource"> & {
  browse: HarnessBrowseUrl | null;
}) {
  const presentation = item.payload.presentation;
  switch (presentation.kind) {
    case "execute":
    case "read":
    case "imageView":
    case "edit":
    case "delete":
    case "move":
      return (
        <ResourceLocations
          locations={presentation.resources}
          target="file"
          onOpenResource={onOpenResource}
        />
      );
    case "search":
      return (
        <>
          <p>{presentation.querySummary}</p>
          <ResourceLocations
            locations={presentation.resources}
            target="file"
            onOpenResource={onOpenResource}
          />
        </>
      );
    case "webSearch":
      // Codex s5/f0405: one plaintext block, the result or else the query.
      // A failed search never shows its query as an error alert.
      return (
        <ToolResult
          item={item}
          plain
          output={
            item.payload.outputSummary ||
            (item.toolState === "failed" ? null : item.payload.inputSummary)
          }
        />
      );
    case "mcp":
      return <ToolResult item={item} />;
    case "subagent":
      return <p>Harness-owned activity</p>;
    case "fetch": {
      return (
        <>
          {presentation.summary ? <p>{presentation.summary}</p> : null}
          {browse && allowBrowse !== false ? (
            <button
              type="button"
              className="acp-resource-action"
              onClick={() => {
                const owner = harnessBrowseOwner();
                if (
                  !owner ||
                  owner.projectId !== item.projectId ||
                  owner.chatId !== item.chatId ||
                  owner.sessionId !== item.sessionId ||
                  owner.runtimeEpoch !== item.runtimeEpoch
                ) {
                  return;
                }
                const opened = invokeBrowseTool(CURATED_BROWSE_TOOL_NAME, {
                  owner,
                  url: browse.href,
                });
                if (opened) {
                  useAgentArtifactFollowStore
                    .getState()
                    .pin(owner, { kind: "preview" });
                }
              }}
            >
              Open in Sage browser
            </button>
          ) : null}
        </>
      );
    }
    case "think":
      return presentation.summary ? <p>{presentation.summary}</p> : null;
    case "switchMode":
      return presentation.modeLabel ? <p>{presentation.modeLabel}</p> : null;
    case "other":
      return (
        <>
          {presentation.rawKind &&
          presentation.rawKind.trim() !== presentation.label.trim() ? (
            <span className="acp-tool-card__raw-kind">
              {presentation.rawKind}
            </span>
          ) : null}
          {presentation.summary ? <p>{presentation.summary}</p> : null}
        </>
      );
  }
}
