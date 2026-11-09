import { useState } from "react";
import type { AcpUsageRequest } from "../../../domain/acpUsage";
import type { SafeResourceLocation } from "../../../domain/acpProjection";
import { errorMessage } from "../../../domain/errors";
import {
  acpBackgroundTerminals,
  type BackgroundTerminal,
} from "../../../services/acpBackgroundTerminals";
import "./background-terminals.css";

function place(
  cwd: SafeResourceLocation | null,
  rootLabels: Readonly<Record<string, string>>,
): string | null {
  if (!cwd) return null;
  if (cwd.kind === "unavailable") return cwd.label;
  const { rootId, relativePath } = cwd.resource;
  return (
    [rootLabels[rootId], relativePath].filter(Boolean).join("/") ||
    "Project root"
  );
}

/** The session's running background terminals, each with its own Stop. */
export function BackgroundTerminals({
  request,
  terminals,
  rootLabels,
  onChange,
}: {
  request: AcpUsageRequest;
  terminals: readonly BackgroundTerminal[];
  rootLabels: Readonly<Record<string, string>>;
  onChange: (terminals: readonly BackgroundTerminal[]) => void;
}) {
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const stop = (handle: string | null) => {
    setStopping(true);
    setError(null);
    acpBackgroundTerminals
      .stop(request, handle)
      .then(onChange, (cause: unknown) => setError(errorMessage(cause)))
      .finally(() => setStopping(false));
  };
  return (
    <details className="acp-background-terminals">
      <summary>Background terminals ({terminals.length})</summary>
      <ul aria-label="Background terminals">
        {terminals.map((terminal) => {
          const where = place(terminal.cwd, rootLabels);
          return (
            <li key={terminal.handle}>
              <code title={terminal.command}>{terminal.command}</code>
              {where ? <span>{where}</span> : null}
              <span>Running</span>
              <button
                className="button button--compact"
                type="button"
                disabled={stopping}
                aria-label={`Stop ${terminal.command}`}
                onClick={() => stop(terminal.handle)}
              >
                Stop
              </button>
            </li>
          );
        })}
      </ul>
      <button
        type="button"
        className="button button--compact"
        disabled={stopping}
        onClick={() => stop(null)}
      >
        Stop all
      </button>
      {error ? <p role="alert">{error}</p> : null}
    </details>
  );
}
