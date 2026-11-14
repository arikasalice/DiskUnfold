import { useEffect, useRef } from "react";
import type { ReactNode } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { Action } from "./controls";
import { EnterPolicyProvider } from "./ime-composition";
import "../theme/inspection-dialog.css";
import "../theme/confirm-dialog.css";

const textEntry =
  "input:not([type=checkbox], [type=radio], [type=button], [type=submit], [type=reset])";

/**
 * Asks the user to confirm one consequential choice.
 *
 * It is an alert dialog: the safe choice (`cancelLabel`) takes the initial focus,
 * Escape and an outside press both cancel, and `isPending` disables both buttons
 * while the confirmed action is in flight.
 *
 * Enter in a text entry inside the dialog never submits or confirms. Only a
 * press on the confirm button (Space, Enter or a pointer) requests the action.
 */
export function ConfirmDialog({
  title,
  children,
  confirmLabel,
  cancelLabel,
  onConfirm,
  onCancel,
  isPending = false,
}: {
  title: string;
  children: ReactNode;
  confirmLabel: string;
  cancelLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
  isPending?: boolean;
}) {
  const cancel = useRef<HTMLButtonElement>(null);
  // The dialog moves focus to itself when nothing inside it has focus; the
  // safe choice claims it afterwards so the default never depends on that order.
  useEffect(() => {
    cancel.current?.focus();
  }, []);
  return (
    <ModalOverlay
      className="du-inspection-overlay"
      isOpen
      onOpenChange={(open) => {
        if (!open && !isPending) onCancel();
      }}
    >
      <Modal className="du-inspection-modal du-confirm-modal">
        <Dialog role="alertdialog" className="du-inspection-dialog">
          <header className="du-inspection-dialog__header">
            <Heading slot="title" className="du-heading du-heading--2">
              {title}
            </Heading>
          </header>
          <div
            className="du-confirm-dialog__content"
            onKeyDown={(event) => {
              if (event.key === "Enter" && event.target instanceof Element) {
                if (event.target.matches(textEntry)) event.preventDefault();
              }
            }}
          >
            <EnterPolicyProvider value="never">
              <section className="du-inspection-dialog__body">
                {children}
              </section>
            </EnterPolicyProvider>
            <footer className="du-confirm-dialog__actions">
              <Action
                ref={cancel}
                autoFocus
                isDisabled={isPending}
                onPress={onCancel}
              >
                {cancelLabel}
              </Action>
              <Action
                variant="primary"
                isDisabled={isPending}
                onPress={onConfirm}
              >
                {confirmLabel}
              </Action>
            </footer>
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
