import { useReducer } from "react";
import { Notice } from "../../../ui/Notice";
import { decodeSessionChrome, type ChromeWidget } from "./sessionChrome";
import type { ChatComposerRegionProps } from "../chatComposerRegionProps";

/** Per composer scope, the newest offer the user answered; at most 32 scopes. */
const answered = new Map<string, string>();

function answer(scope: string, key: string) {
  answered.delete(scope);
  answered.set(scope, key);
  if (answered.size > 32)
    answered.delete(answered.keys().next().value as string);
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
 * below the editor are moved after the field, by flex order. An offer never
 * changes the composer by itself: it waits as a notice until the user uses
 * or ignores it, or the harness withdraws it (a sent prompt does).
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
  if (!chrome) return null;
  const offer =
    chrome.editorText && answered.get(scope) !== chrome.editorText.key
      ? chrome.editorText
      : undefined;
  const settle = (use: boolean) => {
    if (!offer) return;
    answer(scope, offer.key);
    if (use) onTextChange(offer.text);
    refresh();
  };
  const widgets = (placement: ChromeWidget["placement"]) =>
    chrome.widgets
      .filter((widget) => widget.placement === placement)
      .map((widget) => <Widget key={widget.key} widget={widget} />);
  return (
    <>
      <div className="composer__notices">
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
        {offer ? (
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
              label: text ? "Replace draft with suggestion" : "Use suggestion",
              onClick: () => settle(true),
            }}
            details={<pre>{offer.text}</pre>}
          />
        ) : null}
        {/* Announced once per new offer: the keyed line is inserted afresh. */}
        <p className="sr-only" role="status">
          {offer ? <span key={offer.key}>The agent suggests text.</span> : null}
        </p>
      </div>
      {/* The input column is a flex column: these show after the field. */}
      <div className="composer__notices" style={{ order: 1 }}>
        {widgets("below")}
      </div>
    </>
  );
}
