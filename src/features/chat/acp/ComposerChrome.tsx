import { useEffect, useReducer } from "react";
import { Notice } from "../../../ui/Notice";
import { decodeSessionChrome, type ChromeWidget } from "./sessionChrome";
import type { ChatComposerRegionProps } from "../chatComposerRegionProps";

/** Offers already used or ignored, newest last; one per composer scope. */
const handled = new Map<string, string>();
const HANDLED_MAX = 32;

function remember(scope: string, key: string) {
  handled.delete(scope);
  handled.set(scope, key);
  if (handled.size > HANDLED_MAX)
    handled.delete(handled.keys().next().value as string);
}

function Widget({ widget }: { widget: ChromeWidget }) {
  return (
    <div className="notice__details" data-chrome-widget={widget.key}>
      <details open>
        <summary className="notice__text-link">{widget.key}</summary>
        <pre>
          {widget.lines.join("\n")}
          {widget.hiddenLines
            ? `\n${widget.hiddenLines} more ${widget.hiddenLines === 1 ? "line" : "lines"} not shown`
            : null}
        </pre>
      </details>
    </div>
  );
}

/**
 * A harness's subtitle, status lines, text widgets and composer offer,
 * rendered by kind for every extension from one slot in the composer's
 * input column: the subtitle and widgets placed above the editor go before
 * the field, everything else after it. An offer fills only an empty
 * composer and otherwise waits for Replace or Ignore, so a draft is never
 * overwritten.
 */
export function ComposerChrome({
  chatAuth,
  dictationScope: scope,
  text,
  onTextChange,
}: Pick<
  ChatComposerRegionProps,
  "chatAuth" | "dictationScope" | "text" | "onTextChange"
>) {
  const [, refresh] = useReducer((count: number) => count + 1, 0);
  const chrome = decodeSessionChrome(chatAuth?.page?.sessionSurface);
  const offer =
    chrome?.editorText && handled.get(scope) !== chrome.editorText.key
      ? chrome.editorText
      : undefined;
  const empty = !text.trim();
  useEffect(() => {
    if (!offer || !empty) return;
    remember(scope, offer.key);
    onTextChange(offer.text);
    refresh();
  }, [offer, empty, scope, onTextChange]);
  if (!chrome) return null;
  const settle = (replace: boolean) => {
    if (!offer) return;
    remember(scope, offer.key);
    if (replace) onTextChange(offer.text);
    refresh();
  };
  const widgets = (placement: ChromeWidget["placement"]) =>
    chrome.widgets
      .filter((widget) => widget.placement === placement)
      .map((widget) => <Widget key={widget.key} widget={widget} />);
  return (
    <>
      {/* The input column is a flex column; order places these around the field. */}
      <div className="composer__notices" style={{ order: -1 }}>
        {chrome.subtitle ? (
          <p className="notice" data-chat-subtitle="">
            {chrome.subtitle}
          </p>
        ) : null}
        {widgets("above")}
      </div>
      <div className="composer__notices" style={{ order: 1 }}>
        {widgets("below")}
        {chrome.statuses.map((status) => (
          <p
            key={status.key}
            className="notice"
            data-chrome-status={status.key}
          >
            {status.text}
          </p>
        ))}
        {offer && !empty ? (
          <div>
            <Notice
              identity={JSON.stringify([scope, "composer-offer", offer.key])}
              severity="info"
              message="The agent suggests text"
              action={{ label: "Replace", onClick: () => settle(true) }}
              details={<pre>{offer.text}</pre>}
            />
            <button
              type="button"
              className="notice__text-link"
              onClick={() => settle(false)}
            >
              Ignore
            </button>
          </div>
        ) : null}
      </div>
    </>
  );
}
