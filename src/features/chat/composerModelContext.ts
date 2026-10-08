export function formatContextWindow(tokens: number | undefined): string | null {
  if (!tokens || !Number.isSafeInteger(tokens) || tokens <= 0) return null;
  if (tokens >= 1_000_000 && tokens % 1_000_000 === 0)
    return `${tokens / 1_000_000}M`;
  if (tokens >= 1_000 && tokens % 1_000 === 0) return `${tokens / 1_000}K`;
  return tokens.toLocaleString();
}
