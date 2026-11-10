import assert from "node:assert/strict";
import { test } from "node:test";

import { buildTree, countDocs, fileName, filterDocs, firstDoc, resolveLink, tagCounts, titleFromFile, toggleTag } from "../app/src/features/wiki/wikiTree.ts";
import type { DreamDoc } from "../app/src/platform/backend/types.ts";

const doc = (path: string, title: string, at: string, project?: string): DreamDoc => ({ path, title, kind: path.startsWith("외부 자료/") ? "external" : path.startsWith("작업 성공/") ? "done" : path.startsWith("도구 오류/") ? "tool" : path.startsWith("배운 것/") ? "note" : "failed", project, at, size: 1 });
const DOCS = [
  doc("작업 실패/blog/r2-build.md", "설치 파일 · 빌드", "2026-01-03T00:00:00.000Z", "C:/work/blog"),
  doc("작업 성공/blog/r1-build.md", "설치 파일 · 빌드", "2026-01-02T00:00:00.000Z", "C:/work/blog"),
  doc("작업 성공/blog/r3-test.md", "설치 파일 · 검증", "2026-01-04T00:00:00.000Z", "C:/work/blog"),
  doc("작업 성공/app/r0-copy.md", "문구 · 작성", "2026-01-01T00:00:00.000Z", "C:/work/app"),
  doc("외부 자료/20260101-guide.md", "배포 안내", "2026-01-01T12:00:00.000Z"),
];
const PATHS = DOCS.map((d) => d.path);

test("buildTree lists materials, then successes, then failures, each project a folder and documents newest first", () => {
  const root = buildTree(DOCS);
  assert.deepEqual(root.docs, []);
  assert.deepEqual(root.folders.map((f) => f.path), ["외부 자료", "작업 성공", "작업 실패"]);
  const done = root.folders[1];
  assert.deepEqual(done.folders.map((f) => [f.name, f.path]), [["app", "작업 성공/app"], ["blog", "작업 성공/blog"]]);
  assert.deepEqual(done.folders[1].docs.map((d) => d.path), ["작업 성공/blog/r3-test.md", "작업 성공/blog/r1-build.md"]);
  assert.deepEqual(root.folders.map(countDocs), [1, 3, 1]);
  // Only what is there is listed: no empty top folder.
  assert.deepEqual(buildTree([DOCS[0]]).folders.map((f) => f.name), ["작업 실패"]);
  // What the cards tried in vain comes last.
  const tool = doc("도구 오류/blog/r2-build.md", "도구 오류 2건 · 설치 파일 · 빌드", "2026-01-03T00:00:00.000Z", "C:/work/blog");
  const note = doc("배운 것/blog/r2-build-1.md", "서명 키는 환경 변수로만", "2026-01-03T00:00:00.000Z", "C:/work/blog");
  assert.deepEqual(buildTree([tool, ...DOCS, note]).folders.map((f) => f.name), ["배운 것", "외부 자료", "작업 성공", "작업 실패", "도구 오류"]);
  assert.deepEqual(buildTree([]), { name: "", path: "", folders: [], docs: [] });
});

test("filterDocs matches every word against the title, the path or the project", () => {
  assert.equal(filterDocs(DOCS, "  ").length, DOCS.length);
  assert.deepEqual(filterDocs(DOCS, "빌드 실패").map((d) => d.path), ["작업 실패/blog/r2-build.md"]);
  assert.deepEqual(filterDocs(DOCS, "WORK/APP").map((d) => d.path), ["작업 성공/app/r0-copy.md"]);
  assert.deepEqual(filterDocs(DOCS, "nothing"), []);
});

test("documents are found by their tags, and a #word asks for exactly that tag", () => {
  const tagged = [{ ...DOCS[0], tags: ["서명", "배포"] }, { ...DOCS[1], tags: ["서명키"] }, { ...DOCS[4], tags: ["배포"] }, DOCS[3]];
  assert.deepEqual(filterDocs(tagged, "서명").map((d) => d.path), ["작업 실패/blog/r2-build.md", "작업 성공/blog/r1-build.md"]);
  assert.deepEqual(filterDocs(tagged, "#서명").map((d) => d.path), ["작업 실패/blog/r2-build.md"]);
  assert.deepEqual(filterDocs(tagged, "#배포 안내").map((d) => d.path), ["외부 자료/20260101-guide.md"]);
  assert.deepEqual(tagCounts(tagged), [{ tag: "배포", count: 2 }, { tag: "서명", count: 1 }, { tag: "서명키", count: 1 }]);
  assert.equal(toggleTag("빌드", "배포"), "빌드 #배포");
  assert.equal(toggleTag("빌드 #배포  #서명", "배포"), "빌드 #서명");
});

test("firstDoc opens the newest document", () => {
  assert.equal(firstDoc(DOCS), "작업 성공/blog/r3-test.md");
  assert.equal(firstDoc([]), undefined);
  assert.equal(fileName("작업 성공/blog/r3-test.md"), "r3-test.md");
});

test("resolveLink leads only to a listed document named by its path", () => {
  assert.equal(resolveLink("작업 성공/blog/r3-test.md", "작업 성공/blog/r1-build.md#결과", PATHS), "작업 성공/blog/r1-build.md");
  assert.equal(resolveLink("작업 성공/blog/r3-test.md", "/%EC%99%B8%EB%B6%80%20%EC%9E%90%EB%A3%8C/20260101-guide.md", PATHS), "외부 자료/20260101-guide.md");
  for (const href of ["https://example.invalid/guide.md", "mailto:a@example.invalid", "#section", "//example.invalid/x.md", "", "r1-build.md", "../blog/r1-build.md", "작업 성공/blog/r3-test.md"]) {
    assert.equal(resolveLink("작업 성공/blog/r3-test.md", href, PATHS), undefined, href);
  }
});

test("titleFromFile names a material after its file", () => {
  assert.equal(titleFromFile("C:\\notes\\release-checklist.md"), "release-checklist");
  assert.equal(titleFromFile("notes/2026.01 plan.txt"), "2026.01 plan");
  assert.equal(titleFromFile(".env"), ".env");
});
