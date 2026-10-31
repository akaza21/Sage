/** Commands the File editor runs on request, as from the command palette. */
export type FileSurfaceCommand =
  "find" | "replace" | "goToLine" | "goToSymbol" | "save";

type Runner = (command: FileSurfaceCommand) => void;

// The File surfaces showing, latest last; the latest one runs a command.
const runners: Runner[] = [];

/** Registers the File editor that is showing; returns its removal. */
export function registerFileSurfaceCommands(runner: Runner): () => void {
  runners.push(runner);
  return () => {
    const at = runners.lastIndexOf(runner);
    if (at >= 0) runners.splice(at, 1);
  };
}

export function hasFileSurface(): boolean {
  return runners.length > 0;
}

/** Runs `command` in the File editor showing, if there is one. */
export function runFileSurfaceCommand(command: FileSurfaceCommand): boolean {
  const runner = runners.at(-1);
  runner?.(command);
  return runner !== undefined;
}
