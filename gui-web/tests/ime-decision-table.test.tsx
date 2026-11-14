import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Action, ConfirmDialog, PathField } from "../src/components";
import {
  IME_DECISION_TABLE,
  classifyImeKeyState,
  decideImeKey,
} from "../src/components/ime-composition";

afterEach(cleanup);

function press(
  target: Element,
  key: string,
  init: {
    isComposing?: boolean | undefined;
    keyCode?: number;
    repeat?: boolean;
    code?: string;
  },
) {
  const event = new KeyboardEvent("keydown", {
    key,
    code: init.code ?? "",
    repeat: init.repeat ?? false,
    bubbles: true,
    cancelable: true,
  });
  Object.defineProperty(event, "isComposing", { value: init.isComposing });
  Object.defineProperty(event, "keyCode", { value: init.keyCode ?? 0 });
  act(() => {
    target.dispatchEvent(event);
  });
  return event;
}

function renderEntry() {
  const screenEnter = vi.fn();
  const screenEscape = vi.fn();
  const onEnter = vi.fn();
  render(
    <div
      onKeyDown={(event) => {
        if (event.key === "Enter") screenEnter();
        if (event.key === "Escape") screenEscape();
      }}
    >
      <PathField
        label="Search"
        description="Type to search."
        value=""
        onChange={() => {}}
        isDisabled={false}
        onEnter={onEnter}
      />
    </div>,
  );
  return {
    input: screen.getByRole("textbox", { name: "Search" }),
    screenEnter,
    screenEscape,
    onEnter,
  };
}

test("the decision table is the seven F13 rows an input can own", () => {
  expect(IME_DECISION_TABLE.map((row) => row.id)).toEqual([
    "composing-enter",
    "composing-escape",
    "just-ended-enter",
    "just-ended-escape",
    "unknown-enter",
    "unknown-escape",
    "approval-enter",
    "idle-enter-run",
  ]);
  expect(classifyImeKeyState("idle", { isComposing: true })).toBe("composing");
  expect(classifyImeKeyState("idle", { keyCode: 229 })).toBe("composing");
  expect(classifyImeKeyState("idle", { key: "Process" })).toBe("composing");
  expect(classifyImeKeyState("just-ended", { isComposing: false })).toBe(
    "just-ended",
  );
  expect(classifyImeKeyState("idle", {})).toBe("unknown");
  expect(classifyImeKeyState("idle", { isComposing: false })).toBe("idle");
  expect(decideImeKey("idle", "ArrowDown", "run").outcome).toBe("pass");
});

test("row 1: Enter during composition never runs and is left to the IME", () => {
  const { input, screenEnter, onEnter } = renderEntry();
  fireEvent.compositionStart(input);
  const event = press(input, "Enter", { isComposing: true, keyCode: 229 });
  expect(onEnter).not.toHaveBeenCalled();
  expect(screenEnter).not.toHaveBeenCalled();
  expect(event.defaultPrevented).toBe(false);
});

test("row 2: Escape during composition only cancels the composition", () => {
  const { input, screenEscape } = renderEntry();
  fireEvent.compositionStart(input);
  const event = press(input, "Escape", { isComposing: true, keyCode: 229 });
  expect(screenEscape).not.toHaveBeenCalled();
  expect(event.defaultPrevented).toBe(false);
  fireEvent.compositionEnd(input);
  fireEvent.keyUp(input, { key: "Escape" });
  press(input, "Escape", { isComposing: false, keyCode: 27 });
  expect(screenEscape).toHaveBeenCalledTimes(1);
});

test("row 3: the Enter that ends a composition is ignored", () => {
  const { input, screenEnter, onEnter } = renderEntry();
  fireEvent.compositionStart(input);
  fireEvent.compositionEnd(input);
  const event = press(input, "Enter", { isComposing: false, keyCode: 13 });
  expect(onEnter).not.toHaveBeenCalled();
  expect(screenEnter).not.toHaveBeenCalled();
  expect(event.defaultPrevented).toBe(true);
});

