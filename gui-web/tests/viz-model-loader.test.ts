import { expect, test, vi } from "vitest";
import type { Mock } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { VizModelLoader } from "../src/viz";
import type { VizLoadState } from "../src/viz";
import type {
  AnalysisJobStatus,
  VizRenderRequest,
  VizRenderResponse,
} from "../src/ipc";

const revision = modelFixture().frame.resultRevision;
const request = (seq: number, channel = "view"): VizRenderRequest => ({
  viewChannel: channel,
  requestSeq: seq,
  resultRevision: revision,
  format: "treemap",
  aggregateScopeRef: { resultVersion: revision, itemNo: 0 },
  navigationScopeRef: { resultVersion: revision, itemNo: 0 },
  measure: "size",
  sizeBasis: "logical",
  classificationCriterion: "type",
  colorCriterion: "type",
  denominatorBasis: "scope",
  minAreaPx2: 1,
  depthLimit: 2,
  viewport: { width: 960, height: 330, scale: 1 },
  visibleWindow: { left: 0, top: 0, right: 1000000, bottom: 1000000 },
});
function model(seq: number) {
  const frame = modelFixture().frame;
  frame.requestSeq = seq;
  return frame;
}
function job(seq: number, jobRef = "job-1"): VizRenderResponse {
  const { observedAt, completeness } = modelFixture().frame;
  return {
    kind: "job",
    requestSeq: seq,
    resultRevision: revision,
    observedAt,
    completeness,
    jobRef,
  };
}
const status = (state: string, jobRef = "job-1") =>
  ({ jobRef, kind: "viz.compute", state }) as AnalysisJobStatus;
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => {
    resolve = yes;
  });
  return { promise, resolve };
}

type Extra = { owner: string };
type RenderMock = Mock<
  (request: VizRenderRequest) => Promise<VizRenderResponse>
>;
const renderMock = (): RenderMock =>
  vi.fn<(request: VizRenderRequest) => Promise<VizRenderResponse>>();
function setup(render: RenderMock) {
  const cancel = vi.fn().mockResolvedValue({});
  const publish =
    vi.fn<(patch: Partial<VizLoadState>, extra?: Extra) => void>();
  const admit = vi.fn();
  const commit = vi.fn(
    (_response: unknown, _request: unknown, evidence: string): Extra => ({
      owner: evidence,
    }),
  );
  const loader = new VizModelLoader<string, Extra>(
    { render, cancel },
    { admit, commit, publish },
    "none",
  );
  return { loader, cancel, publish, admit, commit };
}

test("rejects a request that is not pinned, not newer or on another channel without changing state", async () => {
  const render = renderMock().mockResolvedValue(model(1));
  const { loader, publish, admit } = setup(render);
  await loader.render(request(1), "a");
  publish.mockClear();
  admit.mockClear();
  await expect(loader.render(request(1), "a")).rejects.toThrow(/increasing/);
  await expect(loader.render(request(0), "a")).rejects.toThrow(/increasing/);
  await expect(
    loader.render({ ...request(2), resultRevision: "" }, "a"),
  ).rejects.toThrow(/pinned revision/);
  await expect(loader.render(request(2, "other"), "a")).rejects.toThrow(
    /one view channel/,
  );
  expect(render).toHaveBeenCalledTimes(1);
  expect(publish).not.toHaveBeenCalled();
  expect(admit).not.toHaveBeenCalled();
  expect(loader.nextRequestSequence()).toBe(2);
});

test("lets the owner reject a request before the loader binds it", async () => {
  const render = renderMock().mockResolvedValue(model(1));
  const { loader, publish, admit } = setup(render);
  admit.mockImplementationOnce(() => {
    throw new Error("owner says no");
  });
  await expect(loader.render(request(1, "view"), "a")).rejects.toThrow(
    "owner says no",
  );
  expect(publish).not.toHaveBeenCalled();
  expect(loader.nextRequestSequence()).toBe(1);
  await loader.render(request(1, "another"), "a");
  expect(render).toHaveBeenCalledTimes(1);
});

test("discards the answer of a request that a newer one has replaced", async () => {
  const first = deferred<VizRenderResponse>();
  const render = renderMock()
    .mockReturnValueOnce(first.promise)
    .mockResolvedValueOnce(model(2));
  const { loader, publish, commit } = setup(render);
  const late = loader.render(request(1), "a");
  await loader.render(request(2), "b");
  first.resolve(model(1));
  await late;
  expect(commit).toHaveBeenCalledTimes(1);
  expect(commit).toHaveBeenCalledWith(expect.anything(), request(2), "b");
  expect(publish).toHaveBeenLastCalledWith(
    expect.objectContaining({ phase: "model" }),
    { owner: "b" },
  );
});

