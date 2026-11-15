import type { ReactNode, Ref } from "react";
import {
  Activity,
  ArrowLeft,
  Circle,
  CircleAlert,
  CircleCheck,
  CircleX,
  Copy,
  FlaskConical,
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
import type { TimestampParts } from "../i18n/regional-format";
import { ExplainedText } from "./explained-text";
import { STATUS_TONES } from "./status-table";
import type { StatusRow, StatusShape, StatusTone } from "./status-table";
import "../theme/status-badge.css";

const icons = {
  activity: Activity,
  alert: CircleAlert,
  back: ArrowLeft,
  copy: Copy,
  error: CircleX,
  folder: FolderOpen,
  sample: FlaskConical,
  drive: HardDrive,
  history: History,
  info: Info,
  language: Languages,
  layers: Layers3,
  refresh: RefreshCw,
  scan: ScanLine,
  success: CircleCheck,
  warning: TriangleAlert,
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

/**
 * A heading of the page outline. `ref` with `programmaticFocus` lets a screen
 * move the focus to its title when it opens; that focus stop is reachable only
 * by script and draws no frame around the text.
 */
export function Heading({
  level,
  children,
  id,
  ref,
  programmaticFocus = false,
}: {
  level: 1 | 2 | 3;
  children: ReactNode;
  id?: string;
  ref?: Ref<HTMLHeadingElement>;
  programmaticFocus?: boolean;
}) {
  const Tag = `h${level}` as const;
  return (
    <Tag
      id={id}
      ref={ref}
      tabIndex={programmaticFocus ? -1 : undefined}
      className={`du-heading du-heading--${level}`}
    >
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
 * A value that a line never breaks inside, such as a size with its unit
 * ("At least 390.6 KiB"). Its container wraps around it as a whole.
 */
export function Unbroken({ children }: { children: ReactNode }) {
  return <span className="du-unbroken">{children}</span>;
}

/**
 * A timestamp drawn as unbroken pieces: the date with its time, then its time
 * zone. A line breaks only between them, or before the note.
 */
export function Moment({ moment, zone, note }: TimestampParts) {
  return (
    <>
      <Unbroken>{moment}</Unbroken>
      {zone !== undefined && (
        <>
          {" "}
          <Unbroken>{zone}</Unbroken>
        </>
      )}
      {note !== undefined && ` · ${note}`}
    </>
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
  id,
}: {
  children?: ReactNode;
  tone?: StatusTone;
  status?: StatusRow;
  live?: boolean;
  /** Lets another element name itself with this badge. */
  id?: string;
}) {
  const text = useLocaleText();
  const shape = status?.shape ?? STATUS_TONES[tone].shape;
  const Glyph = statusShapes[shape];
  const shown = status?.tone ?? tone;
  return (
    <span
      id={id}
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
  icon,
}: {
  children: ReactNode;
  tone?: "info" | "error";
  id?: string;
  announce?: boolean;
  /** Replaces the default notice icon. */
  icon?: IconName;
}) {
  let role: "alert" | "status" | undefined;
  if (announce) role = tone === "error" ? "alert" : "status";
  return (
    <div id={id} className={`du-notice du-notice--${tone}`} role={role}>
      <Icon name={icon ?? (tone === "error" ? "alert" : "activity")} />
      <div>{children}</div>
    </div>
  );
}

/**
 * An operation without a confirmed total is indeterminate and claims no
 * percentage. A confirmed total draws a determinate bar. A paused operation
 * keeps the bar's place without motion or a progress role.
 */
export function Progress({
  label,
  value,
  maxValue,
  paused = false,
}: {
  label: string;
  value?: number;
  maxValue?: number;
  paused?: boolean;
}) {
  if (paused)
    return (
      <div className="du-progress du-progress--paused" aria-hidden>
        <div className="du-progress__track" />
      </div>
    );
  const determinate =
    value !== undefined && maxValue !== undefined && maxValue > 0;
  if (determinate) {
    const ratio = Math.min(1, Math.max(0, value / maxValue));
    return (
      <ProgressBar
        aria-label={label}
        value={Math.min(value, maxValue)}
        maxValue={maxValue}
        className="du-progress du-progress--determinate"
      >
        <div className="du-progress__track" aria-hidden>
          <div
            className="du-progress__indicator"
            style={{ width: `${ratio * 100}%` }}
          />
        </div>
      </ProgressBar>
    );
  }
  return (
    <ProgressBar aria-label={label} isIndeterminate className="du-progress">
      <div className="du-progress__track" aria-hidden>
        <div className="du-progress__indicator" />
      </div>
    </ProgressBar>
  );
}

/**
 * Labelled values, one label and its value per line.
 *
 * The label keeps its width and never wraps. The value takes the rest of the
 * line and wraps inside it. Once less than 12 characters of width would remain
 * for a value that does not fit, the value moves below its label and starts at
 * the left. A value that fits stands at the end of the label's line.
 *
 * Prose values break between words. Mark a row `identifier` when its value is
 * a digest, commit, source or other identifier that may break between any two
 * characters. Pass a path as `<NameText path />`, which breaks after its
 * separators first.
 */
export function Readout({
  label,
  rows,
}: {
  label: string;
  rows: {
    label: string;
    /** Explains the label on pointer hover or keyboard focus (see `ExplainedText`). */
    explanation?: string;
    value: ReactNode;
    identifier?: boolean;
    emphasized?: boolean;
    /** Marks text that stands in for a value still to come; it takes the secondary text colour. */
    pending?: boolean;
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
              row.identifier && "du-readout__identifier",
              row.emphasized && "du-readout__emphasized",
              row.pending && "du-readout__pending",
            ]
              .filter(Boolean)
              .join(" ") || undefined
          }
        >
          <dt>
            {row.explanation ? (
              <ExplainedText explanation={row.explanation}>
                {row.label}
              </ExplainedText>
            ) : (
              row.label
            )}
          </dt>
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

/**
 * Groups a displayed value and its supporting evidence under one label.
 * `pending` marks text that stands in for a value still to come: it keeps
 * the place and size of a value and takes the secondary text colour.
 */
export function Metric({
  label,
  children,
  detail,
  prominent = false,
  pending = false,
}: {
  label: string;
  children: ReactNode;
  detail?: string | undefined;
  prominent?: boolean;
  pending?: boolean;
}) {
  return (
    <div
      className="du-metric"
      data-prominent={prominent || undefined}
      data-pending={pending || undefined}
      role="group"
      aria-label={label}
    >
      <p className="du-metric__label">{label}</p>
      <p className="du-metric__value">{children}</p>
      {detail && <p className="du-metric__detail">{detail}</p>}
    </div>
  );
}
