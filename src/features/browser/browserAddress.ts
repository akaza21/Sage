import type { BrowserNavigation } from "../../ports/nativeBrowserPort";
import { browserFaviconDataUrl } from "./browserFavicon";

const MAX_ADDRESS_LENGTH = 2048;

function containsControl(text: string): boolean {
  for (const character of text) {
    const codePoint = character.codePointAt(0);
    if (codePoint !== undefined && (codePoint < 32 || codePoint === 127)) {
      return true;
    }
  }
  return false;
}

export function resolveBrowserAddress(input: string): string {
  if (!input || input.length > MAX_ADDRESS_LENGTH || containsControl(input)) {
    throw new Error("Invalid browser address");
  }
  const text = input.trim();
  const explicitScheme = /^[a-z][a-z0-9+.-]*:/iu.test(text);
  if (explicitScheme && !/^https?:\/\//iu.test(text)) {
    throw new Error("Browser addresses must use HTTP or HTTPS");
  }
  const looksLikeHost =
    !/\s/u.test(text) &&
    (/^https?:\/\//iu.test(text) ||
      text.includes(".") ||
      /^localhost(?::\d+)?(?:\/|$)/iu.test(text));
  if (!looksLikeHost) {
    return `https://www.google.com/search?${new URLSearchParams({ q: text })}`;
  }
  const candidate = /^https?:\/\//iu.test(text)
    ? text
    : /^localhost(?::\d+)?(?:\/|$)/iu.test(text)
      ? `http://${text}`
      : `https://${text}`;
  const url = new URL(candidate);
  if (
    !["http:", "https:"].includes(url.protocol) ||
    !url.hostname ||
    url.username ||
    url.password
  ) {
    throw new Error("Browser address is not admitted");
  }
  return url.href;
}

export interface BrowserPageIdentity {
  origin: string;
  transport: "secure" | "not-secure";
  url: string;
}

export function browserPageIdentity(url: string): BrowserPageIdentity | null {
  try {
    if (resolveBrowserAddress(url) !== url) return null;
    const parsed = new URL(url);
    return {
      origin: parsed.origin,
      transport: parsed.protocol === "https:" ? "secure" : "not-secure",
      url,
    };
  } catch {
    return null;
  }
}

export function suggestedSitesWithTitles(
  sites: readonly string[],
  statuses: readonly PromiseSettledResult<BrowserNavigation>[],
): { url: string; title?: string | null; favicon?: string | null }[] {
  return sites.slice(0, 4).map((url) => {
    const match = statuses.find(
      (result) =>
        result.status === "fulfilled" &&
        !result.value.loading &&
        !result.value.error &&
        result.value.url === url,
    );
    return {
      url,
      title: match?.status === "fulfilled" ? match.value.title : null,
      favicon:
        match?.status === "fulfilled"
          ? browserFaviconDataUrl(match.value.favicon)
          : null,
    };
  });
}

export type BrowserSuggestedSites = ReturnType<typeof suggestedSitesWithTitles>;
