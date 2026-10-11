export function richContentReadiness(turn, visible, style) {
  const codes = turn?.querySelectorAll(".acp-code-block code") ?? [];
  const images = turn?.querySelectorAll(".acp-image__src") ?? [];
  if (codes.length > 64 || images.length > 64)
    throw Error("probe-rich-content-bound");
  const blocks = Array.from(codes, (code) => {
    const tokens = code.querySelectorAll("span");
    if (tokens.length > 512) throw Error("probe-rich-content-bound");
    return { code, tokens };
  });
  const codeHighlighted = blocks.some(
    ({ code, tokens }) =>
      visible(code) &&
      Array.from(tokens).some(
        (token) =>
          visible(token) &&
          token.textContent.trim() &&
          style(token).color !== style(code).color,
      ),
  );
  const imageLoaded = Array.from(images).some(
    (image) =>
      visible(image) &&
      image.complete &&
      image.naturalWidth === 320 &&
      image.naturalHeight === 180,
  );
  return { codeHighlighted, imageLoaded };
}
