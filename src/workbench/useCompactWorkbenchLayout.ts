import { useEffect, useState } from "react";
import { COMPACT_WORKBENCH_QUERY } from "./workbenchViewport";

export function useCompactWorkbenchLayout(query = COMPACT_WORKBENCH_QUERY) {
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
