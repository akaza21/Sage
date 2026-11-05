import type { ComponentPropsWithRef } from "react";

type Props = Omit<ComponentPropsWithRef<"button">, "type" | "aria-label"> & {
  "aria-label": string;
};

/** Plain icon controls share their accessible name and default tooltip. */
export function IconButton({
  "aria-label": label,
  title = label,
  className,
  ...props
}: Props) {
  return (
    <button
      {...props}
      className={className ? `icon-button ${className}` : "icon-button"}
      type="button"
      aria-label={label}
      title={title}
    />
  );
}
