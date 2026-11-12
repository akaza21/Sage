import {
  DeferredModuleSurface,
  LazyModuleBoundary,
} from "../../ui/LazyModuleBoundary";
import type { HarnessDiscoveryNotice } from "./HarnessDiscoveryNotice";
import { ArrowRightLeft } from "../../ui/leanIcons";

import {
  Component,
  Suspense,
  useState,
  type ComponentProps,
  type ComponentType,
  type ReactNode,
} from "react";
import type * as ChatNavigationChunk from "./ChatNavigation";
import type * as PermissionPromptChunk from "./acp/PermissionPrompt";
import type * as UserInputChunk from "./acp/UserInputPrompt";
import type * as SessionNoticeChunk from "./acp/SessionNoticeStrip";
import type * as TranscriptChunk from "./TranscriptInteractions";
import type * as ChatSearchChunk from "../chatSearch/ChatSearchControl";
import type * as ForkChunk from "./ForkChat";
import type * as HandoffChunk from "./ChatHandoffDialog";
import type * as HandoffControlChunk from "./ChatHandoffControl";
import type * as LegacyTranscriptChunk from "./ChatLegacyTranscript";
import type * as QueuedChunk from "./ComposerQueuedPrompts";
import type * as EditSummaryChunk from "./acp/TurnEditSummary";
import type * as ComposerOptionsChunk from "./ComposerAddActions";
import type * as HarnessSignInChunk from "../harness/HarnessSignInStatus";
import type * as GoalChunk from "./BoundChatGoalControl";
import type * as AttachmentsChunk from "./ComposerAttachments";

// eslint-disable-next-line @typescript-eslint/no-explicit-any -- keyed re-export of components
type AnyComponent = ComponentType<any>;
type Components = Record<string, AnyComponent>;

/** A part's chunk failed to import; the part, not its chat, gives way. */
class PartLoadFailed extends Error {}

/**
 * Renders the part's fallback in place of a chunk that failed to load, so
 * the surface around it stays up, and tells its owner; a later mount
 * imports again. Any other error goes on to the boundary above.
 */
class PartLoadBoundary extends Component<
  { fallback: ReactNode; onLoadFailed?: () => void; children: ReactNode },
  { error?: unknown }
> {
  state: { error?: unknown } = {};
  static getDerivedStateFromError(error: unknown) {
    return { error };
  }
  componentDidCatch(error: unknown) {
    if (error instanceof PartLoadFailed) this.props.onLoadFailed?.();
  }
  render() {
    const { error } = this.state;
    if (error === undefined) return this.props.children;
    if (error instanceof PartLoadFailed) return this.props.fallback;
    throw error;
  }
}

/** One loader per chunk; all named parts render synchronously once loaded. */
export function lazyParts<Chunk extends Components>(
  load: () => Promise<Chunk>,
  /**
   * Stands in for the chunk when it fails to load, for the mounts that were
   * waiting on it; the failure is not kept, so a later mount loads again.
   * Without it, those mounts render their fallback.
   */
  unavailable?: Chunk,
) {
  let parts: Partial<
    Record<keyof Chunk, () => Promise<{ default: AnyComponent }>>
  > = {};
  let loaded: Chunk | null = null;
  let pending: Promise<Chunk> | null = null;
  // A load that fell back to `unavailable`: the mounts showing it keep their
  // loaders across renders, and the next mount starts afresh.
  let fellBack = false;
  const loadOnce = () =>
    (pending ??= load().then(
      (chunk) => (loaded = chunk),
      (error: unknown) => {
        pending = null;
        if (!unavailable) {
          parts = {};
          throw error;
        }
        fellBack = true;
        return unavailable;
      },
    ));
  function Part<K extends keyof Chunk & string>({
    fallback = null,
    onLoadFailed,
    ...props
  }: {
    part: K;
    fallback?: ReactNode;
    /** Called when the chunk fails to load, for an owner that gives way. */
    onLoadFailed?: () => void;
  } & ComponentProps<Chunk[K]>) {
    const part: K = props.part;
    // Fix the subtree per mount.
    const [Ready] = useState<AnyComponent | undefined>(() => {
      if (fellBack) {
        fellBack = false;
        parts = {};
      }
      return loaded?.[part];
    });
    if (Ready) return <Ready {...props} />;
    const loadPart = (parts[part] ??= () =>
      loadOnce().then(
        (chunk) => ({ default: chunk[part]! }),
        (error: unknown) => {
          throw new PartLoadFailed(`The ${part} part failed to load.`, {
            cause: error,
          });
        },
      ));
    return (
      <PartLoadBoundary fallback={fallback} onLoadFailed={onLoadFailed}>
        <Suspense fallback={fallback}>
          <DeferredModuleSurface
            load={loadPart}
            fallback={fallback}
            render={(Surface) => <Surface {...props} />}
          />
        </Suspense>
      </PartLoadBoundary>
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
/** Most requests are approvals, so the question card loads on its first use. */
export const QuestionPart = lazyParts<typeof UserInputChunk>(
  () => import("./acp/UserInputPrompt"),
);
/** Harness notices, background activities and Undo are rare, so the
 * session strip loads with the first of them. */
export const NoticePart = lazyParts<typeof SessionNoticeChunk>(
  () => import("./acp/SessionNoticeStrip"),
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
        harness.characterisation === "newerThanTested" ||
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

const HandoffControlPart = lazyParts<typeof HandoffControlChunk>(
  () => import("./ChatHandoffControl"),
);
/** Loads the Hand off trigger, as a chat row menu does before it opens. */
export const preloadChatHandoffControl = HandoffControlPart.preload;
/**
 * The Hand off trigger, for the Editor chat header and a chat row menu.
 * Agent chrome renders neither on a chat's first paint, so it loads on
 * demand; until it has, an invisible copy holds its place so the controls
 * beside it never shift.
 */
export function ChatHandoffControl(
  props: ComponentProps<typeof HandoffControlChunk.ChatHandoffControl>,
) {
  return (
    <HandoffControlPart
      part="ChatHandoffControl"
      {...props}
      fallback={
        <div className="handoff-control" style={{ visibility: "hidden" }}>
          <button
            className={props.menuItem ? "menu-item" : "button button--compact"}
            type="button"
            disabled
            tabIndex={-1}
            aria-hidden="true"
          >
            <ArrowRightLeft size={14} aria-hidden="true" />
            Hand off
          </button>
        </div>
      }
    />
  );
}

/**
 * The transcript of a chat without a native session, such as the
 * development simulator's; native chats never load it.
 */
export const LegacyTranscriptPart = lazyParts<typeof LegacyTranscriptChunk>(
  () => import("./ChatLegacyTranscript"),
);
/**
 * A native Goal's control and its mutations load with the first chat that
 * shows one.
 */
export const GoalPart = lazyParts<typeof GoalChunk>(
  () => import("./BoundChatGoalControl"),
);
/** Attachment chips load with the first context attached to a draft. */
export const AttachmentsPart = lazyParts<
  Pick<typeof AttachmentsChunk, "ComposerAttachments">
>(() => import("./ComposerAttachments"));
