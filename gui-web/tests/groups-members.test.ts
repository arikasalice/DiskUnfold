import { expect, test, vi } from "vitest";
import { AnalysisError } from "../src/ipc";
import { GroupMembersReader, memberRowKey } from "../src/state/groups-members";
import type { MembersView } from "../src/state/groups-members";
import {
  ARCHIVE,
  CLASSIFICATION,
  MOVIES,
  RESULT,
  deferred,
  fakeClient,
  flush,
  memberRow,
} from "./groups-fixtures";
import type { Body } from "./groups-fixtures";

const view = (overrides: Partial<MembersView> = {}): MembersView => ({
  classificationRef: CLASSIFICATION,
  resultVersion: RESULT,
  category: { kind: "group", groupId: MOVIES },
  count: 1000,
  ...overrides,
});

function answer(body: Body, total = 1000) {
  return {
    classificationRef: body.classificationRef,
    resultVersion: body.resultVersion,
    start: body.start,
    total,
    rows: Array.from(
      { length: Math.min(body.count, total - body.start) },
      (_, i) => memberRow(body.start + i + 1),
    ),
    sort: { id: "size", revision: 1 },
  };
}

function setup(handler: (body: Body) => unknown = (body) => answer(body)) {
  const fake = fakeClient({ "group.members": handler });
  const unreadable = vi.fn(() => true);
  const reader = new GroupMembersReader(fake.client, unreadable);
  return { fake, reader, unreadable };
}

const bodies = (fake: ReturnType<typeof fakeClient>) =>
  fake.calls
    .filter((call) => call.name === "group.members")
    .map((call) => call.body);

test("F36 FR6: only the visible rows and one screen around them are requested, in bounded chunks", async () => {
  const { fake, reader } = setup();
  reader.setView(view());
  reader.show(0, 10);
  await flush();
  expect(bodies(fake)).toEqual([
    {
      classificationRef: CLASSIFICATION,
      resultVersion: RESULT,
      category: { kind: "group", groupId: MOVIES },
      start: 0,
      count: 100,
    },
  ]);
  // Rows around a later window; far chunks are never requested.
  reader.show(495, 505);
  await flush();
  expect(bodies(fake).map((body) => body.start)).toEqual([0, 400, 500]);
  expect(bodies(fake).every((body) => body.count <= 1000)).toBe(true);
  expect(reader.row(0)?.item.itemNo).toBe(1);
  expect(reader.row(450)?.item.itemNo).toBe(451);
  expect(reader.row(300)).toBeUndefined();
  expect(reader.getSnapshot().total).toBe(1000);
});

test("F36 FR6: a cached chunk is not requested again and rows are keyed by item reference", async () => {
  const { fake, reader } = setup();
  reader.setView(view());
  reader.show(0, 10);
  await flush();
  reader.show(5, 15);
  reader.show(0, 10);
  await flush();
  expect(fake.count("group.members")).toBe(1);
  expect(memberRowKey(reader.row(2)!.item)).toBe(`${RESULT}:3`);
});

test("F36 FR6: a category, sort or classification change empties the cache and drops late answers", async () => {
  const gate = deferred<unknown>();
  let first = true;
  const { fake, reader } = setup((body) => {
    if (first) {
      first = false;
      return gate.promise;
    }
    return answer(body);
  });
  reader.setView(view());
  reader.show(0, 10);
  await flush();
  reader.setView(view({ category: { kind: "group", groupId: ARCHIVE } }));
  await flush();
  expect(reader.row(0)?.item.itemNo).toBe(1);
  // The first request belonged to the earlier category; its answer is ignored.
  gate.resolve(
    answer({
      classificationRef: CLASSIFICATION,
      resultVersion: RESULT,
      start: 0,
      count: 100,
    }),
  );
  await flush();
  expect(fake.count("group.members")).toBe(2);
  expect(bodies(fake)[1]!.category).toEqual({
    kind: "group",
    groupId: ARCHIVE,
  });

  reader.setView(
    view({
      category: { kind: "group", groupId: ARCHIVE },
      sort: { id: "name", revision: 1, descending: false },
    }),
  );
  expect(reader.row(0)).toBeUndefined();
  await flush();
  expect(bodies(fake)[2]!.sort).toEqual({
    id: "name",
    revision: 1,
    descending: false,
  });

  reader.setView(
    view({ classificationRef: "other", category: { kind: "overlap" } }),
  );
  expect(reader.row(0)).toBeUndefined();
  await flush();
  expect(bodies(fake)[3]).toMatchObject({
    classificationRef: "other",
    category: { kind: "overlap" },
  });
});

test("F36 FR6: the same view is not read again", async () => {
  const { fake, reader } = setup();
  reader.setView(view());
  reader.show(0, 10);
  await flush();
  reader.setView(view());
  await flush();
  expect(fake.count("group.members")).toBe(1);
});

test("F36 FR6: an unreadable classification is reported; other failures can be retried", async () => {
  let reason: [string, string] = ["NOT_FOUND", "group.result_expired"];
  const { fake, reader, unreadable } = setup(() => {
    throw new AnalysisError(reason[0], reason[1] as never, false);
  });
  reader.setView(view());
  reader.show(0, 10);
  await flush();
  expect(unreadable).toHaveBeenCalledOnce();
  expect(reader.getSnapshot().failed).toBeUndefined();

  reason = ["BUSY", "db.busy"];
  reader.setView(view({ count: 999 }));
  reader.show(0, 11);
  await flush();
  expect(reader.getSnapshot().failed).toBeDefined();
  const before = fake.count("group.members");
  reader.show(0, 12);
  await flush();
  expect(fake.count("group.members")).toBe(before);
  reason = ["BUSY", "db.busy"];
  reader.retry();
  await flush();
  expect(fake.count("group.members")).toBe(before + 1);
});

test("F36 FR6: an answer for another classification or position is refused", async () => {
  const { reader } = setup((body) => ({
    ...answer(body),
    start: body.start + 100,
  }));
  reader.setView(view());
  reader.show(0, 10);
  await flush();
  expect(reader.row(0)).toBeUndefined();
  expect(reader.getSnapshot().failed).toBeDefined();
});

test("F36 FR6: a total that differs from the summary is adopted without trusting old rows", async () => {
  const { reader } = setup((body) => answer(body, 40));
  reader.setView(view({ count: 100 }));
  reader.show(0, 10);
  await flush();
  expect(reader.getSnapshot().total).toBe(40);
  expect(reader.row(39)).toBeDefined();
  expect(reader.row(40)).toBeUndefined();
});
