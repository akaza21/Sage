import { useEffect, useState } from "react";
import "./fork-chat.css";
import { AcpDesktopError, invokeAcp } from "../../services/acpDesktopCommand";
import { isRecord } from "../../services/acpDesktopGuards";
import { sessionRequest } from "./acp/useSessionControl";

/** At most 200 paths, with the count of all of them. */
type Listed = { paths: string[]; count: number };
/**
 * The native dry run: the files it keeps with why, or else the files it would
 * restore. `caveat` (what rewind does not undo) comes exactly when it restores.
 */
export type Preview = {
  fileCount: number;
  sections: (Listed & { note: string; keeps?: true })[];
  caveat: string | null;
};
/** The preview, `null` when files stay as they are, or why the check failed. */
export type Found = Preview | null | string;

const within = (value: unknown, max: number): value is unknown[] =>
  Array.isArray(value) && value.length <= max;
const isText = (value: unknown) => typeof value === "string";

/** A dry run only: no file changes until the user confirms. `null` means files stay. */
async function preview(chatId: string, before: string) {
  const request = sessionRequest(chatId, { kind: "previewRewind", before });
  if (!request) return null;
  const value = await invokeAcp<{ preview?: unknown }, object>(
    "control_acp_session",
    request,
  );
  const found = value?.preview;
  if (found === undefined) return null;
  if (
    !isRecord(found) ||
    !Number.isSafeInteger(found.fileCount) ||
    !(found.caveat === null || isText(found.caveat)) ||
    !within(found.sections, 2) ||
    !found.sections.every(
      (section) =>
        isRecord(section) &&
        isText(section.note) &&
        within(section.paths, 200) &&
        section.paths.every(isText) &&
        Number.isSafeInteger(section.count),
    )
  )
    throw new Error();
  return found as Preview;
}

/**
 * The file preview for rewinding to before `turnId`, fetched again whenever
 * `asked` changes; `undefined` while it loads or when there is no `turnId`.
 */
export function useFileRewindPreview(
  chatId: string,
  turnId: string | null | undefined,
  asked = 0,
): Found | undefined {
  const key = turnId && turnId + asked;
  const [found, setFound] = useState<[string, Found]>();
  useEffect(() => {
    if (!key || !turnId) return;
    let live = true;
    preview(chatId, turnId).then(
      (next) => live && setFound([key, next]),
      (error: unknown) =>
        live &&
        setFound([
          key,
          `Couldn't check the files. ${error instanceof AcpDesktopError ? error.message : ""}`,
        ]),
    );
    return () => {
      live = false;
    };
  }, [chatId, turnId, key]);
  return found?.[0] === key ? found[1] : undefined;
}

function Paths({ paths, count }: Listed) {
  return (
    <ul className="rewind-files">
      {paths.map((path) => (
        <li key={path}>{path}</li>
      ))}
      {count > paths.length ? <li>+{count - paths.length} more</li> : null}
    </ul>
  );
}

/** The files a rewind keeps with why, or restores with its caveat, or why the check failed. */
export function FileRewindPreview({ found }: { found: Found | undefined }) {
  if (!found) return null;
  if (typeof found === "string") return <p role="alert">{found}</p>;
  return (
    <>
      {found.sections.map((section) => (
        <div key={section.note}>
          <p role={section.keeps && "alert"}>{section.note}</p>
          <Paths {...section} />
        </div>
      ))}
      {found.caveat ? (
        <p>
          <small>{found.caveat}</small>
        </p>
      ) : null}
    </>
  );
}
