import { createElement, useState, type ReactNode } from "react";

// Tab strips draw favicons from the core bundle; the address helpers in
// `browserAddress.ts` stay in the browser surface's lazy chunk.

export function browserFaviconDataUrl(
  value: string | null | undefined,
): string | null {
  if (!value || value.length > 22 + Math.ceil((32 * 1024) / 3) * 4) return null;
  return /^data:image\/png;base64,iVBORw0KGgo[A-Za-z0-9+/]*={0,2}$/.test(value)
    ? value
    : null;
}

export function BrowserFavicon({
  value,
  fallback,
  className,
}: {
  value?: string | null;
  fallback: ReactNode;
  className?: string;
}): ReactNode {
  const [failed, setFailed] = useState(false);
  const src = browserFaviconDataUrl(value);
  return src && !failed
    ? createElement("img", {
        src,
        className,
        alt: "",
        "aria-hidden": true,
        onError: () => setFailed(true),
      })
    : fallback;
}
