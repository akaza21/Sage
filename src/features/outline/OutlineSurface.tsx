import {
  Box,
  Braces,
  FileText,
  Folder,
  ListTree,
  SquareFunction,
  Target,
} from "../../ui/leanIcons";
import { useWorkbenchStore } from "../../app/store";
import { resourceTitle, sameResource } from "../../domain/resourceIdentity";
import { outlineKindLabel, outlineSummaryTitle } from "./outlineCard";
import { type OutlineSymbolKind, type SyntaxOutline } from "./syntaxOutline";
import { useEffect, useState } from "react";
import { type ResourceRef } from "../../domain/resource";
import { contentText, type EditorContent } from "../files/editorContent";
import { outlineAvailability } from "./outlineAvailability";

const KIND_ICONS: Readonly<Record<OutlineSymbolKind, typeof Box>> = {
  function: SquareFunction,
  class: Box,
  type: Braces,
  variable: Target,
  module: Folder,
  heading: FileText,
  property: ListTree,
  rule: Braces,
};

export function OutlineSurface() {
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  const transitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const active = useSyntaxOutline();
  const outline = active?.status === "ready" ? active.outline : null;
  const symbols = outline?.symbols ?? [];
  const resource = active?.resource;

  return (
    <section className="outline-surface" aria-label="Outline">
      <header className="outline-surface__header">
        <h1>Outline</h1>
        <span>{outlineSummaryTitle(symbols.length, outline?.truncated)}</span>
      </header>
      {resource && symbols.length ? (
        <ul className="outline-surface__list">
          {symbols.map((symbol) => {
            const kind = outlineKindLabel(symbol.kind);
            const Icon = KIND_ICONS[symbol.kind];
            return (
              <li key={`${symbol.offset}:${symbol.name}`}>
                <button
                  className="outline-card"
                  type="button"
                  aria-label={`${kind}: ${symbol.name}. ${resource.relativePath}, line ${symbol.line}, column ${symbol.column}`}
                  title={`${symbol.name} — ${resource.relativePath}:${symbol.line}:${symbol.column}`}
                  onClick={() => {
                    if (!projectId) return;
                    void useWorkbenchStore.getState().openFileLocation({
                      projectId,
                      transitionEpoch,
                      resource,
                      lineNumber: symbol.line,
                      columnNumber: symbol.column,
                      selection: { kind: "caret" },
                    });
                  }}
                >
                  <span className="outline-card__kind" aria-hidden="true">
                    <Icon size={14} strokeWidth={2} />
                  </span>
                  <span className="outline-card__copy">
                    <span className="outline-card__name">{symbol.name}</span>
                    <small className="outline-card__location">
                      <span className="outline-card__location-file">
                        {resourceTitle(resource)}
                      </span>
                      <span className="outline-card__location-position">
                        :{symbol.line}:{symbol.column}
                      </span>
                    </small>
                  </span>
                  <span className="sr-only">{kind}</span>
                </button>
              </li>
            );
          })}
        </ul>
      ) : (
        <p className="outline-surface__empty" role="status">
          {outlineEmptyMessage(active?.status)}
        </p>
      )}
    </section>
  );
}

function outlineEmptyMessage(status: ActiveOutline["status"] | undefined) {
  switch (status) {
    case "reading":
      return "Reading symbols…";
    case "unsupported":
      return "No Outline for this File type.";
    case "too-large":
      return "This File is too large for an Outline.";
    case "ready":
      return "No symbols for the active File.";
    case undefined:
      return "No File is open.";
  }
}

/** Typing settles before the document is reparsed for the Outline. */
const OUTLINE_SETTLE_MS = 200;

type ActiveOutline =
  | { resource: ResourceRef; status: "ready"; outline: SyntaxOutline }
  | { resource: ResourceRef; status: "reading" | "unsupported" | "too-large" };

/**
 * Outline of the active File, rebuilt from its syntax tree once edits settle.
 * The store is read through a subscription, not a selector, so keystrokes do
 * not re-render the Outline; it commits only when a new Outline is ready.
 */
function useSyntaxOutline(): ActiveOutline | null {
  const [active, setActive] = useState<ActiveOutline | null>(null);

  useEffect(() => {
    let request = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let seenContent: EditorContent | null = null;
    let seenResource: ResourceRef | null = null;

    const schedule = () => {
      const editor = useWorkbenchStore.getState().activeEditor;
      if (editor?.phase !== "ready") {
        if (seenResource) {
          seenResource = null;
          seenContent = null;
          request += 1;
          clearTimeout(timer);
          setActive(null);
        }
        return;
      }
      if (
        seenResource &&
        sameResource(seenResource, editor.resource) &&
        seenContent === editor.content
      ) {
        return;
      }
      const resourceChanged =
        !seenResource || !sameResource(seenResource, editor.resource);
      seenResource = editor.resource;
      seenContent = editor.content;
      const current = ++request;
      clearTimeout(timer);
      const { resource, content } = editor;
      const availability = outlineAvailability(
        resource.relativePath,
        content.length,
      );
      if (availability !== "available") {
        setActive({ resource, status: availability });
        return;
      }
      // While typing the last Outline stays; a new File says it is reading.
      if (resourceChanged) setActive({ resource, status: "reading" });
      else {
        setActive((previous) =>
          previous?.status === "ready"
            ? previous
            : { resource, status: "reading" },
        );
      }
      timer = setTimeout(
        () => {
          // The parser stays in the lazy CodeMirror chunk; Outline loads it on demand.
          void import("./syntaxOutline")
            .then(({ syntaxOutline }) =>
              syntaxOutline(
                resource.relativePath,
                contentText(content),
                () => current === request,
              ),
            )
            .then(
              (outline) => {
                if (current !== request) return;
                setActive(
                  outline
                    ? { resource, status: "ready", outline }
                    : { resource, status: "unsupported" },
                );
              },
              () => {
                if (current === request) {
                  setActive({ resource, status: "unsupported" });
                }
              },
            );
        },
        resourceChanged ? 0 : OUTLINE_SETTLE_MS,
      );
    };

    schedule();
    const unsubscribe = useWorkbenchStore.subscribe(schedule);
    return () => {
      request += 1;
      clearTimeout(timer);
      unsubscribe();
    };
  }, []);

  return active;
}
