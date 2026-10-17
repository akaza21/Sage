export function shortcutLabel(key: string, usesMeta: boolean): string {
  const upper = key.toUpperCase();
  return usesMeta === true ? `\u2318${upper}` : `Ctrl+${upper}`;
}

export function usesMetaModifier(): boolean {
  return /mac/i.test(navigator.platform);
}
