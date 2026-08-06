import { useEffect, useState } from "react";

const compactWorkbenchQuery = "(max-width: 760px)";

export function useCompactWorkbenchLayout() {
  const [compact, setCompact] = useState(
    () => window.matchMedia(compactWorkbenchQuery).matches,
  );

  useEffect(() => {
    const media = window.matchMedia(compactWorkbenchQuery);
    const update = () => setCompact(media.matches);
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);

  return compact;
}
