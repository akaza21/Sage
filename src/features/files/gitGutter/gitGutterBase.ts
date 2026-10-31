import { resourceKey } from "../../../domain/resourceIdentity";
import type { FileGitBase, ResourceRef } from "../../../domain/workbench";

/** EP-14 budget: staged texts kept for the gutter, across Files. */
export const MAX_GUTTER_BASE_BYTES = 16 * 1024 * 1024;
/** Files whose last blob id is remembered. */
const MAX_REMEMBERED_FILES = 512;

type ReadBase = (
  resource: ResourceRef,
  knownOid: string | null,
) => Promise<FileGitBase>;

/**
 * Staged texts by blob id, least recently used first, within
 * `MAX_GUTTER_BASE_BYTES` (UTF-16 code units × 2). A File asks with the blob
 * it last saw, so an unchanged index costs Git one `ls-files` and no text
 * crosses IPC.
 */
export function createGutterBaseCache(
  read: ReadBase,
  maxBytes = MAX_GUTTER_BASE_BYTES,
) {
  const texts = new Map<string, string>();
  const lastOid = new Map<string, string>();
  let bytes = 0;

  const remember = (oid: string, content: string) => {
    const size = content.length * 2;
    if (size > maxBytes) return;
    const previous = texts.get(oid);
    if (previous !== undefined) {
      texts.delete(oid);
      bytes -= previous.length * 2;
    }
    texts.set(oid, content);
    bytes += size;
    for (const [key, text] of texts) {
      if (bytes <= maxBytes) break;
      texts.delete(key);
      bytes -= text.length * 2;
    }
  };

  const rememberOid = (key: string, oid: string) => {
    lastOid.delete(key);
    lastOid.set(key, oid);
    if (lastOid.size > MAX_REMEMBERED_FILES)
      lastOid.delete(lastOid.keys().next().value!);
  };

  return {
    /** The staged text, or null when the File gets no markers. */
    async load(resource: ResourceRef): Promise<string | null> {
      const key = resourceKey(resource);
      const known = lastOid.get(key) ?? null;
      const cached = known === null ? undefined : texts.get(known);
      const answer = await read(resource, cached === undefined ? null : known);
      if (answer.state === "none") {
        lastOid.delete(key);
        return null;
      }
      rememberOid(key, answer.oid);
      if (answer.state === "ready") {
        remember(answer.oid, answer.content);
        return answer.content;
      }
      const text = texts.get(answer.oid);
      if (text !== undefined) {
        remember(answer.oid, text);
        return text;
      }
      // Evicted while the request ran: ask again for the text.
      lastOid.delete(key);
      const fresh = await read(resource, null);
      if (fresh.state !== "ready") return null;
      rememberOid(key, fresh.oid);
      remember(fresh.oid, fresh.content);
      return fresh.content;
    },
    bytes: () => bytes,
  };
}
