import type { ComponentProps } from "react";
import type * as ProjectScopeChunk from "./ProjectScopeChip";
import { lazyParts } from "./lazyParts";

const ScopePart = lazyParts<typeof ProjectScopeChunk>(
  () => import("./ProjectScopeChip"),
);

export function ProjectScopeChip(
  props: ComponentProps<typeof ProjectScopeChunk.ProjectScopeChip>,
) {
  return (
    <ScopePart
      part="ProjectScopeChip"
      {...props}
      fallback={props.projectName}
    />
  );
}
