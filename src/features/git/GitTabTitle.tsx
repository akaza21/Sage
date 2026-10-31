import { useWorkbenchStore } from "../../app/store";
import type { ResourceRef } from "../../domain/workbench";
import { GitDecorationBadge } from "./GitDecorationBadge";
import { resourceGitDecoration } from "./gitDecorations";
import "./git-decorations.css";

/** A File tab's title with its Git tone and letter (EP-13). */
export default function GitTabTitle({
  title,
  resource,
}: {
  title: string;
  resource: ResourceRef;
}) {
  const git = useWorkbenchStore((state) =>
    resourceGitDecoration(state.repositories, resource),
  );
  if (!git) return <span>{title}</span>;
  return (
    <>
      <span className={`tab__title--git git-tone--${git.tone}`}>{title}</span>
      <span className="sr-only">, {git.label}</span>
      <GitDecorationBadge decoration={git} />
    </>
  );
}
