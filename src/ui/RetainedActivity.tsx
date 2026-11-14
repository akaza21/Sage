import { Activity, useState, type ReactNode } from "react";

export function RetainedActivity({
  visible,
  children,
}: {
  visible: boolean;
  children: ReactNode;
}) {
  const [visited, setVisited] = useState(visible);
  if (visible && !visited) setVisited(true);
  if (!visited) return null;
  return <Activity mode={visible ? "visible" : "hidden"}>{children}</Activity>;
}
