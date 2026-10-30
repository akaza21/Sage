export default function annotationBadges(entries, url) {
  globalThis.__sageAnnotationBadges?.stop();
  const host = document.createElement("div");
  host.style.cssText =
    "all:initial;position:fixed;inset:0;z-index:2147483646;pointer-events:none";
  const shadow = host.attachShadow({ mode: "closed" });
  const badges = entries.map(({ ordinal, box }) => {
    const badge = document.createElement("span");
    badge.textContent = String(ordinal);
    badge.style.cssText =
      "position:absolute;display:grid;place-items:center;width:26px;height:26px;border:1px solid white;border-radius:50% 50% 50% 2px;background:#339cff;color:white;font:12px sans-serif";
    shadow.append(badge);
    return { badge, box };
  });
  const paint = () => {
    if (location.href !== url) return stop();
    for (const { badge, box } of badges) {
      badge.style.left = `${box.x - scrollX}px`;
      badge.style.top = `${box.y - scrollY}px`;
    }
  };
  const stop = () => {
    host.remove();
    window.removeEventListener("scroll", paint, true);
    window.removeEventListener("resize", paint, true);
    window.removeEventListener("hashchange", paint);
    window.removeEventListener("popstate", paint);
  };
  window.addEventListener("scroll", paint, true);
  window.addEventListener("resize", paint, true);
  window.addEventListener("hashchange", paint);
  window.addEventListener("popstate", paint);
  const resume = () => {
    if (entries.length) document.documentElement.append(host);
    paint();
  };
  resume();
  globalThis.__sageAnnotationBadges = {
    stop,
    pause: () => host.remove(),
    resume,
  };
}
