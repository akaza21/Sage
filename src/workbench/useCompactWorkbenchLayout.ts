import { useEffect, useState } from "react";

const compactWorkbenchQuery = "(max-width: 760px)";

export function useCompactWorkbenchLayout(query = compactWorkbenchQuery) {
  const [compact, setCompact] = useState(
    () => window.matchMedia(query).matches,
  );

  useEffect(() => {
    const media = window.matchMedia(query);
    const update = () => setCompact(media.matches);
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, [query]);

  return compact;
}
