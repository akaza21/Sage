// Go Back and Go Forward through the places the Editor has been (ED-2b), as
// in VS Code. A place is left behind when another File becomes active or the
// caret moves more than FAR_LINES lines at once (Go to Line, an Outline or
// Search result, a click far away). Moving a few lines updates the current
// place instead.

import type { ResourceRef } from "../domain/workbench";
import { sameResource } from "../domain/resourceIdentity";
import { useEditorCaretStore } from "./editorCaretStore";
import type { FileNavigationTarget } from "./fileNavigationSlice";
import { useWorkbenchStore } from "./store";

export interface EditorPlace {
  projectId: string;
  resource: ResourceRef;
  line: number;
  column: number;
}

interface Trip {
  place: EditorPlace;
  /** Until `openFileLocation` returns, the File may still be opening. */
  opening: boolean;
  /** A caret has moved in the File since it opened: the target landed. */
  arrived: boolean;
}

export const MAX_EDITOR_PLACES = 50;
const FAR_LINES = 10;

let back: EditorPlace[] = [];
let forward: EditorPlace[] = [];
let current: EditorPlace | null = null;
let historyProject: string | null = null;
// The editor puts the caret somewhere when a File opens, then a jump that
// opened it moves it. Until the caret moves in it, a File's place is only
// where it opened, and a jump there replaces it instead of leaving it behind.
let provisional = false;
let trip: Trip | null = null;
let stopWatching: (() => void) | null = null;

/** Starts noting places. Every later call does nothing. */
export function watchEditorHistory() {
  stopWatching ??= useEditorCaretStore.subscribe((caretState, previous) => {
    const state = useWorkbenchStore.getState();
    const projectId = state.project?.id;
    const tab = state.tabs.find((item) => item.id === state.activeTabId);
    const editor = state.activeEditor;
    if (
      !caretState.caret ||
      !caretState.editable ||
      !projectId ||
      tab?.kind !== "file" ||
      !tab.resource ||
      editor?.phase !== "ready" ||
      !sameResource(editor.resource, tab.resource)
    ) {
      return;
    }
    notePlace(
      {
        projectId,
        resource: tab.resource,
        line: caretState.caret.line,
        column: caretState.caret.column,
      },
      // A remounted editor (back from a Diff or Settings tab) starts at the
      // top again, which is not a move the user made.
      previous.caret === null,
    );
  });
}

/** For tests: forgets every place and stops noting them. */
export function resetEditorHistory() {
  stopWatching?.();
  stopWatching = null;
  back = [];
  forward = [];
  current = null;
  historyProject = null;
  provisional = false;
  trip = null;
}

const isTarget = (target: FileNavigationTarget | null, place: EditorPlace) =>
  !!target &&
  sameResource(target.resource, place.resource) &&
  target.lineNumber === place.line &&
  target.columnNumber === place.column;

export function notePlace(place: EditorPlace, remounted = false) {
  if (historyProject !== place.projectId) {
    back = [];
    forward = [];
    current = null;
    trip = null;
    historyProject = place.projectId;
  }
  if (trip) {
    if (sameResource(trip.place.resource, place.resource)) {
      // The File arriving: its editor mounting, then the target applied
      // (or, if something kept it from applying, the user's first move).
      current = place;
      if (!remounted) {
        trip.arrived = true;
        if (!trip.opening) trip = null;
      }
      return;
    }
    // The File being left can still report while it saves on the way out.
    if (trip.opening) return;
    trip = null;
  }
  if (!current) {
    // After a place that could not be opened: a new start.
    forward = [];
    current = place;
    provisional = true;
    return;
  }
  if (!sameResource(current.resource, place.resource)) {
    leave(place);
    provisional = true;
    return;
  }
  if (remounted) return;
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

const fileTabOf = (resource: ResourceRef) =>
  useWorkbenchStore
    .getState()
    .tabs.find(
      (tab) => tab.kind === "file" && sameResource(tab.resource, resource),
    );

async function travel(backwards: boolean) {
  const from = backwards ? back : forward;
  const to = backwards ? forward : back;
  const target = from.at(-1);
  const state = useWorkbenchStore.getState();
  if (!target || trip?.opening || target.projectId !== state.project?.id) {
    return;
  }
  from.pop();
  const left = current;
  if (left) to.push(left);
  current = target;
  provisional = false;
  const thisTrip: Trip = { place: target, opening: true, arrived: false };
  trip = thisTrip;
  const hadTab = !!fileTabOf(target.resource);
  await state.openFileLocation({
    projectId: target.projectId,
    transitionEpoch: state.nativeTransitionEpoch,
    resource: target.resource,
    lineNumber: target.line,
    columnNumber: target.column,
    selection: { kind: "caret" },
  });
  if (trip !== thisTrip) return;
  thisTrip.opening = false;
  const after = useWorkbenchStore.getState();
  const editor = after.activeEditor;
  const active = after.tabs.find((tab) => tab.id === after.activeTabId);
  const busy =
    after.projectTransitioning ||
    after.fileRenameInFlight ||
    after.resourceTrashInFlight ||
    after.fileCreateInFlight ||
    after.folderCreateInFlight;
  // The target is set even for a File that failed to load.
  if (
    editor?.phase === "error" &&
    sameResource(editor.resource, target.resource)
  ) {
    // Its File is gone (renamed, deleted) or unreadable: the place is
    // dropped, so the next Go Back moves on, with the tab opened for it.
    trip = null;
    current = null;
    const tab = fileTabOf(target.resource);
    if (!hadTab && tab) after.closeTab(tab.id);
    return;
  }
  const opened =
    active?.kind === "file" &&
    sameResource(active.resource, target.resource) &&
    // While the store is busy it refuses the open, and an already active
    // tab proves nothing: then only the target, or its caret, shows it.
    (thisTrip.arrived ||
      isTarget(after.fileNavigationTarget, target) ||
      !hadTab ||
      !busy);
  if (opened) {
    // Its caret may still be on the way; the trip ends when it lands.
    if (thisTrip.arrived) trip = null;
    return;
  }
  // Nothing opened. Busy for a moment (a save, a rename, a Project
  // opening): keep the place for the next try. Otherwise skip it.
  trip = null;
  current = left;
  if (left) to.pop();
  if (busy) from.push(target);
}
