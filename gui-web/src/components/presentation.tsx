import type { ReactNode, Ref } from "react";
import {
  Activity,
  ArrowLeft,
  Circle,
  CircleAlert,
  CircleCheck,
  CircleX,
  Copy,
  FolderOpen,
  HardDrive,
  History,
  Info,
  Languages,
  Layers3,
  RefreshCw,
  ScanLine,
  TriangleAlert,
} from "lucide-react";
import { ProgressBar } from "react-aria-components";
import { useLocaleText } from "../i18n/presentation";
import { STATUS_TONES } from "./status-table";
import type { StatusRow, StatusShape, StatusTone } from "./status-table";
import "../theme/status-badge.css";

const icons = {
  activity: Activity,
  alert: CircleAlert,
  back: ArrowLeft,
  copy: Copy,
  folder: FolderOpen,
  drive: HardDrive,
  history: History,
  language: Languages,
  layers: Layers3,
  refresh: RefreshCw,
  scan: ScanLine,
};

export type IconName = keyof typeof icons;

const statusShapes = {
  dot: Circle,
  info: Info,
  check: CircleCheck,
  warning: TriangleAlert,
  cross: CircleX,
} satisfies Record<StatusShape, typeof Circle>;

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
  focusable = false,
}: {
  children: ReactNode;
  kind?: "body" | "muted" | "eyebrow" | "path";
  focusable?: boolean;
}) {
  return (
    <p
      className={`du-text du-text--${kind}`}
      tabIndex={focusable ? 0 : undefined}
    >
      {children}
    </p>
  );
}

/**
 * A status shown by shape, text and colour together.
 *
 * Pass the already localized text as `children` with a `tone`, or pass a
 * {@link StatusRow} whose message key names the status. The shape comes from
 * the status table, so the status is readable without colour.
 */
export function StatusBadge({
  children,
  tone = "neutral",
  status,
  live = false,
}: {
  children?: ReactNode;
  tone?: StatusTone;
  status?: StatusRow;
  live?: boolean;
}) {
  const text = useLocaleText();
  const shape = status?.shape ?? STATUS_TONES[tone].shape;
  const Glyph = statusShapes[shape];
  const shown = status?.tone ?? tone;
  return (
    <span
      className={`du-badge du-badge--${shown}`}
      data-status-shape={shape}
      role={live ? "status" : undefined}
    >
      <Glyph
        className="du-badge__shape"
        aria-hidden
        focusable="false"
        size={14}
        strokeWidth={2}
      />
      {children ?? (status && text(status.messageKey, status.messageKey))}
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

/**
 * Notices are announced when they first appear. Errors are assertive.
 *
 * A notice that is placed inside a persistent live region passes
 * `announce={false}`, so the region announces it once when its content changes.
 */
export function Notice({
  children,
  tone = "info",
  id,
  announce = true,
}: {
  children: ReactNode;
  tone?: "info" | "error";
  id?: string;
  announce?: boolean;
}) {
  let role: "alert" | "status" | undefined;
  if (announce) role = tone === "error" ? "alert" : "status";
  return (
    <div id={id} className={`du-notice du-notice--${tone}`} role={role}>
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
  rows: {
    label: string;
    value: ReactNode;
    wideValue?: boolean;
    emphasized?: boolean;
    notes?: readonly string[];
  }[];
}) {
  if (rows.length === 0) return null;
  return (
    <dl className="du-readout" aria-label={label}>
      {rows.map((row) => (
        <div
          key={row.label}
          className={
            [
              row.wideValue && "du-readout__wide-value",
              row.emphasized && "du-readout__emphasized",
            ]
              .filter(Boolean)
              .join(" ") || undefined
          }
        >
          <dt>{row.label}</dt>
          <dd>
            {row.value}
            {row.notes?.map((note, index) => (
              <span className="du-readout__note" key={index}>
                {note}
              </span>
            ))}
          </dd>
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
