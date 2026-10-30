import { isRecord } from "./guards";
import { utf8ByteLength } from "./text";

// The two browser context kinds, apart from `attachedContext.ts` so that
// module stays within the line budget; it re-exports everything here.

export const MAX_BROWSER_PAGE_TEXT_BYTES = 4 * 1024;
// Contract browser-element-context-v1 §2.1: at most 8 contexts per prompt
// and the 4 KiB attached-context note.
const MAX_ELEMENT_ORDINAL = 8;
const MAX_ELEMENT_COMMENT_BYTES = 4 * 1024;

export function unsafeDisplayCharacter(character: string): boolean {
  const code = character.codePointAt(0) ?? 0;
  return (
    code < 32 ||
    (code >= 0x7f && code <= 0x9f) ||
    code === 0x061c ||
    code === 0x200e ||
    code === 0x200f ||
    code === 0x2028 ||
    code === 0x2029 ||
    (code >= 0x202a && code <= 0x202e) ||
    (code >= 0x2066 && code <= 0x2069)
  );
}

export interface BrowserPageContext {
  kind: "browserPage";
  tabId: string;
  url: string;
  title: string;
  text: string;
}

/** Contract browser-element-context-v1; Rust built and redacted it. */
export interface BrowserElementContext {
  kind: "browserElement";
  target: "element" | "area";
  ordinal: number;
  tabId: string;
  url: string;
  title: string;
  selector?: string;
  role?: string;
  name?: string;
  text: string;
  box: { x: number; y: number; width: number; height: number };
  comment: string;
}

/** Within `limit` bytes, with no control or bidi character outside `allowed`. */
function boundedText(
  value: unknown,
  limit: number,
  allowed = "",
): value is string {
  return (
    typeof value === "string" &&
    utf8ByteLength(value) <= limit &&
    ![...value].some(
      (character) =>
        !allowed.includes(character) && unsafeDisplayCharacter(character),
    )
  );
}

export function parseBrowserPageContext(
  value: unknown,
): BrowserPageContext | null {
  if (!isRecord(value) || value.kind !== "browserPage") return null;
  const { tabId, url, title, text } = value;
  if (
    typeof tabId !== "string" ||
    !/^browser-[1-9]\d*$/.test(tabId) ||
    typeof url !== "string" ||
    utf8ByteLength(url) > 2048 ||
    !boundedText(title, 256) ||
    !boundedText(text, MAX_BROWSER_PAGE_TEXT_BYTES, "\n")
  )
    return null;
  try {
    const parsed = new URL(url);
    if (
      !["http:", "https:"].includes(parsed.protocol) ||
      parsed.username ||
      parsed.password
    )
      return null;
  } catch {
    return null;
  }
  return { kind: "browserPage", tabId, url, title, text };
}

const ELEMENT_KEYS =
  "box,comment,kind,name,ordinal,role,selector,tabId,target,text,title,url";
const AREA_KEYS = "box,comment,kind,ordinal,tabId,target,text,title,url";

const between = (value: unknown, low: number, high: number) =>
  Number.isInteger(value) && Number(value) >= low && Number(value) <= high;

/** The contract's mirrored limits; Rust checks the redaction fixpoints again. */
export function parseBrowserElementContext(
  value: unknown,
): BrowserElementContext | null {
  if (!isRecord(value) || value.kind !== "browserElement") return null;
  const { box, url, tabId, title } = value;
  const valid =
    (value.target === "area"
      ? Object.keys(value).sort().join() === AREA_KEYS
      : value.target === "element" &&
        Object.keys(value).sort().join() === ELEMENT_KEYS &&
        boundedText(value.selector, 512) &&
        value.selector !== "" &&
        /^[A-Za-z][A-Za-z0-9-]{0,63}$/.test(String(value.role)) &&
        boundedText(value.name, 256)) &&
    between(value.ordinal, 1, MAX_ELEMENT_ORDINAL) &&
    parseBrowserPageContext({
      kind: "browserPage",
      tabId,
      url,
      title,
      text: "",
    }) &&
    boundedText(value.text, 1024, "\n") &&
    boundedText(value.comment, MAX_ELEMENT_COMMENT_BYTES, "\n\t") &&
    isRecord(box) &&
    Object.keys(box).sort().join() === "height,width,x,y" &&
    between(box.x, 0, 1e7) &&
    between(box.y, 0, 1e7) &&
    between(box.width, 1, 1e5) &&
    between(box.height, 1, 1e5) &&
    utf8ByteLength(JSON.stringify(value)) <= 8192;
  return valid ? (value as unknown as BrowserElementContext) : null;
}
