import { useEffect, useRef, useState, type CSSProperties } from "react";
import { utf8ByteLength } from "../../domain/text";
import {
  browserElementSink,
  handOffBrowserElement,
} from "../../app/browserAnnotations";
import {
  browserAnnotateConfirm,
  browserAnnotateFrame,
  browserControl,
  type BrowserAnnotateFrame,
  type BrowserAnnotateSelection,
} from "../../ports/nativeBrowserPort";

const pct = (value: number, total: number) => `${(value / total) * 100}%`;

/**
 * The comment box over the frozen page (Codex ref 60). Rust hid the live page
 * and kept this picture of the whole page area, so the badge and the box sit
 * exactly over the picked element. The stage keeps the picture's proportions
 * while the window resizes, and everything is placed in its shares.
 */
export function BrowserAnnotatePopover({
  selection,
  frame,
  ordinal,
  blocked,
  error,
  onConfirm,
  onCancel,
}: {
  selection: BrowserAnnotateSelection;
  frame: BrowserAnnotateFrame | null;
  ordinal: number | null;
  blocked: string | null;
  error: string | null;
  onConfirm: (comment: string) => void;
  onCancel: () => void;
}) {
  const [comment, setComment] = useState("");
  const { box, viewport } = selection;
  const left = box.x - viewport.x;
  const top = box.y - viewport.y;
  const bottom = top + box.height;
  const tooLong = utf8ByteLength(comment) > 4096;
  const reason = blocked ?? (tooLong ? "Comments hold up to 4 KB" : null);
  const outline: CSSProperties = {
    left: pct(left, viewport.width),
    top: pct(top, viewport.height),
    width: pct(box.width, viewport.width),
    height: pct(box.height, viewport.height),
  };
  const pill: CSSProperties = {
    left: `clamp(8px, ${pct(left, viewport.width)}, calc(100% - 304px))`,
    ...(bottom / viewport.height > 0.8
      ? { bottom: `calc(${pct(viewport.height - top, viewport.height)} + 8px)` }
      : { top: `calc(${pct(bottom, viewport.height)} + 8px)` }),
  };
  return (
    <div
      className="browser-annotate"
      role="dialog"
      aria-label={`Comment on ${selection.label}`}
      tabIndex={-1}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        event.stopPropagation();
        onCancel();
      }}
    >
      <div
        className="browser-annotate__stage"
        style={
          {
            "--browser-annotate-ratio": viewport.width / viewport.height,
          } as CSSProperties
        }
      >
        {frame ? (
          <img
            className="browser-annotate__frame"
            src={`data:image/jpeg;base64,${frame.jpegBase64}`}
            alt=""
            onClick={onCancel}
          />
        ) : null}
        <div className="browser-annotate__outline" style={outline} />
        <span
          className="browser-annotate__badge"
          style={{ left: outline.left, top: outline.top }}
        >
          {ordinal ?? 1}
        </span>
        <form
          className="browser-annotate__pill"
          style={pill}
          onSubmit={(event) => {
            event.preventDefault();
            if (!reason) onConfirm(comment);
          }}
        >
          <button
            type="button"
            className="browser-annotate__adjust"
            aria-label="Adjust"
            title="Adjusting styles is not available yet"
            disabled
          />
          <input
            className="browser-annotate__comment"
            aria-label="Comment"
            placeholder="Add a comment…"
            value={comment}
            autoFocus
            onChange={(event) => setComment(event.target.value)}
          />
          <button
            type="submit"
            className="browser-annotate__confirm"
            aria-label="Add comment"
            title={reason ?? undefined}
            disabled={Boolean(reason)}
          />
          {error ? (
            <p className="browser-annotate__error" role="alert">
              {error}
            </p>
          ) : null}
        </form>
      </div>
    </div>
  );
}

const NO_SINK = "The composer cannot take page elements yet";
const FULL = "This message already has 8 page elements";

/**
 * Loads the frozen frame and turns the comment into an element item. The
 * element keeps waiting until the composer took the item, so a refused item
 * stays on screen with the reason.
 */
export default function BrowserAnnotateComment({
  tabId,
  selection,
}: {
  tabId: string;
  selection: BrowserAnnotateSelection;
}) {
  const [frame, setFrame] = useState<BrowserAnnotateFrame | null>(null);
  const [error, setError] = useState<string | null>(null);
  const confirming = useRef(false);
  const sink = browserElementSink();
  const ordinal = sink?.nextOrdinal() ?? null;
  const fail = (cause: unknown) => setError(String(cause).slice(0, 512));
  useEffect(() => {
    let live = true;
    browserAnnotateFrame(tabId)
      .then((next) => live && setFrame(next))
      .catch((cause: unknown) => live && fail(cause));
    return () => {
      live = false;
    };
  }, [tabId, selection]);
  return (
    <BrowserAnnotatePopover
      selection={selection}
      frame={frame}
      ordinal={ordinal}
      blocked={!sink ? NO_SINK : ordinal === null ? FULL : null}
      error={error}
      onConfirm={(comment) => {
        if (confirming.current) return;
        confirming.current = true;
        void browserAnnotateConfirm(tabId, ordinal!, comment)
          .then((item) =>
            handOffBrowserElement(item)
              ? browserControl(tabId, "annotateCancel")
              : setError("The page element could not be attached"),
          )
          .catch(fail)
          .finally(() => (confirming.current = false));
      }}
      onCancel={() => void browserControl(tabId, "annotateCancel").catch(fail)}
    />
  );
}
