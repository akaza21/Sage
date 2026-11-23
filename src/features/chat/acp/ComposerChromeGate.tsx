import type { ComponentProps } from "react";
import { lazyParts } from "../lazyParts";
import type * as ChromeChunk from "./ComposerChrome";

const ChromePart = lazyParts<typeof ChromeChunk>(
  () => import("./ComposerChrome"),
);

/**
 * Fetches the composer chrome only while the session reports some; the
 * native view carries `chrome` only then.
 */
export function ComposerChromeGate(
  props: ComponentProps<typeof ChromeChunk.ComposerChrome>,
) {
  const surface = props.chatAuth?.page?.sessionSurface;
  return surface && typeof surface === "object" && "chrome" in surface ? (
    <ChromePart part="ComposerChrome" {...props} />
  ) : null;
}
