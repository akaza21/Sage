import { useEffect, type ComponentProps } from "react";

import type * as PasteChunk from "./ComposerPaste";
import { lazyParts } from "./lazyParts";
import type { ComposerPasteWiring } from "./useComposerPaste";

// The staged batch (pasted text and picked files), its chips and messages
// load with a native chat's composer or with held input; only the
// synchronous size check stays eager.
const PastePart = lazyParts<typeof PasteChunk>(
  () => import("./ComposerPaste"),
  { ComposerPaste: PasteUnavailable },
);

const PASTE_UNAVAILABLE =
  "Text over the message limit cannot be attached: attachment support failed to load.";

/**
 * Stands in for a paste part that failed to load: nothing can be staged,
 * so held input is let go, sends are not held for a batch, and a standing
 * notice says why long text is refused. The next composer mount tries the
 * import again.
 */
function PasteUnavailable({
  input: { seq, take },
  noticeId,
  onStaged,
}: ComponentProps<typeof PasteChunk.ComposerPaste>) {
  useEffect(() => void take(), [seq, take]);
  useEffect(
    () => onStaged({ revision: null, busy: false, settled: true }),
    [onStaged],
  );
  return (
    <p id={noticeId} className="composer__disabled-reason">
      {PASTE_UNAVAILABLE}
    </p>
  );
}

/** The lazy paste part, once the composer's paste wiring arms it. */
export function ComposerPasteSlot({
  paste,
  ...props
}: {
  paste: ComposerPasteWiring;
  chatId: string;
  session: ComponentProps<typeof PasteChunk.ComposerPaste>["session"];
  text: string;
  disabled: boolean;
}) {
  return paste.armed ? (
    <PastePart
      // One batch per chat: another chat's part starts empty.
      key={props.chatId}
      part="ComposerPaste"
      {...props}
      nativeAcp={paste.nativeAcp}
      activeTurn={paste.activeTurn}
      input={paste.signal}
      noticeId={paste.noticeId}
      onStaged={paste.setStaged}
    />
  ) : null;
}
