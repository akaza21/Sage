import { Folder, Monitor } from "lucide-react";

import { composerWorkspaceName } from "./composerChrome";

export function ComposerContext({
  project,
}: {
  project: { name: string } | null | undefined;
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
    </p>
  );
}
