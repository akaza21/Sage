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
import type * as EditSummaryChunk from "./acp/TurnEditSummary";
import type * as HarnessSignInChunk from "../harness/HarnessSignInStatus";

// eslint-disable-next-line @typescript-eslint/no-explicit-any -- keyed re-export of components
type AnyComponent = ComponentType<any>;
type Components = Record<string, AnyComponent>;

/** One loader per chunk; all named parts render synchronously once loaded. */
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
    // Fix the subtree per mount.
    const [Ready] = useState<AnyComponent | undefined>(() => loaded?.[part]);
    if (Ready) return <Ready {...props} />;
    const Loaded: AnyComponent = (parts[part] ??= lazy(() =>
      loadOnce().then(
        (chunk) => ({ default: chunk[part]! }),
        (error: unknown) => {
          // Retry failed imports on the next mount.
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

export const TranscriptPart = lazyParts<
  Omit<typeof TranscriptChunk, "openSideChat" | "readTranscriptQuote">
>(() => import("./TranscriptInteractions"));
export const NavigationPart = lazyParts<typeof ChatNavigationChunk>(
  () => import("./ChatNavigation"),
);
export const ApprovalPart = lazyParts<typeof PermissionPromptChunk>(
  () => import("./acp/PermissionPrompt"),
);
export const ChatSearchPart = lazyParts<typeof ChatSearchChunk>(
  () => import("../chatSearch/ChatSearchControl"),
);
export const ForkPart = lazyParts<typeof ForkChunk>(() => import("./ForkChat"));
export const HandoffPart = lazyParts<typeof HandoffChunk>(
  () => import("./ChatHandoffDialog"),
);
export const QueuedPart = lazyParts<typeof QueuedChunk>(
  () => import("./ComposerQueuedPrompts"),
);
export const EditSummaryPart = lazyParts<typeof EditSummaryChunk>(
  () => import("./acp/TurnEditSummary"),
);
export const HarnessSignInPart = lazyParts<typeof HarnessSignInChunk>(
  () => import("../harness/HarnessSignInStatus"),
);
/** Load only with the first queued message. */
export function QueuedRows(
  props: ComponentProps<typeof QueuedChunk.ComposerQueuedPrompts>,
) {
  return props.prompts?.length ? (
    <QueuedPart part="ComposerQueuedPrompts" {...props} />
  ) : null;
}
