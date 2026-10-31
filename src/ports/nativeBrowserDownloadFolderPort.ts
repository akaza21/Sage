import { invokeNative } from "./nativeInvoke";

/**
 * The saved browser download folder; see
 * `settings_commands/browser_download_folder.rs`. No command takes a path:
 * Change opens a native folder picker in Rust.
 */
export interface BrowserDownloadFolder {
  /** The saved folder, or null when the next download asks. */
  path: string | null;
  /** False when the saved folder is gone or read-only; the next download asks. */
  available: boolean;
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
  return { path, available: path !== null && source.available === true };
}
