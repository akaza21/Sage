// Go Back and Go Forward through the places the Editor has been (ED-2b), as
// in VS Code. A place is left behind when another File becomes active or the
// caret moves more than FAR_LINES lines at once (Go to Line, an Outline or
// Search result, a click far away). Moving a few lines updates the current
// place instead.

import type { ResourceRef } from "../domain/workbench";
import { sameResource } from "../domain/resourceIdentity";
import { useEditorCaretStore } from "./editorCaretStore";
import { useWorkbenchStore } from "./store";

export interface EditorPlace {
  projectId: string;
  resource: ResourceRef;
  line: number;
  column: number;
}

export const MAX_EDITOR_PLACES = 50;
const FAR_LINES = 10;

let back: EditorPlace[] = [];
let forward: EditorPlace[] = [];
let current: EditorPlace | null = null;
// The editor puts the caret somewhere when a File opens, then a jump that
// opened it moves it. Until the caret moves in it, a File's place is only
// where it opened, and a jump there replaces it instead of leaving it behind.
let provisional = false;
let travelling: EditorPlace | null = null;
let stopWatching: (() => void) | null = null;

/** Starts noting places. Every later call does nothing. */
export function watchEditorHistory() {
  if (stopWatching) return;
  const stopCaret = useEditorCaretStore.subscribe(noteCaret);
  // A Go Back/Forward is over once its target has been applied.
  const stopTarget = useWorkbenchStore.subscribe((state, previous) => {
    if (
      travelling &&
      previous.fileNavigationTarget &&
      !state.fileNavigationTarget
    ) {
      travelling = null;
    }
  });
  stopWatching = () => {
    stopCaret();
    stopTarget();
  };
}

/** For tests: forgets every place and stops noting them. */
export function resetEditorHistory() {
  stopWatching?.();
  stopWatching = null;
  back = [];
  forward = [];
  current = null;
  provisional = false;
  travelling = null;
}

function noteCaret() {
  const caret = useEditorCaretStore.getState().caret;
  const state = useWorkbenchStore.getState();
  const projectId = state.project?.id;
  const tab = state.tabs.find((item) => item.id === state.activeTabId);
  const editor = state.activeEditor;
  if (
    !caret ||
    !projectId ||
    tab?.kind !== "file" ||
    !tab.resource ||
    editor?.phase !== "ready" ||
    !sameResource(editor.resource, tab.resource)
  ) {
    return;
  }
  notePlace({
    projectId,
    resource: tab.resource,
    line: caret.line,
    column: caret.column,
  });
}

export function notePlace(place: EditorPlace) {
  if (current && current.projectId !== place.projectId) {
    back = [];
    forward = [];
    current = null;
  }
  if (travelling) {
    if (sameResource(travelling.resource, place.resource)) {
      current = place;
      return;
    }
    // Something else took the editor before the target was applied.
    travelling = null;
  }
  if (!current) {
    current = place;
    provisional = true;
    return;
  }
  if (!sameResource(current.resource, place.resource)) {
    leave(place);
    provisional = true;
    return;
  }
  if (!provisional && Math.abs(place.line - current.line) > FAR_LINES) {
    leave(place);
  } else {
    current = place;
  }
  provisional = false;
}

function leave(place: EditorPlace) {
  if (current) {
    back.push(current);
    if (back.length > MAX_EDITOR_PLACES) back.shift();
  }
  forward = [];
  current = place;
}

function projectId() {
  return useWorkbenchStore.getState().project?.id ?? null;
}

export function canGoBack() {
  return !!projectId() && back.at(-1)?.projectId === projectId();
}

export function canGoForward() {
  return !!projectId() && forward.at(-1)?.projectId === projectId();
}

export const goBack = () => travel(true);
export const goForward = () => travel(false);

async function travel(backwards: boolean) {
  const from = backwards ? back : forward;
  const to = backwards ? forward : back;
  const target = from.at(-1);
  const state = useWorkbenchStore.getState();
  if (!target || travelling || target.projectId !== state.project?.id) return;
  from.pop();
  const left = current;
  if (left) to.push(left);
  current = target;
  provisional = false;
  travelling = target;
  await state.openFileLocation({
    projectId: target.projectId,
    transitionEpoch: state.nativeTransitionEpoch,
    resource: target.resource,
    lineNumber: target.line,
    columnNumber: target.column,
    selection: { kind: "caret" },
  });
  const after = useWorkbenchStore.getState();
  if (travelling !== target) return;
  const active = after.tabs.find((tab) => tab.id === after.activeTabId);
  if (
    active?.kind === "file" &&
    sameResource(active.resource, target.resource)
  ) {
    // Applied already, or never asked for: either way nothing is on its way.
    if (!after.fileNavigationTarget) travelling = null;
  } else {
    // It could not be opened (gone, or the Project changed): skip it, so the
    // next Go Back moves on instead of failing on it again.
    travelling = null;
    if (left) to.pop();
    current = left;
  }
}
