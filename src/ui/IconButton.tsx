import type { ComponentPropsWithRef } from "react";

type Props = Omit<ComponentPropsWithRef<"button">, "type" | "aria-label"> & {
  "aria-label": string;
};

/**
 * Plain icon controls show their accessible name as the shared tooltip
 * (tooltipLayer.ts), on hover and on keyboard focus. An explicit `title`
 * replaces it with a native title.
 */
export function IconButton({
  "aria-label": label,
  title,
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
