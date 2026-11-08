import { getWorkbenchPorts } from "../../../ports/workbenchRuntime";
import { createGutterBaseCache } from "./gitGutterBase";
import {
  attachGitGutter,
  type GitGutter,
  type KnownText,
} from "./gitGutterExtension";
import { sharedGutterWorkerPool } from "./gutterWorkerClient";
import type { EditorView } from "@codemirror/view";
import { useWorkbenchStore, type WorkbenchState } from "../../../app/store";
import type { ResourceRef } from "../../../domain/workbench";

// The gutter's lazy entry: nothing here loads until a File in a
// repository opens with `editor.gitGutter` on.
let cache: ReturnType<typeof createGutterBaseCache> | null = null;

export function gutterBases() {
  cache ??= createGutterBaseCache((resource, knownOid) =>
    getWorkbenchPorts().repository.getFileGitBase(resource, knownOid),
  );
  return cache;
}

export function attachSharedGitGutter(
  view: EditorView,
  knownText: KnownText,
): GitGutter {
  return attachGitGutter(view, sharedGutterWorkerPool(), knownText);
}

/** Subscribe and coalesce base reloads only after the gutter has loaded. */
export function watchGitGutterBase(handle: GitGutter, resource: ResourceRef) {
  let current = true;
  let loading = false;
  let again = false;
  const load = () => {
    if (loading) {
      again = true;
      return;
    }
    loading = true;
    again = false;
    void gutterBases()
      .load(resource)
      .then(
        (base) => current && handle.setBase(base),
        () => current && handle.setBase(null),
      )
      .finally(() => {
        loading = false;
        if (current && again) load();
      });
  };
  load();
  const rootOf = (state: WorkbenchState) =>
    state.repositories.roots.find((root) => root.rootId === resource.rootId);
  // Store subscription, not a selector: refreshes re-render nothing here.
  const unsubscribe = useWorkbenchStore.subscribe((state, previous) => {
    const refreshed =
      previous.repositoryRefreshing && !state.repositoryRefreshing;
    if (refreshed || rootOf(state) !== rootOf(previous)) load();
  });
  return () => {
    current = false;
    unsubscribe();
  };
}

export type { GitGutter };