test("row 3: the mark stays until the key release, however late the Enter comes", async () => {
  const { input, onEnter } = renderEntry();
  fireEvent.compositionEnd(input);
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 50));
  });
  const event = press(input, "Enter", { isComposing: false, keyCode: 13 });
  expect(event.defaultPrevented).toBe(true);
  expect(onEnter).not.toHaveBeenCalled();
});

test("row 3: the just-ended mark clears on the next key release", () => {
  const { input, onEnter } = renderEntry();
  fireEvent.compositionEnd(input);
  fireEvent.keyUp(input, { key: "Enter" });
  press(input, "Enter", { isComposing: false, keyCode: 13 });
  expect(onEnter).toHaveBeenCalledTimes(1);
});

test("row 4: an Enter whose composition state cannot be judged does not run", () => {
  const { input, screenEnter, onEnter } = renderEntry();
  const event = press(input, "Enter", { isComposing: undefined, keyCode: 0 });
  expect(onEnter).not.toHaveBeenCalled();
  expect(screenEnter).not.toHaveBeenCalled();
  expect(event.defaultPrevented).toBe(true);
});

test("row 5: an Enter outside composition runs the search once", () => {
  const { input, onEnter } = renderEntry();
  press(input, "Enter", { isComposing: false, keyCode: 13 });
  expect(onEnter).toHaveBeenCalledTimes(1);
  press(input, "Enter", { isComposing: false, keyCode: 13, repeat: true });
  expect(onEnter).toHaveBeenCalledTimes(1);
});

test("row 6: Enter in an approval entry submits nothing", () => {
  const onConfirm = vi.fn();
  const onEnter = vi.fn();
  render(
    <ConfirmDialog
      title="Approve"
      confirmLabel="Approve now"
      cancelLabel="Cancel"
      onConfirm={onConfirm}
      onCancel={() => {}}
    >
      <PathField
        label="Reason"
        description="Why."
        value=""
        onChange={() => {}}
        isDisabled={false}
        onEnter={onEnter}
      />
    </ConfirmDialog>,
  );
  const input = screen.getByRole("textbox", { name: "Reason" });
  const event = press(input, "Enter", { isComposing: false, keyCode: 13 });
  expect(event.defaultPrevented).toBe(true);
  expect(onEnter).not.toHaveBeenCalled();
  expect(onConfirm).not.toHaveBeenCalled();
});

test("row 7: a button runs once for Space, Enter and a click", () => {
  const onPress = vi.fn();
  render(<Action onPress={onPress}>Run</Action>);
  const button = screen.getByRole("button", { name: "Run" });
  fireEvent.keyDown(button, { key: "Enter" });
  fireEvent.keyUp(button, { key: "Enter" });
  expect(onPress).toHaveBeenCalledTimes(1);
  fireEvent.keyDown(button, { key: " " });
  fireEvent.keyUp(button, { key: " " });
  expect(onPress).toHaveBeenCalledTimes(2);
  fireEvent.click(button);
  expect(onPress).toHaveBeenCalledTimes(3);
});

/**
 * One Enter press that ends a composition, as each web view delivers it. Every
 * order must run `onEnter` zero times and leave the next, separate Enter alone.
 */
const delays = [0, 25, 300];

async function nextTask(delay: number) {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, delay));
  });
}

function release(input: Element, key = "Enter", code = "Enter") {
  fireEvent.keyUp(input, { key, code });
}

function expectNextEnterRuns(input: Element, onEnter: () => void) {
  const event = press(input, "Enter", {
    isComposing: false,
    keyCode: 13,
    code: "Enter",
  });
  expect(event.defaultPrevented).toBe(false);
  release(input);
  expect(onEnter).toHaveBeenCalledTimes(1);
}

test("WebView2 order: keydown 229, compositionend, keyup", () => {
  const { input, screenEnter, onEnter } = renderEntry();
  fireEvent.compositionStart(input);
  const down = press(input, "Enter", {
    isComposing: true,
    keyCode: 229,
    code: "Enter",
  });
  fireEvent.compositionEnd(input);
  release(input);
  expect(down.defaultPrevented).toBe(false);
  expect(onEnter).not.toHaveBeenCalled();
  expect(screenEnter).not.toHaveBeenCalled();
  expectNextEnterRuns(input, onEnter);
});

