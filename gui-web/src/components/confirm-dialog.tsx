import type { ReactNode } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { Action } from "./controls";
import "../theme/inspection-dialog.css";
import "../theme/confirm-dialog.css";

/**
 * Asks the user to confirm one consequential choice.
 *
 * It is an alert dialog: the safe choice (`cancelLabel`) takes the initial focus,
 * Escape and an outside press both cancel, and `isPending` disables both buttons
 * while the confirmed action is in flight.
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
          <section className="du-inspection-dialog__body">{children}</section>
          <footer className="du-confirm-dialog__actions">
            <Action autoFocus isDisabled={isPending} onPress={onCancel}>
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
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
