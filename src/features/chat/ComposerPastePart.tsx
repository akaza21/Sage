import { useEffect, type ComponentProps } from "react";

import type * as PasteChunk from "./ComposerPaste";
import { lazyParts } from "./lazyParts";
import type { ComposerPasteWiring } from "./useComposerPaste";

const PastePart = lazyParts<typeof PasteChunk>(
  () => import("./ComposerPaste"),
  { ComposerPaste: PasteUnavailable },
);

const PASTE_UNAVAILABLE =
  "Text over the message limit cannot be attached: attachment support failed to load.";

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
