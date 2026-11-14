import type { ComponentPropsWithRef } from "react";

type Props = Omit<ComponentPropsWithRef<"button">, "type" | "aria-label"> & {
  "aria-label": string;
};

/**
 * Plain icon controls show their accessible name as the shared tooltip
 * (tooltipLayer.ts), on hover and on keyboard focus. An explicit `title`, or
 * a disabled button, uses a native title instead.
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
      // A disabled button gets no pointer events in WebKit, so it keeps the
      // native title there and everywhere.
      title={title ?? (props.disabled ? label : undefined)}
    />
  );
}
