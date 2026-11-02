import { getWorkbenchPorts } from "../../../ports/workbenchRuntime";
import { createGutterBaseCache } from "./gitGutterBase";
import {
  attachGitGutter,
  type GitGutter,
  type KnownText,
} from "./gitGutterExtension";
import { sharedGutterWorkerPool } from "./gutterWorkerClient";
import type { EditorView } from "@codemirror/view";

// The gutter's lazy entry (EP-14): nothing here loads until a File in a
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

export type { GitGutter };
