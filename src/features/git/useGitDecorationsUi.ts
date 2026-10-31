import { useEffect, useState } from "react";

import type * as GitDecorationsUi from "./gitDecorationsUi";

export type GitDecorationsUiModule = typeof GitDecorationsUi;

let loaded: GitDecorationsUiModule | undefined;

/**
 * The Git decoration chunk (EP-13), fetched the first time it is `wanted`:
 * `git.decorations` on and a repository root to mark. Until then, and while
 * it is not wanted, nothing renders marks.
 */
export function useGitDecorationsUi(
  wanted: boolean,
): GitDecorationsUiModule | undefined {
  const [module, setModule] = useState(loaded);
  useEffect(() => {
    if (!wanted || module) return;
    let live = true;
    import("./gitDecorationsUi").then(
      (next) => {
        loaded = next;
        if (live) setModule(next);
      },
      (error: unknown) => console.warn("Git decorations could not load", error),
    );
    return () => {
      live = false;
    };
  }, [module, wanted]);
  return wanted ? module : undefined;
}
