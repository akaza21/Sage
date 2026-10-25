import { useEffect, useState } from "react";

import { useWorkbenchStore } from "../../app/store";
import type { ResourceRef } from "../../domain/resource";
import { sameResource } from "../../domain/resourceIdentity";
import { outlineAvailability } from "./outlineAvailability";
import type { SyntaxOutline } from "./syntaxOutline";

/** Typing settles before the document is reparsed for the Outline. */
export const OUTLINE_SETTLE_MS = 200;

export type ActiveOutline =
  | { resource: ResourceRef; status: "ready"; outline: SyntaxOutline }
  | { resource: ResourceRef; status: "reading" | "unsupported" | "too-large" };

/**
 * Outline of the active File, rebuilt from its syntax tree once edits settle.
 * The store is read through a subscription, not a selector, so keystrokes do
 * not re-render the Outline; it commits only when a new Outline is ready.
 */
export function useSyntaxOutline(): ActiveOutline | null {
  const [active, setActive] = useState<ActiveOutline | null>(null);

  useEffect(() => {
    let request = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let seenContent: string | null = null;
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
                content,
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
