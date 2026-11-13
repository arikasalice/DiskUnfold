import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { useHostQuery } from "../src/state/use-host-query";

afterEach(cleanup);

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

test("keeps the last value across a failed reload", async () => {
  let calls = 0;
  const load = () =>
    ++calls === 1 ? Promise.resolve("first") : Promise.reject(new Error("x"));
  const { result } = renderHook(() => useHostQuery(load));
  await waitFor(() => expect(result.current.data).toBe("first"));
  act(() => result.current.reload());
  await waitFor(() => expect(result.current.failed).toBe(true));
  expect(result.current.data).toBe("first");
});

test("drops the earlier value when the question changes and ignores its late answer", async () => {
  const first = deferred<string>();
  const second = deferred<string>();
  const loadA = () => Promise.resolve("answer A");
  const loadB = () => second.promise;
  const loadC = () => first.promise;
  const { result, rerender } = renderHook(
    ({ load }: { load: () => Promise<string> }) => useHostQuery(load),
    { initialProps: { load: loadA } },
  );
  await waitFor(() => expect(result.current.data).toBe("answer A"));

  rerender({ load: loadC });
  expect(result.current.data).toBeUndefined();
  expect(result.current.loading).toBe(true);

  rerender({ load: loadB });
  await act(async () => first.resolve("late answer C"));
  expect(result.current.data).toBeUndefined();
  expect(result.current.loading).toBe(true);

  await act(async () => second.resolve("answer B"));
  expect(result.current.data).toBe("answer B");
  expect(result.current.loading).toBe(false);
});

test("a failure of the new question does not bring back the earlier value", async () => {
  const loadA = () => Promise.resolve("answer A");
  const loadB = () => Promise.reject(new Error("no"));
  const { result, rerender } = renderHook(
    ({ load }: { load: () => Promise<string> }) => useHostQuery(load),
    { initialProps: { load: loadA } },
  );
  await waitFor(() => expect(result.current.data).toBe("answer A"));
  rerender({ load: loadB });
  await waitFor(() => expect(result.current.failed).toBe(true));
  expect(result.current.data).toBeUndefined();
});
