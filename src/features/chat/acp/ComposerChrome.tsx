import { useEffect, useReducer } from "react";
import { Notice } from "../../../ui/Notice";
import { decodeSessionChrome, type ChromeWidget } from "./sessionChrome";
import { autoFilledRef } from "./useAcpChatActions";
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
 * below the editor are moved after the field, by flex order. An offer fills
 * the field by itself only when the field is empty, no send is in flight or
 * about to restore its prompt, and the page is this chat's; otherwise it
 * waits as a notice for Use or Ignore. Auto-filled text the user has not
 * touched yields to a failed send's restore (see `useAcpChatActions`).
 */
export function ComposerChrome({
  chatAuth,
  dictationScope: scope,
  actionBusy,
  hidden,
  projectTransitioning,
  text,
  onTextChange,
}: Pick<
  ChatComposerRegionProps,
  | "chatAuth"
  | "dictationScope"
  | "actionBusy"
  | "hidden"
  | "projectTransitioning"
  | "text"
  | "onTextChange"
>) {
  const [, refresh] = useReducer((count: number) => count + 1, 0);
  const page = chatAuth?.page;
  const chrome = decodeSessionChrome(page?.sessionSurface);
  const offer =
    chrome?.editorText && answered.get(scope) !== chrome.editorText.key
      ? chrome.editorText
      : undefined;
  const filled = autoFilledRef.current;
  const untouched = filled?.chatId === scope && filled.text === text;
  useEffect(() => {
    // Any other text in this chat's field means the suggestion is the user's.
    if (filled?.chatId === scope && !untouched) autoFilledRef.current = null;
    // The chat's `sending` covers a send in flight and its failure restore.
    if (
      !offer?.text ||
      text ||
      actionBusy ||
      hidden ||
      projectTransitioning ||
      page?.chatId !== scope
    )
      return;
    answer(scope, offer.key);
    autoFilledRef.current = { chatId: scope, text: offer.text };
    onTextChange(offer.text);
  });
  if (!chrome) return null;
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
        {/* Announced once per new offer or fill: the line is inserted afresh. */}
        <p className={untouched ? "notice" : "sr-only"} role="status">
          {untouched ? (
            <>
              Suggested text added to the message.{" "}
              <button
                type="button"
                className="notice__text-link"
                onClick={() => onTextChange("")}
              >
                Clear suggested text
              </button>
            </>
          ) : offer ? (
            <span key={offer.key}>The agent suggests text.</span>
          ) : null}
        </p>
      </div>
      {/* The input column is a flex column: these show after the field. */}
      <div className="composer__notices" style={{ order: 1 }}>
        {widgets("below")}
      </div>
    </>
  );
}
