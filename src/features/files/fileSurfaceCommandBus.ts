export type FileSurfaceCommand =
  "find" | "replace" | "goToLine" | "goToSymbol" | "save";

export interface FileSurfaceCommands {
  available: (command: FileSurfaceCommand) => boolean;
  run: (command: FileSurfaceCommand) => void;
}

const surfaces: FileSurfaceCommands[] = [];

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

export function runFileSurfaceCommand(command: FileSurfaceCommand): boolean {
  const surface = surfaces.at(-1);
  if (!surface?.available(command)) return false;
  surface.run(command);
  return true;
}
