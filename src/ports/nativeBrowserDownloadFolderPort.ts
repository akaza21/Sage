import { invokeNative } from "./nativeInvoke";

/**
 * The saved browser download folder; see
 * `settings_commands/browser_download_folder.rs`. No command takes a path:
 * Change opens a native folder picker in Rust.
 */
export interface BrowserDownloadFolder {
  path: string | null;
  available: boolean;
  /** SQLite change sequence, shared by snapshots, replies, and events. */
  revision: number;
}

export function browserDownloadFolder(): Promise<unknown> {
  return invokeNative("browser_download_folder");
}

export function browserChooseDownloadFolder(): Promise<unknown> {
  return invokeNative("browser_choose_download_folder");
}

export function browserResetDownloadFolder(): Promise<unknown> {
  return invokeNative("browser_reset_download_folder");
}

/** Keep only the known fields; Rust owns and re-checks the folder. */
export function admitDownloadFolder(input: unknown): BrowserDownloadFolder {
  const source =
    typeof input === "object" && input !== null
      ? (input as Record<string, unknown>)
      : {};
  const path =
    typeof source.path === "string" && source.path.length > 0
      ? source.path
      : null;
  const revision =
    typeof source.revision === "number" &&
    Number.isSafeInteger(source.revision) &&
    source.revision >= 0
      ? source.revision
      : 0;
  return {
    path,
    available: path !== null && source.available === true,
    revision,
  };
}

export async function listenBrowserDownloadFolder(
  onChange: (folder: BrowserDownloadFolder) => void,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<unknown>("browser:download-folder", (event) =>
    onChange(admitDownloadFolder(event.payload)),
  );
}
