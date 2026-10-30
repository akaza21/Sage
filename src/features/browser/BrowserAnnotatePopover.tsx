import {
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { utf8ByteLength } from "../../domain/text";
import {
  browserElementSink,
  handOffBrowserElement,
  parseBrowserElement,
} from "../../app/browserAnnotations";
import {
  browserControl,
  type BrowserAnnotateSelection,
} from "../../ports/nativeBrowserPort";
import {
  browserAnnotateConfirm,
  browserAnnotateFrame,
  type BrowserAnnotateFrame,
  type BrowserAnnotateCrop,
} from "../../ports/nativeBrowserAnnotatePort";
import { BrowserAreaPicture } from "./BrowserAreaPicture";
import {
  forgetBrowserCrop,
  rememberBrowserCrop,
} from "../../app/browserCropPreview";
import { useBrowserPicturePlacement } from "./useBrowserPicturePlacement";

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
  picture,
}: {
  selection: BrowserAnnotateSelection;
  frame: BrowserAnnotateFrame | null;
  ordinal: number | null;
  blocked: string | null;
  error: string | null;
  onConfirm: (comment: string) => void;
  onCancel: () => void;
  picture?: ReactNode;
}) {
  const [comment, setComment] = useState("");
  const stage = useRef<HTMLDivElement>(null);
  useBrowserPicturePlacement(stage);
  const { box, viewport } = selection;
  const left = box.x - viewport.x;
  const top = box.y - viewport.y;
  const bottom = top + box.height;
  const area = selection.target === "area";
  const tooLong = utf8ByteLength(comment) > 4096;
  const reason = blocked ?? (tooLong ? "Comments hold up to 4 KB" : null);
  const outline: CSSProperties = {
    left: pct(left, viewport.width),
    top: pct(top, viewport.height),
    width: pct(box.width, viewport.width),
    height: pct(box.height, viewport.height),
  };
  const pill: CSSProperties = {
    left: `clamp(8px, calc(${pct(left, viewport.width)} + ${area ? 24 : 0}px), calc(100% - 304px))`,
    ...(area
      ? {
          top: `clamp(8px, calc(${pct(bottom, viewport.height)} - 44px), calc(100% - 52px))`,
        }
      : bottom / viewport.height > 0.8
        ? {
            bottom: `calc(${pct(viewport.height - top, viewport.height)} + 8px)`,
          }
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
        ref={stage}
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
            src={
              frame.pngBase64
                ? `data:image/png;base64,${frame.pngBase64}`
                : `data:image/jpeg;base64,${frame.jpegBase64}`
            }
            alt=""
            onClick={onCancel}
          />
        ) : null}
        <div
          className={`browser-annotate__outline${selection.target === "area" ? " browser-annotate__outline--area" : ""}`}
          style={outline}
        />
        {!area ? (
          <span
            className="browser-annotate__badge"
            style={{ left: outline.left, top: outline.top }}
          >
            {ordinal ?? 1}
          </span>
        ) : null}
        <form
          className="browser-annotate__pill"
          style={pill}
          onSubmit={(event) => {
            event.preventDefault();
            if (!reason) onConfirm(comment);
          }}
        >
          {area ? (
            <span className="browser-annotate__badge browser-annotate__badge--area">
              {ordinal ?? 1}
            </span>
          ) : null}
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
          {error && !picture ? (
            <p className="browser-annotate__error" role="alert">
              {error}
            </p>
          ) : null}
          {picture ? (
            <div className="browser-annotate__details">
              {error ? (
                <p className="browser-annotate__error" role="alert">
                  {error}
                </p>
              ) : null}
              {picture}
            </div>
          ) : null}
        </form>
      </div>
    </div>
  );
}

const NO_SINK = "The composer cannot take page elements yet";
const FULL = "Open an Agent Chat whose message has room for this element";

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
  const [crop, setCrop] = useState<BrowserAnnotateCrop | null>(null);
  const [pictureBlocked, setPictureBlocked] = useState(false);
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
      blocked={
        !sink
          ? NO_SINK
          : ordinal === null
            ? FULL
            : pictureBlocked
              ? "Wait for the masked picture, or remove it"
              : null
      }
      picture={
        selection.target === "area" ? (
          <BrowserAreaPicture
            tabId={tabId}
            onChange={(next, blocked) => {
              setCrop(next);
              setPictureBlocked(blocked);
            }}
          />
        ) : undefined
      }
      error={error}
      onConfirm={(comment) => {
        if (confirming.current) return;
        confirming.current = true;
        void browserAnnotateConfirm(tabId, ordinal!, comment)
          .then((item) => {
            const context = parseBrowserElement(item);
            if (context && crop && !rememberBrowserCrop(context, crop))
              return setError(
                "Remove a draft picture before adding another (8 picture limit)",
              );
            if (!handOffBrowserElement(item)) {
              if (context) forgetBrowserCrop(context);
              return setError("The page element could not be attached");
            }
            return browserControl(tabId, "annotateCancel");
          })
          .catch(fail)
          .finally(() => (confirming.current = false));
      }}
      onCancel={() => void browserControl(tabId, "annotateCancel").catch(fail)}
    />
  );
}
