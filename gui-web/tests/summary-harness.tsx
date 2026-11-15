import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { Profiler } from "react";
import { vi } from "vitest";
import { windowsPath } from "../src/ipc";
import type {
  AnalysisClient,
  VolumeGapRequest,
  VolumeGapResponse,
} from "../src/ipc";
import { SummaryHarness, volumeReport, watchSummary } from "./summary-fixture";

/*
 * The summary batch driven by a test: totals offered one version at a time
 * and volume reads answered or refused on demand, under fake timers.
 */

export const volumeSummary = () =>
  screen.queryByLabelText("Volume usage summary");

/** The values the volume part shows. */
export const shownPart = () => within(volumeSummary()!);

/** Volume reads that the test answers or refuses one at a time. */
function volumeClient() {
  const requests: { version: string; at: number; volume: boolean }[] = [];
  const pending: {
    version: string;
    requestSeq: number;
    resolve: (response: VolumeGapResponse) => void;
    reject: (failure: unknown) => void;
  }[] = [];
  const client = {
    call: ((name: string, body: never) => {
      if (name !== "space.volume_gap") throw new Error("Unexpected " + name);
      const request = body as VolumeGapRequest;
      requests.push({
        version: request.resultVersion,
        at: Date.now(),
        volume: request.target.volumeId != null,
      });
      return new Promise<VolumeGapResponse>((resolve, reject) =>
        pending.push({
          version: request.resultVersion,
          requestSeq: request.requestSeq,
          resolve,
          reject,
        }),
      );
    }) as AnalysisClient["call"],
    subscribe: () => () => {},
  } satisfies AnalysisClient;
  return { client, requests, pending };
}

/**
 * The summary batch with fake timers and a volume part read on demand, with
 * the explanation of the first root already open on "v1".
 */
export async function summaryRun(
  roots = [windowsPath("C:\\one"), windowsPath("C:\\two")],
) {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
  const volume = volumeClient();
  const watch = watchSummary(() => screen.queryByLabelText("Summary total"));
  const changes: number[] = [];
  let last: number | undefined;
  const onRender = () => {
    watch.onRender();
    const total = watch.seen.at(-1)?.[0];
    if (total !== last) changes.push(Date.now());
    last = total;
  };
  const view = render(
    <Profiler id="summary" onRender={onRender}>
      <SummaryHarness client={volume.client} version="v1" roots={roots} />
    </Profiler>,
  );
  /** Offers the totals of `version`; `live` is whether more will follow. */
  const show = (version: string, live = true) =>
    view.rerender(
      <Profiler id="summary" onRender={onRender}>
        <SummaryHarness
          client={volume.client}
          version={version}
          live={live}
          roots={roots}
        />
      </Profiler>,
    );
  const settle = async (ms = 0) => {
    await act(async () => {
      await vi.advanceTimersByTimeAsync(ms);
    });
  };
  const answer = async () => {
    const next = volume.pending.shift()!;
    await act(async () => {
      next.resolve({
        ...volumeReport(next.version),
        requestSeq: next.requestSeq,
      });
      await vi.advanceTimersByTimeAsync(0);
    });
  };
  const refuse = async (failure: unknown) => {
    await act(async () => {
      volume.pending.shift()!.reject(failure);
      await vi.advanceTimersByTimeAsync(0);
    });
  };
  const total = () => screen.getByLabelText("Summary total").textContent;
  const reads = (whole = false) =>
    volume.requests
      .filter(({ volume: compared }) => compared === whole)
      .map(({ version }) => version);
  fireEvent.click(screen.getByRole("button", { name: "Explain volume usage" }));
  await answer();
  return {
    ...volume,
    watch,
    changes,
    show,
    settle,
    answer,
    refuse,
    total,
    reads,
  };
}