test("publishes an accepted model and the owner's part as one update", async () => {
  const render = renderMock().mockResolvedValue(model(1));
  const { loader, publish } = setup(render);
  await loader.render(request(1), "a");
  expect(publish.mock.calls).toEqual([
    [{ phase: "loading", response: null, error: null }, undefined],
    [{ phase: "model", job: null, response: model(1) }, { owner: "a" }],
  ]);
});

test("turns a mismatched or inconsistent answer into an error without committing", async () => {
  const wrongSeq = model(9);
  const inconsistent = model(2);
  inconsistent.model.aggregateScopeRef = { resultVersion: revision, itemNo: 7 };
  const render = renderMock()
    .mockResolvedValueOnce(wrongSeq)
    .mockResolvedValueOnce(inconsistent);
  const { loader, publish, commit } = setup(render);
  await loader.render(request(1), "a");
  expect(publish).toHaveBeenLastCalledWith(
    expect.objectContaining({ phase: "error" }),
    undefined,
  );
  await loader.render(request(2), "a");
  expect(publish).toHaveBeenLastCalledWith(
    expect.objectContaining({ phase: "error" }),
    undefined,
  );
  expect(commit).not.toHaveBeenCalled();
});

test("renders again with the next sequence and the same evidence when its job completes", async () => {
  const render = renderMock()
    .mockResolvedValueOnce(job(1))
    .mockResolvedValueOnce(model(2));
  const { loader, commit } = setup(render);
  await loader.render(request(1), "kept");
  await loader.completeJob(status("running"));
  expect(render).toHaveBeenCalledTimes(1);
  await loader.completeJob(status("done", "other-job"));
  expect(render).toHaveBeenCalledTimes(1);
  await loader.completeJob(status("done"));
  expect(render).toHaveBeenCalledTimes(2);
  expect(render.mock.calls[1]![0]).toMatchObject({ requestSeq: 2 });
  expect(commit).toHaveBeenCalledWith(
    expect.anything(),
    expect.anything(),
    "kept",
  );
  await loader.completeJob(status("done"));
  expect(render).toHaveBeenCalledTimes(2);
});

test.each([
  ["cancelled", "cancelled"],
  ["failed", "error"],
  ["partial", "error"],
])("a %s job ends in the %s phase and drops the job", async (state, phase) => {
  const render = renderMock().mockResolvedValueOnce(job(1));
  const { loader, publish } = setup(render);
  await loader.render(request(1), "a");
  await loader.completeJob(status(state));
  expect(publish).toHaveBeenLastCalledWith(
    expect.objectContaining({ phase, job: null }),
    undefined,
  );
});

test("retry repeats the failed request with the next sequence only after an error", async () => {
  const render = renderMock()
    .mockRejectedValueOnce(new Error("transport"))
    .mockResolvedValueOnce(model(2));
  const { loader, commit } = setup(render);
  await loader.retry();
  expect(render).not.toHaveBeenCalled();
  await loader.render(request(1), "again");
  await loader.retry();
  expect(render).toHaveBeenCalledTimes(2);
  expect(render.mock.calls[1]![0]).toMatchObject({ requestSeq: 2 });
  expect(commit).toHaveBeenCalledWith(
    expect.anything(),
    expect.anything(),
    "again",
  );
  await loader.retry();
  expect(render).toHaveBeenCalledTimes(2);
});

test("cancels the request that is loading, then the accepted job, on the bound channel", async () => {
  const pending = deferred<VizRenderResponse>();
  const render = renderMock()
    .mockReturnValueOnce(pending.promise)
    .mockResolvedValueOnce(job(2));
  const { loader, cancel } = setup(render);
  await loader.cancelCurrent();
  expect(cancel).not.toHaveBeenCalled();
  const first = loader.render(request(1), "a");
  await loader.cancelCurrent();
  expect(cancel).toHaveBeenLastCalledWith({
    target: { kind: "subscription", viewChannel: "view", requestSeq: 1 },
  });
  pending.resolve(model(1));
  await first;
  await loader.render(request(2), "a");
  await loader.cancelCurrent();
  expect(cancel).toHaveBeenLastCalledWith({
    target: { kind: "subscription", viewChannel: "view", requestSeq: 2 },
  });
});

test("refuses a further sequence once the sequence space is used up", async () => {
  const render = renderMock().mockResolvedValue(model(0xffff_ffff));
  const { loader } = setup(render);
  await loader.render(request(0xffff_ffff), "a");
  expect(() => loader.nextRequestSequence()).toThrow(/exhausted/);
});
