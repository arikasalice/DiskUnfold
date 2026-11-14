import type { ReactNode } from "react";
import { Notice, StatusBadge, Text } from "./presentation";
import { Icon } from "./presentation";
import "../theme/sample.css";

/**
 * Global strip shown for as long as a sample session is open. The state is
 * conveyed by the icon and the `status` words, never by colour alone.
 */
export function SampleBanner({
  label,
  status,
  detail,
  children,
}: {
  /** Accessible name of the strip. */
  label: string;
  /** Short words such as "Sample experience in progress". */
  status: string;
  detail?: string | undefined;
  /** Actions the user can take while the sample is open. */
  children?: ReactNode;
}) {
  return (
    <section className="du-sample-banner" aria-label={label}>
      <Notice icon="sample" announce={false}>
        <div className="du-sample-banner__body">
          <StatusBadge tone="info">{status}</StatusBadge>
          {detail && <Text kind="muted">{detail}</Text>}
          {children && (
            <div className="du-sample-banner__actions">{children}</div>
          )}
        </div>
      </Notice>
    </section>
  );
}

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
