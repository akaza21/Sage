import type * as SlashChunk from "./ComposerSlashController";
import { lazyParts } from "./lazyParts";

/** The composer's `/` controller facade. */
export const SlashPart = lazyParts<
  Pick<typeof SlashChunk, "ComposerSlashController">
>(() => import("./ComposerSlashController"));

/**
 * A harness's composer chrome shares the composer options chunk; it renders
 * nothing until a session reports some.
 */
export { ComposerOptionsPart as ChromePart } from "./lazyParts";
