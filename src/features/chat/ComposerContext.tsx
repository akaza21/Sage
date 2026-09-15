import { Folder, Monitor } from "lucide-react";

import type { AcpItem } from "../../domain/acp";
import { composerWorkspaceName } from "./composerChrome";

export function ComposerContext({
  project,
  usage,
}: {
  project: { name: string } | null | undefined;
  usage: Extract<AcpItem, { kind: "usage" }>["payload"] | null;
}) {
  const workspace = composerWorkspaceName(project);
  return (
    <p className="composer__context" aria-label="Composer context">
      <span className="composer__context-item">
        <Monitor size={12} strokeWidth={1.8} aria-hidden="true" />
        Local
      </span>
      {workspace ? (
        <span className="composer__context-item">
          <Folder size={12} strokeWidth={1.8} aria-hidden="true" />
          {workspace}
        </span>
      ) : null}
      {usage?.totalTokens !== null && usage?.totalTokens !== undefined ? (
        <span className="composer__context-item">
          Reported usage · {usage.totalTokens.toLocaleString()} tokens
        </span>
      ) : null}
    </p>
  );
}
