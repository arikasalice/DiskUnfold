/** The five status tones a badge can use. */
export type StatusTone = "neutral" | "info" | "good" | "warning" | "danger";

/** The glyph that carries a status without colour. */
export type StatusShape = "dot" | "info" | "check" | "warning" | "cross";

/**
 * One status presentation: a shape, a message key and a colour token.
 *
 * All three are required, so a status is never conveyed by colour alone.
 */
export type StatusRow = Readonly<{
  id: string;
  tone: StatusTone;
  shape: StatusShape;
  messageKey: string;
  colorToken: string;
}>;

/** The shape and colour variable of each tone. Colour variables live in `theme/tokens.css`. */
export const STATUS_TONES: Readonly<
  Record<StatusTone, Readonly<{ shape: StatusShape; colorVar: string }>>
> = {
  neutral: { shape: "dot", colorVar: "--du-muted" },
  info: { shape: "info", colorVar: "--du-info-text" },
  good: { shape: "check", colorVar: "--du-good-text" },
  warning: { shape: "warning", colorVar: "--du-warning-text" },
  danger: { shape: "cross", colorVar: "--du-danger-text" },
};

/**
 * Builds a status row from a tone and the message key that names the status.
 *
 * Throws `RangeError` when the id or message key is empty, so a status
 * without a name fails where it is declared.
 */
export function defineStatusRow(
  row: Readonly<{ id: string; tone: StatusTone; messageKey: string }>,
): StatusRow {
  if (!row.id || !row.messageKey)
    throw new RangeError(
      `A status row needs an id and a message key: ${row.id}`,
    );
  const { shape, colorVar } = STATUS_TONES[row.tone];
  return {
    id: row.id,
    tone: row.tone,
    shape,
    messageKey: row.messageKey,
    colorToken: colorVar,
  };
}
