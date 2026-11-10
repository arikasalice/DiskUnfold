import type { ReactNode, Ref } from "react";
import {
  Activity,
  ArrowLeft,
  CircleAlert,
  FolderOpen,
  HardDrive,
  History,
  Layers3,
  RefreshCw,
  ScanLine,
} from "lucide-react";
import { ProgressBar } from "react-aria-components";

const icons = {
  activity: Activity,
  alert: CircleAlert,
  back: ArrowLeft,
  folder: FolderOpen,
  drive: HardDrive,
  history: History,
  layers: Layers3,
  refresh: RefreshCw,
  scan: ScanLine,
};

export type IconName = keyof typeof icons;

/** Decorative icon. An action's accessible name always comes from its label. */
export function Icon({ name }: { name: IconName }) {
  const Glyph = icons[name];
  return <Glyph aria-hidden focusable="false" size={18} strokeWidth={1.75} />;
}

export function BrandMark({ src }: { src: string }) {
  return (
    <img className="du-brand-mark" src={src} width={30} height={30} alt="" />
  );
}

export function Heading({
  level,
  children,
  id,
}: {
  level: 1 | 2 | 3;
  children: ReactNode;
  id?: string;
}) {
  const Tag = `h${level}` as const;
  return (
    <Tag id={id} className={`du-heading du-heading--${level}`}>
      {children}
    </Tag>
  );
}

export function Text({
  children,
  kind = "body",
}: {
  children: ReactNode;
  kind?: "body" | "muted" | "eyebrow" | "path";
}) {
  return <p className={`du-text du-text--${kind}`}>{children}</p>;
}

export function StatusBadge({
  children,
  tone = "neutral",
  live = false,
}: {
  children: ReactNode;
  tone?: "neutral" | "info" | "good" | "warning";
  live?: boolean;
}) {
  return (
    <span
      className={`du-badge du-badge--${tone}`}
      role={live ? "status" : undefined}
    >
      {children}
    </span>
  );
}

/** A persistent text status that can receive focus after an action is removed. */
export function LiveStatus({
  children,
  focusTarget = false,
  ref,
}: {
  children: ReactNode;
  focusTarget?: boolean;
  ref?: Ref<HTMLDivElement>;
}) {
  return (
    <div
      ref={ref}
      className="du-live-status"
      role="status"
      tabIndex={focusTarget ? -1 : undefined}
    >
      {children}
    </div>
  );
}

/** Notices are announced when they first appear. Errors are assertive. */
export function Notice({
  children,
  tone = "info",
  id,
}: {
  children: ReactNode;
  tone?: "info" | "error";
  id?: string;
}) {
  return (
    <div
      id={id}
      className={`du-notice du-notice--${tone}`}
      role={tone === "error" ? "alert" : "status"}
    >
      <Icon name={tone === "error" ? "alert" : "activity"} />
      <div>{children}</div>
    </div>
  );
}

/** An indeterminate operation does not claim a percentage. */
export function Progress({ label }: { label: string }) {
  return (
    <ProgressBar aria-label={label} isIndeterminate className="du-progress">
      <div className="du-progress__track" aria-hidden>
        <div className="du-progress__indicator" />
      </div>
    </ProgressBar>
  );
}

export function Readout({
  label,
  rows,
}: {
  label: string;
  rows: { label: string; value: ReactNode }[];
}) {
  if (rows.length === 0) return null;
  return (
    <dl className="du-readout" aria-label={label}>
      {rows.map((row) => (
        <div key={row.label}>
          <dt>{row.label}</dt>
          <dd>{row.value}</dd>
        </div>
      ))}
    </dl>
  );
}

/** Groups a displayed value and its supporting evidence under one label. */
export function Metric({
  label,
  children,
  detail,
  prominent = false,
}: {
  label: string;
  children: ReactNode;
  detail?: string | undefined;
  prominent?: boolean;
}) {
  return (
    <div
      className="du-metric"
      data-prominent={prominent || undefined}
      role="group"
      aria-label={label}
    >
      <p className="du-metric__label">{label}</p>
      <p className="du-metric__value">{children}</p>
      {detail && <p className="du-metric__detail">{detail}</p>}
    </div>
  );
}
