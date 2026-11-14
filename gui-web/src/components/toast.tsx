import { useEffect, useState } from "react";
import "../theme/toast.css";

export const TOAST_DURATION_MS = 4_000;

/** One short notice; a new `id` shows it again even with the same text. */
export type ToastMessage = { id: number; text: string };

/**
 * A short notice shown once for four seconds.
 *
 * The status region stays mounted so assistive technology announces each new
 * message once. The notice does not take focus or move the layout.
 */
export function Toast({ message }: { message: ToastMessage | undefined }) {
  const [visible, setVisible] = useState<ToastMessage>();
  const id = message?.id;
  const content = message?.text;
  useEffect(() => {
    if (id === undefined || content === undefined) {
      setVisible(undefined);
      return;
    }
    setVisible({ id, text: content });
    const timer = window.setTimeout(
      () => setVisible(undefined),
      TOAST_DURATION_MS,
    );
    return () => window.clearTimeout(timer);
  }, [id, content]);
  return (
    <div className="du-toast-region" role="status">
      {visible && (
        <div className="du-toast" key={visible.id}>
          <span className="du-toast__mark" aria-hidden>
            ✓
          </span>
          <span>{visible.text}</span>
        </div>
      )}
    </div>
  );
}
