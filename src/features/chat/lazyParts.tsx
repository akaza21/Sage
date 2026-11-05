import {
  DeferredModuleSurface,
  LazyModuleBoundary,
} from "../../ui/LazyModuleBoundary";
import type { HarnessDiscoveryNotice } from "./HarnessDiscoveryNotice";

import {
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
import type * as ComposerOptionsChunk from "./ComposerAddActions";
import type * as HarnessSignInChunk from "../harness/HarnessSignInStatus";

// eslint-disable-next-line @typescript-eslint/no-explicit-any -- keyed re-export of components
type AnyComponent = ComponentType<any>;
type Components = Record<string, AnyComponent>;

/** One loader per chunk; all named parts render synchronously once loaded. */
export function lazyParts<Chunk extends Components>(
  load: () => Promise<Chunk>,
) {
  let parts: Partial<
    Record<keyof Chunk, () => Promise<{ default: AnyComponent }>>
  > = {};
  let loaded: Chunk | null = null;
  let pending: Promise<Chunk> | null = null;
  const loadOnce = () =>
    (pending ??= load().then(
      (chunk) => (loaded = chunk),
      (error: unknown) => {
        pending = null;
        parts = {};
        throw error;
      },
    ));
  function Part<K extends keyof Chunk & string>({
    fallback = null,
    ...props
  }: { part: K; fallback?: ReactNode } & ComponentProps<Chunk[K]>) {
    const part: K = props.part;
    // Fix the subtree per mount.
    const [Ready] = useState<AnyComponent | undefined>(() => loaded?.[part]);
    if (Ready) return <Ready {...props} />;
    const loadPart = (parts[part] ??= () =>
      loadOnce().then((chunk) => ({ default: chunk[part]! })));
    return (
      <Suspense fallback={fallback}>
        <DeferredModuleSurface
          load={loadPart}
          fallback={fallback}
          render={(Surface) => <Surface {...props} />}
        />
      </Suspense>
    );
  }
  Part.preload = loadOnce;
  return Part;
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

const loadHarnessNotice = () =>
  import("./HarnessDiscoveryNotice").then((module) => ({
    default: module.HarnessDiscoveryNotice,
  }));

export function LazyHarnessNotice(
  props: NonNullable<Parameters<typeof HarnessDiscoveryNotice>[0]>,
) {
  const { harness, discoveryPhase } = props;
  // Settings uses the store-backed notice; a bound Chat loads only when needed.
  if (
    harness !== undefined &&
    !(
      harness?.transport === "acp" &&
      (harness.installState !== "available" ||
        harness.characterisation === "uncharacterised" ||
        (harness.accountModelDiscovery && discoveryPhase !== "ready"))
    )
  )
    return null;
  return (
    <LazyModuleBoundary<
      NonNullable<Parameters<typeof HarnessDiscoveryNotice>[0]>
    >
      label="Harness status"
      load={loadHarnessNotice}
      fallback={null}
      render={(Notice) => <Notice {...props} />}
    />
  );
}

export const ComposerOptionsPart = lazyParts<
  Pick<typeof ComposerOptionsChunk, "ComposerOptionsMenu">
>(() => import("./ComposerAddActions"));
