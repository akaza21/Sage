export default function readAnnotateArea(box) {
  const viewport = globalThis.visualViewport || {
    pageLeft: scrollX,
    pageTop: scrollY,
    offsetLeft: 0,
    offsetTop: 0,
    scale: 1,
    width: innerWidth,
    height: innerHeight,
  };
  box = {
    ...box,
    x: box.x + viewport.offsetLeft,
    y: box.y + viewport.offsetTop,
  };
  const intersects = (rect) =>
    rect.width > 0 &&
    rect.height > 0 &&
    rect.right > box.x &&
    rect.bottom > box.y &&
    rect.left < box.x + box.width &&
    rect.top < box.y + box.height;
  const control =
    'input,textarea,select,[contenteditable]:not([contenteditable="false"])';
  const skip = `${control},option,script,style,noscript,template,[aria-hidden="true"],iframe`;
  const alpha = (color) =>
    color === "transparent"
      ? 0
      : parseFloat(
          (color.match(/^rgba\(.*,\s*([\d.]+)\)$|\/\s*([\d.]+)\)$/) || [])
            .slice(1)
            .find(Boolean) ?? "1",
        );
  const visibleText = (element) => {
    const style = getComputedStyle(element);
    return (
      parseFloat(style.fontSize) >= 2 &&
      alpha(style.color) >= 0.1 &&
      alpha(style.webkitTextFillColor) >= 0.1
    );
  };
  const visible = (element) => {
    let depth = 0;
    for (let current = element; current; current = current.parentElement) {
      if (++depth > 64) return false;
      const style = getComputedStyle(current);
      if (
        style.display === "none" ||
        style.visibility !== "visible" ||
        Number(style.opacity) === 0 ||
        current.getAttribute("aria-hidden") === "true"
      )
        return false;
    }
    const rect = element.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0 && intersects(rect);
  };
  const nodes = [],
    masks = [];
  const mask = (rect) => {
    if (!intersects(rect)) return true;
    const box = {
      x: rect.left + scrollX,
      y: rect.top + scrollY,
      width: rect.width,
      height: rect.height,
    };
    if (
      masks.some(
        (covered) =>
          covered.x <= box.x &&
          covered.y <= box.y &&
          covered.x + covered.width >= box.x + box.width &&
          covered.y + covered.height >= box.y + box.height,
      )
    )
      return true;
    if (masks.length === 512) return false;
    masks.push(box);
    return true;
  };
  const walk = document.createTreeWalker(
    document.documentElement,
    NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT,
  );
  let visited = 0;
  for (let node = walk.currentNode; node; node = walk.nextNode()) {
    if (++visited > 20000) return null;
    if (node instanceof Element) {
      if (
        (node.closest(control) ||
          node.localName === "iframe" ||
          node.shadowRoot ||
          node.localName.includes("-")) &&
        intersects(node.getBoundingClientRect())
      ) {
        if (!mask(node.getBoundingClientRect())) return null;
        if (nodes.length < 64 && node.matches(control) && visible(node))
          nodes.push({
            role: node.localName === "select" ? "combobox" : "textbox",
            text: "",
          });
      }
      if (
        nodes.length < 64 &&
        node.matches("img,[aria-label]") &&
        !node.closest(skip) &&
        visible(node)
      ) {
        const text = (
          node.getAttribute("aria-label") ||
          node.getAttribute("alt") ||
          ""
        ).slice(0, 256);
        if (text)
          nodes.push({
            role: node.getAttribute("role")?.slice(0, 64) || node.localName,
            text,
          });
      }
      continue;
    }
    if (!(node instanceof Text)) continue;
    const parent = node.parentElement;
    if (parent?.closest(control)) {
      const range = document.createRange();
      range.selectNodeContents(node);
      if (!mask(range.getBoundingClientRect())) return null;
      continue;
    }
    if (nodes.length === 64) continue;
    if (
      !parent ||
      parent.closest(skip) ||
      !visible(parent) ||
      !visibleText(parent)
    )
      continue;
    const text = node.data.slice(0, 256).replace(/\s+/g, " ").trim();
    if (!text) continue;
    const range = document.createRange();
    range.selectNodeContents(node);
    if (!intersects(range.getBoundingClientRect())) continue;
    const semantic = parent.closest("[role],h1,h2,h3,h4,h5,h6,p,a,button,li");
    const tag = semantic?.localName || parent.localName;
    const role =
      semantic?.getAttribute("role")?.slice(0, 64) ||
      (/^h[1-6]$/.test(tag)
        ? "heading"
        : tag === "a"
          ? "link"
          : tag === "p"
            ? "paragraph"
            : tag);
    nodes.push({ role, text });
  }
  return {
    title: document.title.slice(0, 1024),
    nodes,
    masks,
    viewport: {
      x: viewport.pageLeft,
      y: viewport.pageTop,
      width: innerWidth / viewport.scale,
      height: innerHeight / viewport.scale,
      clientWidth: viewport.width,
      clientHeight: viewport.height,
      scale: devicePixelRatio * viewport.scale,
    },
  };
}
