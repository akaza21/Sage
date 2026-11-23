import { useEffect, useReducer, useRef, useState } from "react";
import { Notice } from "../../../ui/Notice";
import { decodeSessionChrome, type ChromeWidget } from "./sessionChrome";
import type { ChatComposerRegionProps } from "../chatComposerRegionProps";

/**
 * Per composer scope, the newest offer already answered (applied, ignored
 * or sent past) and the newest one shown beside a draft. Each keeps at most
 * 32 scopes.
 */
const settled = new Map<string, string>();
const shown = new Map<string, string>();

function remember(map: Map<string, string>, scope: string, key: string) {
  map.delete(scope);
  map.set(scope, key);
  if (map.size > 32) map.delete(map.keys().next().value as string);
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
 * rendered by kind for every extension from one slot before the field, so
 * they are read in the order they show. Only widgets an extension placed
 * below the editor are moved after the field, by flex order. An offer fills
 * only a composer that is empty and was never shown it beside a draft;
 * otherwise it waits for Replace or Ignore. Sending or queueing answers it,
 * so a draft is never overwritten.
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
  const [filled, setFilled] = useState(false);
  const slot = useRef<HTMLDivElement>(null);
  const chrome = decodeSessionChrome(chatAuth?.page?.sessionSurface);
  const offer =
    chrome?.editorText && settled.get(scope) !== chrome.editorText.key
      ? chrome.editorText
      : undefined;
  const key = offer?.key;
  const suggestion = offer?.text;
  const empty = text === "";
  const banner = offer && (!empty || shown.get(scope) === offer.key);
  useEffect(() => {
    if (!key || suggestion === undefined) return;
    if (!empty) return remember(shown, scope, key);
    const field = slot.current?.closest("form")?.querySelector("textarea");
    if (shown.get(scope) === key || field?.value) return;
    remember(settled, scope, key);
    onTextChange(suggestion);
    setFilled(true);
    refresh();
  }, [key, suggestion, empty, scope, onTextChange]);
  useEffect(() => {
    const form = slot.current?.closest("form");
    if (!key || !form) return;
    const answer = () => {
      remember(settled, scope, key);
      refresh();
    };
    form.addEventListener("submit", answer);
    return () => form.removeEventListener("submit", answer);
  }, [key, scope]);
  if (!chrome) return null;
  const settle = (replace: boolean) => {
    if (!offer) return;
    remember(settled, scope, offer.key);
    if (replace) onTextChange(offer.text);
    refresh();
  };
  const widgets = (placement: ChromeWidget["placement"]) =>
    chrome.widgets
      .filter((widget) => widget.placement === placement)
      .map((widget) => <Widget key={widget.key} widget={widget} />);
  return (
    <>
      <div className="composer__notices" ref={slot}>
        {chrome.subtitle ? (
          <p className="notice" data-chat-subtitle="">
            {chrome.subtitle}
          </p>
        ) : null}
        {widgets("above")}
        {chrome.statuses.map((status) => (
          <p
            key={status.key}
            className="notice"
            data-chrome-status={status.key}
          >
            {status.text}
          </p>
        ))}
        {banner ? (
          <Notice
            identity={JSON.stringify([scope, "composer-offer", offer.key])}
            severity="info"
            message={
              <>
                The agent suggests text.{" "}
                <button
                  type="button"
                  className="notice__text-link"
                  onClick={() => settle(false)}
                >
                  Ignore suggestion
                </button>
              </>
            }
            action={{
              label: "Replace draft with suggestion",
              onClick: () => settle(true),
            }}
            details={<pre>{offer.text}</pre>}
          />
        ) : null}
        <p className="sr-only" role="status">
          {filled ? "Suggested text added to the message." : ""}
        </p>
      </div>
      {/* The input column is a flex column: these show after the field. */}
      <div className="composer__notices" style={{ order: 1 }}>
        {widgets("below")}
      </div>
    </>
  );
}
