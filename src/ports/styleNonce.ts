/**
 * The page's CSP nonce for `<style>` elements, or "" when it has none.
 *
 * Tauri marks every `<style>` in index.html at build time and, on each load,
 * swaps in a fresh nonce that it also adds to style-src. A nonce in style-src
 * makes the browser ignore 'unsafe-inline', so a `<style>` a library injects
 * at runtime is refused unless it carries the same nonce. Browsers blank the
 * attribute once the policy applies, so the value is read from the property.
 */
export function styleNonce(document: Document = globalThis.document): string {
  return (
    document.head?.querySelector<HTMLStyleElement>("style[nonce]")?.nonce ?? ""
  );
}

/**
 * Runs `create` so that each `<style>` element `document` creates meanwhile
 * carries the style nonce. For libraries with no nonce option that create
 * their style elements synchronously. Parsed markup is unaffected, so the
 * nonce still refuses an injected `<style>`.
 */
export function withStyleNonce<T>(document: Document, create: () => T): T {
  const nonce = styleNonce(document);
  if (!nonce || Object.hasOwn(document, "createElement")) return create();
  const createElement = document.createElement;
  Object.defineProperty(document, "createElement", {
    configurable: true,
    value(this: Document, ...args: Parameters<Document["createElement"]>) {
      const element = createElement.apply(this, args);
      if (element instanceof HTMLStyleElement) element.nonce = nonce;
      return element;
    },
  });
  try {
    return create();
  } finally {
    delete (document as { createElement?: unknown }).createElement;
  }
}
