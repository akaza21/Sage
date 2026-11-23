import type { ChatComposerRegionProps } from "./chatComposerRegionProps";
import type * as SlashChunk from "./ComposerSlashController";
import { NoticePart, lazyParts } from "./lazyParts";

/** The `/` controller facade: composer commands and a chat's first send. */
export const SlashPart = lazyParts<
  Pick<typeof SlashChunk, "ComposerSlashController" | "FirstChatSend">
>(() => import("./ComposerSlashController"));

/**
 * A harness's composer chrome. Its gate rides in the session strip's chunk,
 * which the transcript already loads for any chat with a session surface,
 * and fetches the chrome itself only while the session reports some.
 */
export const harnessChrome = (props: ChatComposerRegionProps) =>
  props.chatAuth?.page?.sessionSurface ? (
    <NoticePart part="ComposerChromeGate" {...props} />
  ) : null;
