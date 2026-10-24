import {
  lazy,
  Suspense,
  useState,
  type ComponentProps,
  type ComponentType,
  type ReactNode,
} from "react";
import type * as ChatNavigationChunk from "./ChatNavigation";
import type * as PermissionPromptChunk from "./acp/PermissionPrompt";
import type * as TranscriptChunk from "./TranscriptInteractions";
import type * as ChatSearchChunk from "../chatSearch/ChatSearchControl";
import type * as ForkChunk from "./ForkChat";
import type * as HandoffChunk from "./ChatHandoffDialog";
import type * as QueuedChunk from "./ComposerQueuedPrompts";

// eslint-disable-next-line @typescript-eslint/no-explicit-any -- keyed re-export of components
type AnyComponent = ComponentType<any>;
type Components = Record<string, AnyComponent>;

/**
 * One tiny core loader per lazy feature chunk. `<Part part="Name" … />`
 * renders that named component of the chunk, and `fallback` (nothing by
 * default) until it loads. Once the chunk has loaded, every part renders
 * synchronously: a part first used later never suspends, so it appears in
 * the same frame as the state that asked for it.
 */
export function lazyParts<Chunk extends Components>(
  load: () => Promise<Chunk>,
) {
  const parts: Partial<Record<keyof Chunk, AnyComponent>> = {};
  let loaded: Chunk | null = null;
  const loadOnce = () =>
    load().then((chunk) => {
      loaded = chunk;
      return chunk;
    });
  return function Part<K extends keyof Chunk & string>({
    fallback = null,
    ...props
  }: { part: K; fallback?: ReactNode } & ComponentProps<Chunk[K]>) {
    const part: K = props.part;
    // Decided once per mount, so a mounted part never swaps its subtree.
    const [Ready] = useState<AnyComponent | undefined>(() => loaded?.[part]);
    if (Ready) return <Ready {...props} />;
    const Loaded: AnyComponent = (parts[part] ??= lazy(() =>
      loadOnce().then(
        (chunk) => ({ default: chunk[part]! }),
        (error: unknown) => {
          // A failed import is not cached: the next mount (reopening the
          // Chat) imports again instead of rethrowing the old failure.
          delete parts[part];
          throw error;
        },
      ),
    ));
    return (
      <Suspense fallback={fallback}>
        <Loaded {...props} />
      </Suspense>
    );
  };
}

/** Transcript selection, annotations and side chats (TranscriptInteractions.tsx). */
export const TranscriptPart = lazyParts<typeof TranscriptChunk>(
  () => import("./TranscriptInteractions"),
);
/** Turn navigator and find in chat (ChatNavigation.tsx). */
export const NavigationPart = lazyParts<typeof ChatNavigationChunk>(
  () => import("./ChatNavigation"),
);
/** The composer-slot approval card (PermissionPrompt.tsx). */
export const ApprovalPart = lazyParts<typeof PermissionPromptChunk>(
  () => import("./acp/PermissionPrompt"),
);
/** The Chat search dialog (chatSearch/ChatSearchControl.tsx). */
export const ChatSearchPart = lazyParts<typeof ChatSearchChunk>(
  () => import("../chatSearch/ChatSearchControl"),
);
/** Answer "…" menu, fork dialog and "Continued from chat" (ForkChat.tsx). */
export const ForkPart = lazyParts<typeof ForkChunk>(() => import("./ForkChat"));
/** The "Hand off this Chat" dialog (ChatHandoffDialog.tsx). */
export const HandoffPart = lazyParts<typeof HandoffChunk>(
  () => import("./ChatHandoffDialog"),
);
/** The queued-message rows above the composer (ComposerQueuedPrompts.tsx). */
export const QueuedPart = lazyParts<typeof QueuedChunk>(
  () => import("./ComposerQueuedPrompts"),
);
/** Queued rows, loaded with the first queued message (nothing before). */
export function QueuedRows(
  props: ComponentProps<typeof QueuedChunk.ComposerQueuedPrompts>,
) {
  return props.prompts?.length ? (
    <QueuedPart part="ComposerQueuedPrompts" {...props} />
  ) : null;
}