test.each(delays)(
  "WebKitGTK order: compositionend, a later task (%i ms), keydown 13 with isComposing false, keyup",
  async (delay) => {
    const { input, screenEnter, onEnter } = renderEntry();
    fireEvent.compositionStart(input);
    fireEvent.compositionEnd(input);
    await nextTask(delay);
    const down = press(input, "Enter", {
      isComposing: false,
      keyCode: 13,
      code: "Enter",
    });
    expect(down.defaultPrevented).toBe(true);
    release(input);
    expect(onEnter).not.toHaveBeenCalled();
    expect(screenEnter).not.toHaveBeenCalled();
    expectNextEnterRuns(input, onEnter);
  },
);

test("WebKitGTK order with a held Enter: repeats stay consumed until the release", async () => {
  const { input, onEnter } = renderEntry();
  fireEvent.compositionEnd(input);
  await nextTask(25);
  press(input, "Enter", { isComposing: false, keyCode: 13, code: "Enter" });
  const repeat = press(input, "Enter", {
    isComposing: false,
    keyCode: 13,
    code: "Enter",
    repeat: true,
  });
  expect(repeat.defaultPrevented).toBe(true);
  release(input);
  expect(onEnter).not.toHaveBeenCalled();
  expectNextEnterRuns(input, onEnter);
});

test("WKWebView order: keydown with isComposing, compositionend, keyup", () => {
  const { input, screenEnter, onEnter } = renderEntry();
  fireEvent.compositionStart(input);
  const down = press(input, "Enter", {
    isComposing: true,
    keyCode: 13,
    code: "Enter",
  });
  fireEvent.compositionEnd(input);
  release(input);
  expect(down.defaultPrevented).toBe(false);
  expect(onEnter).not.toHaveBeenCalled();
  expect(screenEnter).not.toHaveBeenCalled();
  expectNextEnterRuns(input, onEnter);
});

test("a mark never outlives its Enter: a lost release, another key, blur or a new composition end it", async () => {
  const lostRelease = renderEntry();
  fireEvent.compositionEnd(lostRelease.input);
  press(lostRelease.input, "Enter", {
    isComposing: false,
    keyCode: 13,
    code: "Enter",
  });
  // No keyup arrives; the next, separate Enter press is a new press.
  const second = press(lostRelease.input, "Enter", {
    isComposing: false,
    keyCode: 13,
    code: "Enter",
  });
  expect(second.defaultPrevented).toBe(false);
  expect(lostRelease.onEnter).toHaveBeenCalledTimes(1);
  cleanup();

  const otherKey = renderEntry();
  fireEvent.compositionEnd(otherKey.input);
  press(otherKey.input, "a", { isComposing: false, keyCode: 65, code: "KeyA" });
  expectNextEnterRuns(otherKey.input, otherKey.onEnter);
  cleanup();

  const blurred = renderEntry();
  fireEvent.compositionEnd(blurred.input);
  await nextTask(10);
  fireEvent.blur(blurred.input);
  expectNextEnterRuns(blurred.input, blurred.onEnter);
});

test("a modifier key or the release of another key does not end the mark early", () => {
  const { input, onEnter } = renderEntry();
  fireEvent.compositionEnd(input);
  press(input, "Shift", { isComposing: false, keyCode: 16, code: "ShiftLeft" });
  release(input, "Shift", "ShiftLeft");
  const down = press(input, "Enter", {
    isComposing: false,
    keyCode: 13,
    code: "Enter",
  });
  expect(down.defaultPrevented).toBe(true);
  expect(onEnter).not.toHaveBeenCalled();
});

test("Escape that follows a composition is consumed once", () => {
  const { input, screenEscape } = renderEntry();
  fireEvent.compositionEnd(input);
  press(input, "Escape", { isComposing: false, keyCode: 27, code: "Escape" });
  expect(screenEscape).not.toHaveBeenCalled();
  press(input, "Escape", { isComposing: false, keyCode: 27, code: "Escape" });
  expect(screenEscape).toHaveBeenCalledTimes(1);
});
