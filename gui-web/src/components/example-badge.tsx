import type { ReactNode } from "react";
import { Icon, StatusBadge } from "./presentation";
import "../theme/sample.css";

/** Marks a value that comes from the sample data, with an icon beside the words. */
export function ExampleBadge({ children }: { children: ReactNode }) {
  return (
    <span className="du-example-badge">
      <StatusBadge tone="info">
        <Icon name="sample" />
        {children}
      </StatusBadge>
    </span>
  );
}
