export function browserTabTitle(url: string, documentTitle?: string | null) {
  let host: string;
  try {
    host = new URL(url).host.slice(0, 48);
  } catch {
    return null;
  }
  const title =
    typeof documentTitle === "string"
      ? Array.from(
          Array.from(documentTitle.slice(0, 512), (character) => {
            const code = character.codePointAt(0) ?? 0;
            if (
              code === 0x061c ||
              code === 0x200e ||
              code === 0x200f ||
              (code >= 0x202a && code <= 0x202e) ||
              (code >= 0x2066 && code <= 0x2069)
            )
              return "";
            return code < 32 || (code >= 127 && code <= 159) ? " " : character;
          })
            .join("")
            .replace(/\s+/g, " ")
            .trim(),
        )
          .slice(0, 80)
          .join("")
      : "";
  return title || host;
}
