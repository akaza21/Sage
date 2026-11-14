export function trashName(): "Recycle Bin" | "Trash" {
  if (typeof navigator === "undefined") return "Trash";
  const platform = navigator.platform || navigator.userAgent;
  return /^win|windows/i.test(platform) ? "Recycle Bin" : "Trash";
}
