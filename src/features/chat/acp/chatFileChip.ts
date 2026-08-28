export const CHAT_FILE_CHIP_NAME_CAP = 48;
export const CHAT_FILE_CHIP_COUNT_CAP = 999;

export function chatFileChipBasename(relativePath: string): string | null {
  const trimmed = relativePath.trim();
  if (
    !trimmed ||
    trimmed.split("").some((character) => {
      const code = character.charCodeAt(0);
      return code < 32 || code === 127;
    })
  ) {
    return null;
  }
  const parts = trimmed.split(/[\\/]/u).filter(Boolean);
  const base = parts[parts.length - 1];
  if (!base) return null;
  if (base.length <= CHAT_FILE_CHIP_NAME_CAP) return base;
  return `${base.slice(0, CHAT_FILE_CHIP_NAME_CAP - 1)}…`;
}

export function chatFileChipCount(
  sign: "+" | "-",
  value: number | null,
): string | null {
  if (value === null) return null;
  if (value > CHAT_FILE_CHIP_COUNT_CAP) {
    return `${sign}${CHAT_FILE_CHIP_COUNT_CAP}+`;
  }
  return `${sign}${value}`;
}
