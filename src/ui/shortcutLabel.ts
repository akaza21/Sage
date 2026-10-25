export function shortcutLabel(key: string, usesMeta: boolean): string {
  const upper = key.toUpperCase();
  return usesMeta === true ? `\u2318${upper}` : `Ctrl+${upper}`;
}

/**
 * True on Apple platforms, where Command is the shortcut modifier. WKWebView
 * reports "MacIntel" on Intel and Apple silicon, and iPadOS in desktop mode
 * does too; the user agent covers an empty platform string.
 */
export function usesMetaModifier(): boolean {
  if (typeof navigator === "undefined") return false;
  return /mac|iphone|ipad/i.test(navigator.platform || navigator.userAgent);
}
