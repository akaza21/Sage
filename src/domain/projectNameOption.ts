/** Shared project identity labels for pickers and navigation. Never rename projects. */
type ProjectNameSource = { id: string; name: string; parentPath?: string };

export function projectNameOption(
  { id, name, parentPath: parent }: ProjectNameSource,
  projects: readonly ProjectNameSource[],
) {
  const duplicate = projects.some(
    (other) => other.id !== id && other.name === name,
  );
  const path = parent ?? "No source folder";
  return {
    id,
    label: name,
    subtitle: duplicate
      ? path.length > 64
        ? `...${path.slice(-61)}`
        : path
      : undefined,
    accessibleLabel: duplicate
      ? `${name}, ${path}${projects.some((other) => other.id !== id && other.name === name && other.parentPath === parent) ? `, ${id}` : ""}`
      : undefined,
  };
}
