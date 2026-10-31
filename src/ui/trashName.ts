/**
 * What the operating system calls its trash, as its own file manager does:
 * "Recycle Bin" on Windows and "Trash" on macOS and Linux desktops.
 */
export function trashName(): "Recycle Bin" | "Trash" {
  if (typeof navigator === "undefined") return "Trash";
  const platform = navigator.platform || navigator.userAgent;
  return /^win|windows/i.test(platform) ? "Recycle Bin" : "Trash";
}
