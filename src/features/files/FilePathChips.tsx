import { ChevronRight } from "../../ui/leanIcons";

import { FileKindIcon } from "./FileKindIcon";

export const MAX_BREADCRUMB_PATH_LENGTH = 4096;
export const MAX_BREADCRUMB_SEGMENTS = 6;

export function fileBreadcrumbSegments(path: string): string[] {
  if (path.length === 0 || path.length > MAX_BREADCRUMB_PATH_LENGTH) {
    return [];
  }
  const segments = path.split(/[\\/]/).filter((segment) => segment.length > 0);
  if (segments.length <= MAX_BREADCRUMB_SEGMENTS) {
    return segments;
  }
  return [segments[0]!, "…", ...segments.slice(-(MAX_BREADCRUMB_SEGMENTS - 2))];
}

export function FilePathChips({
  path,
  projectName,
}: {
  path: string;
  projectName?: string;
}) {
  const pathSegments = fileBreadcrumbSegments(path);
  if (pathSegments.length === 0) {
    return <span className="file-breadcrumb__empty">{path}</span>;
  }
  const segments = projectName
    ? pathSegments.length < MAX_BREADCRUMB_SEGMENTS
      ? [projectName, ...pathSegments]
      : [
          projectName,
          pathSegments[0]!,
          "…",
          ...pathSegments.slice(-(MAX_BREADCRUMB_SEGMENTS - 3)),
        ]
    : pathSegments;
  return (
    <nav className="file-breadcrumb" aria-label="File path">
      {segments.map((segment, index) => {
        const last = index === segments.length - 1;
        return (
          <span className="file-breadcrumb__item" key={`${index}:${segment}`}>
            {index > 0 ? <ChevronRight size={12} aria-hidden="true" /> : null}
            {last ? <FileKindIcon path={path} size={14} /> : null}
            <span>{segment}</span>
          </span>
        );
      })}
    </nav>
  );
}
