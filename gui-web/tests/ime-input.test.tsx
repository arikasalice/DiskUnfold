import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Action, PathField, ReviewField } from "../src/components";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

type TextFieldKind = "path" | "review";

function makeField(
  kind: TextFieldKind,
  onCompositionChange: (composing: boolean) => void,
) {
  if (kind === "path")
    return (
      <PathField
        label="Text entry"
        description="Enter text."
        value=""
        onChange={() => {}}
        isDisabled={false}
        onCompositionChange={onCompositionChange}
      />
    );

  return (
    <ReviewField
      label="Text entry"
      description="Enter text."
      value=""
      onChange={() => {}}
    />
  );
}

function keyboardEvent(
  key: string,
  isComposing: boolean | undefined,
  keyCode: number,
) {
  const event = new KeyboardEvent("keydown", {
    key,
    bubbles: true,
    cancelable: true,
  });
  Object.defineProperty(event, "isComposing", { value: isComposing });
  Object.defineProperty(event, "keyCode", { value: keyCode });
  return event;
}

function renderInput(kind: TextFieldKind) {
  const onAction = vi.fn();
  const onEscape = vi.fn();
  const onClick = vi.fn();
  const onCompositionChange = vi.fn();
  render(
    <div
      onKeyDown={(event) => {
        if (event.defaultPrevented) return;
        if (event.key === "Enter") onAction();
        if (event.key === "Escape") onEscape();
      }}
    >
      {makeField(kind, onCompositionChange)}
      <Action onPress={onClick}>Click action</Action>
    </div>,
  );

  return {
    input: screen.getByRole("textbox", { name: "Text entry" }),
    onAction,
    onEscape,
    onClick,
    onCompositionChange,
  };
}

test.each(["path", "review"] as const)(
  "%s input keeps composing keys with the IME and suppresses its completion Enter",
  (kind) => {
    const { input, onAction, onEscape, onClick, onCompositionChange } =
      renderInput(kind);

    fireEvent.compositionStart(input);
    const composingEnter = keyboardEvent("Enter", true, 229);
    let composingDispatch = true;
    act(() => {
      composingDispatch = input.dispatchEvent(composingEnter);
    });
    expect(composingDispatch).toBe(true);
    expect(composingEnter.defaultPrevented).toBe(false);
    expect(onAction).not.toHaveBeenCalled();
    if (kind === "path") {
      expect(onCompositionChange).toHaveBeenLastCalledWith(true);
    }

    const composingEscape = keyboardEvent("Escape", true, 229);
    let escapeDispatch = false;
    act(() => {
      escapeDispatch = input.dispatchEvent(composingEscape);
    });
    expect(escapeDispatch).toBe(true);
    expect(composingEscape.defaultPrevented).toBe(false);
    expect(onEscape).not.toHaveBeenCalled();

    fireEvent.compositionEnd(input);
    if (kind === "path")
      expect(onCompositionChange).toHaveBeenLastCalledWith(false);

    const completionEnter = keyboardEvent("Enter", false, 13);
    let completionDispatch = true;
    act(() => {
      completionDispatch = input.dispatchEvent(completionEnter);
    });
    expect(completionDispatch).toBe(false);
    expect(completionEnter.defaultPrevented).toBe(true);
    expect(onAction).not.toHaveBeenCalled();

    fireEvent.keyUp(input, { key: "Enter", code: "Enter" });
    const nextEnter = keyboardEvent("Enter", false, 13);
    let nextDispatch = false;
    act(() => {
      nextDispatch = input.dispatchEvent(nextEnter);
    });
    expect(nextDispatch).toBe(true);
    expect(onAction).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "Click action" }));
    expect(onClick).toHaveBeenCalledTimes(1);

    const unknownEnter = keyboardEvent("Enter", undefined, 0);
    let unknownDispatch = true;
    act(() => {
      unknownDispatch = input.dispatchEvent(unknownEnter);
    });
    expect(unknownDispatch).toBe(false);
    expect(unknownEnter.defaultPrevented).toBe(true);
    expect(onAction).toHaveBeenCalledTimes(1);
  },
);

test.each(["path", "review"] as const)(
  "%s input treats composition updates as active composition",
  (kind) => {
    const { input, onAction, onCompositionChange } = renderInput(kind);
    fireEvent.compositionUpdate(input);
    if (kind === "path") {
      expect(onCompositionChange).toHaveBeenLastCalledWith(true);
    }

    const event = keyboardEvent("Enter", false, 13);
    let dispatch = false;
    act(() => {
      dispatch = input.dispatchEvent(event);
    });

    expect(dispatch).toBe(true);
    expect(event.defaultPrevented).toBe(false);
    expect(onAction).not.toHaveBeenCalled();
  },
);

test.each(["path", "review"] as const)(
  "%s input treats keyCode 229 as composing even without the composition flag",
  (kind) => {
    const { input, onAction } = renderInput(kind);
    const event = keyboardEvent("Enter", false, 229);
    let dispatch = false;
    act(() => {
      dispatch = input.dispatchEvent(event);
    });

    expect(dispatch).toBe(true);
    expect(event.defaultPrevented).toBe(false);
    expect(onAction).not.toHaveBeenCalled();
  },
);

test("keeps the just-ended guard across tasks and drops it with the first Enter's release", async () => {
  const { input, onAction } = renderInput("path");
  fireEvent.compositionEnd(input);

  await act(async () => {
    await new Promise((resolve) => window.setTimeout(resolve, 50));
  });

  const late = keyboardEvent("Enter", false, 13);
  act(() => {
    input.dispatchEvent(late);
  });
  expect(late.defaultPrevented).toBe(true);
  fireEvent.keyUp(input, { key: "Enter", code: "Enter" });

  const next = keyboardEvent("Enter", false, 13);
  act(() => {
    input.dispatchEvent(next);
  });
  expect(next.defaultPrevented).toBe(false);
  expect(onAction).toHaveBeenCalledTimes(1);
});
