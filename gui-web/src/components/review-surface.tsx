import type { ReactNode } from "react";
import { Action } from "./controls";
import { Heading, Notice, Progress, Text } from "./presentation";
import "../theme/review-surface.css";

/** Display state only. The owning feature decides what its result means. */
export type SurfaceState =
  "ready" | "loading" | "empty" | "error" | "partial" | "stale" | "unavailable";

export function ContentState({
  state,
  message,
  children,
  recovery,
}: {
  state: SurfaceState;
  message: string;
  children: ReactNode;
  recovery?: { label: string; onPress: () => void };
}) {
  const showContent =
    state === "ready" || state === "partial" || state === "stale";
  return (
    <section
      className="du-content-state"
      aria-busy={state === "loading" || undefined}
    >
      {state === "loading" ? (
        <Progress label={message} />
      ) : (
        state !== "ready" && (
          <Notice tone={state === "error" ? "error" : "info"}>{message}</Notice>
        )
      )}
      {recovery && state !== "ready" && (
        <Action onPress={recovery.onPress}>{recovery.label}</Action>
      )}
      {showContent && children}
    </section>
  );
}

/** Shared reading hierarchy; slots carry feature-owned controls and values. */
export function ReviewSurface({
  title,
  description,
  context,
  tools,
  children,
  footer,
}: {
  title: string;
  description: string;
  context?: ReactNode;
  tools?: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
}) {
  return (
    <section className="du-review-surface" aria-label={title}>
      <header className="du-review-surface__header">
        <div>
          <Heading level={2}>{title}</Heading>
          <Text kind="muted">{description}</Text>
        </div>
        {tools && <div className="du-review-actions">{tools}</div>}
      </header>
      {context && <div className="du-review-context">{context}</div>}
      <div className="du-review-surface__body">{children}</div>
      {footer && <footer className="du-review-actions">{footer}</footer>}
    </section>
  );
}

export function ReviewSection({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="du-review-section" aria-label={title}>
      <Heading level={3}>{title}</Heading>
      {children}
    </section>
  );
}
