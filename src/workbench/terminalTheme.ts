/** The viewport's CSS roles resolve aliases and color-mix before xterm parses them. */
export function terminalTheme(element: HTMLElement) {
  const colors = getComputedStyle(element);
  return {
    background: colors.backgroundColor,
    foreground: colors.color,
    cursor: colors.caretColor,
    cursorAccent: colors.backgroundColor,
    selectionBackground: colors.accentColor,
  };
}
