/** Commands the File editor runs on request, as from the command palette. */
export type FileSurfaceCommand =
  "find" | "replace" | "goToLine" | "goToSymbol" | "save";

export interface FileSurfaceCommands {
  /** Whether `command` would do something now. */
  available: (command: FileSurfaceCommand) => boolean;
  run: (command: FileSurfaceCommand) => void;
}

// The File surfaces showing, latest last; the latest one runs a command.
const surfaces: FileSurfaceCommands[] = [];

/** Registers the File editor that is showing; returns its removal. */
export function registerFileSurfaceCommands(
  commands: FileSurfaceCommands,
): () => void {
  surfaces.push(commands);
  return () => {
    const at = surfaces.lastIndexOf(commands);
    if (at >= 0) surfaces.splice(at, 1);
  };
}

export function fileSurfaceCommandAvailable(
  command: FileSurfaceCommand,
): boolean {
  return surfaces.at(-1)?.available(command) ?? false;
}

/** Runs `command` in the File editor showing, if it can. */
export function runFileSurfaceCommand(command: FileSurfaceCommand): boolean {
  const surface = surfaces.at(-1);
  if (!surface?.available(command)) return false;
  surface.run(command);
  return true;
}
