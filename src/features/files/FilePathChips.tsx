import { ChevronRight } from "lucide-react";

import { fileBreadcrumbSegments } from "./editorPathBreadcrumb";
import { FileKindIcon } from "./FileKindIcon";

export function FilePathChips({ path }: { path: string }) {
  const segments = fileBreadcrumbSegments(path);
  if (segments.length === 0) {
    return <span className="file-breadcrumb__empty">{path}</span>;
  }
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
