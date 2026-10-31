export default function installAnnotateDrag(report) {
  let start = null;
  let busy = false;
  let enabled = true;
  const host = document.createElement("div");
  for (const [key, value] of Object.entries({
    position: "fixed",
    inset: "0",
    "z-index": "2147483647",
    "pointer-events": "none",
  }))
    host.style.setProperty(key, value, "important");
  const shadow = host.attachShadow({ mode: "closed" });
  const outline = document.createElement("div");
  outline.style.cssText =
    "position:absolute;box-sizing:border-box;border:2px solid #3a83f7;display:none;pointer-events:none";
  shadow.append(outline);
  document.documentElement.append(host);
  const point = (event) => {
    const viewport = globalThis.visualViewport || {
      offsetLeft: 0,
      offsetTop: 0,
      width: document.documentElement.clientWidth || innerWidth,
      height: document.documentElement.clientHeight || innerHeight,
    };
    return {
      x: Math.max(
        viewport.offsetLeft,
        Math.min(viewport.offsetLeft + viewport.width, event.clientX),
      ),
      y: Math.max(
        viewport.offsetTop,
        Math.min(viewport.offsetTop + viewport.height, event.clientY),
      ),
    };
  };
  const rectangle = (end) => ({
    x: Math.min(start.x, end.x),
    y: Math.min(start.y, end.y),
    width: Math.abs(start.x - end.x),
    height: Math.abs(start.y - end.y),
  });
  const paint = (box, area) =>
    Object.assign(outline.style, {
      display: "block",
      left: `${box.x}px`,
      top: `${box.y}px`,
      width: `${box.width}px`,
      height: `${box.height}px`,
      borderStyle: area ? "dashed" : "solid",
      borderWidth: area ? "1px" : "2px",
      borderColor: area ? "#339cff" : "#3a83f7",
    });
  const consume = (event) => {
    event.preventDefault();
    event.stopImmediatePropagation();
  };
  const cancel = () => {
    start = null;
    outline.style.display = "none";
  };
  const down = (event) => {
    if (!enabled || !event.isTrusted || event.button !== 0 || busy) return;
    consume(event);
    start = point(event);
  };
  const move = (event) => {
    if (!enabled || !event.isTrusted || busy) return;
    if (start) {
      consume(event);
      paint(rectangle(point(event)), true);
      return;
    }
    const element = event.target instanceof Element ? event.target : null;
    if (!element || element === host) return;
    const box = element.getBoundingClientRect();
    paint(
      { x: box.left, y: box.top, width: box.width, height: box.height },
      false,
    );
  };
  const up = (event) => {
    if (!enabled || !event.isTrusted || event.button !== 0 || !start || busy)
      return;
    consume(event);
    const end = point(event);
    const box = rectangle(end);
    const area = box.width >= 4 && box.height >= 4;
    cancel();
    busy = true;
    const viewport = globalThis.visualViewport;
    report(
      JSON.stringify(
        area
          ? {
              target: "area",
              ...box,
              x: box.x - (viewport?.offsetLeft || 0),
              y: box.y - (viewport?.offsetTop || 0),
            }
          : { target: "element", ...end, width: 0, height: 0 },
      ),
    );
  };
  const click = (event) => {
    if (enabled && event.isTrusted) consume(event);
  };
  const key = (event) => {
    if (event.isTrusted && event.key === "Escape") cancel();
  };
  const listeners = {
    mousedown: down,
    mousemove: move,
    mouseup: up,
    click,
    dblclick: click,
    contextmenu: click,
    blur: cancel,
    scroll: cancel,
    resize: cancel,
    keydown: key,
  };
  for (const [name, listener] of Object.entries(listeners))
    window.addEventListener(name, listener, true);
  const stop = () => {
    enabled = false;
    busy = true;
    cancel();
    for (const [name, listener] of Object.entries(listeners))
      window.removeEventListener(name, listener, true);
    host.remove();
  };
  stop.clear = cancel;
  stop.resume = () => {
    enabled = true;
    cancel();
    busy = false;
    if (!host.isConnected) document.documentElement.append(host);
  };
  stop.pause = () => {
    cancel();
    busy = true;
    host.remove();
  };
  return stop;
}
