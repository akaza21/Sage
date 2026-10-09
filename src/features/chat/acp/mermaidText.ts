export function mermaidLines(source: string): string[] {
  return source
    .split("\n")
    .map((line) => line.replace(/\s*%%.*$/u, "").trim())
    .filter(Boolean);
}

export function clampMermaidLabel(value: string): string {
  return value.replace(/\s+/gu, " ").trim().slice(0, 64);
}
