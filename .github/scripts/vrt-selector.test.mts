import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { test } from "vitest";
import {
  assertCaptureGate,
  filterStoryIndex,
  normalizeStoryIndex,
  packageOwnerFromModuleId,
  parseGitNameStatus,
  reachableStoryIds,
  selectVrt,
  sha256,
  stableJson,
  type PreviewGraph,
  type StoryIndex,
} from "./vrt-selector.mts";

const fixturePath = (name: string) =>
  path.resolve(
    process.cwd(),
    `../../.github/scripts/fixtures/vrt-selector/${name}`,
  );

const loadJson = async <T,>(name: string): Promise<T> =>
  JSON.parse(await readFile(fixturePath(name), "utf8")) as T;

const expectedPasswordStories = [
  "auth-passwordinput--password-hidden",
  "auth-passwordinput--password-visible",
  "auth-signinform--default",
  "auth-signupform--default",
  "pages-resetpassword--request-form",
  "pages-resetpassword--request-success",
  "pages-resetpassword--complete-form",
  "pages-resetpassword--invalid-token",
  "pages-signin--default",
  "pages-signin--login-error",
  "pages-signin--login-success",
  "pages-signup--default",
  "pages-signup--register-error",
  "pages-signup--register-submitting",
  "pages-signup--register-success",
  "pages-signup--register-success-201",
].sort();

const expectedNewStoryEntries = ["newstory--compact", "newstory--default"];

test("PasswordInput fixture reaches the exact 16-story set and excludes docs", async () => {
  const index = await loadJson<StoryIndex>("index.json");
  const graph = await loadJson<PreviewGraph>("preview-graph.json");
  assert.deepEqual(
    reachableStoryIds(index, graph, ["apps/frontend/src/components/auth/PasswordInput.vue"]),
    expectedPasswordStories,
  );
  assert.equal(
    normalizeStoryIndex(index).some((entry) => entry.id.endsWith("--docs")),
    false,
  );
});

test("a newly added story file selects its story entries as PARTIAL", async () => {
  const index = await loadJson<StoryIndex>("index.json");
  const graph = await loadJson<PreviewGraph>("preview-graph.json");
  const { manifest } = selectVrt({
    baselineCommit: "a".repeat(40),
    headCommit: "b".repeat(40),
    changedPaths: ["apps/frontend/stories/NewStory.stories.ts"],
    isMain: false,
    isDependencyUpdate: false,
    dependencyPhase: 0,
    index,
    graph,
  });
  assert.equal(manifest.mode, "PARTIAL");
  assert.deepEqual(manifest.selected_story_ids, expectedNewStoryEntries);
  assert.deepEqual(manifest.reason_codes, ["exact_reachability"]);
});

test("a story entry added to an existing file remains explicit in the exact gate", async () => {
  const index = await loadJson<StoryIndex>("index.json");
  const graph = await loadJson<PreviewGraph>("preview-graph.json");
  const { manifest, filteredIndex } = selectVrt({
    baselineCommit: "a".repeat(40),
    headCommit: "b".repeat(40),
    changedPaths: ["apps/frontend/stories/NewStory.stories.ts"],
    isMain: false,
    isDependencyUpdate: false,
    dependencyPhase: 0,
    index,
    graph,
  });
  const filteredSha = sha256(stableJson(filteredIndex));
  assert.deepEqual(manifest.selected_story_ids, expectedNewStoryEntries);
  assert.doesNotThrow(() =>
    assertCaptureGate({
      manifest,
      executedStoryIds: expectedNewStoryEntries,
      capturedStoryIds: expectedNewStoryEntries,
      servedIndexSha256: filteredSha,
      pending: 0,
      skipped: 0,
      failed: 0,
      screenshotsOutsideManifest: 0,
    }),
  );
});

test("unknown source fails closed to FULL", async () => {
  const index = await loadJson<StoryIndex>("index.json");
  const graph = await loadJson<PreviewGraph>("preview-graph.json");
  const { manifest } = selectVrt({
    baselineCommit: "a".repeat(40),
    headCommit: "b".repeat(40),
    changedPaths: ["apps/frontend/src/components/Unknown.vue"],
    isMain: false,
    isDependencyUpdate: false,
    dependencyPhase: 0,
    index,
    graph,
  });
  assert.equal(manifest.mode, "FULL");
  assert.equal(manifest.reason_codes[0], "preflight_fail_closed");
});

test("safe source change selects a PARTIAL exact set", async () => {
  const index = await loadJson<StoryIndex>("index.json");
  const graph = await loadJson<PreviewGraph>("preview-graph.json");
  const { manifest, filteredIndex } = selectVrt({
    baselineCommit: "a".repeat(40),
    headCommit: "b".repeat(40),
    changedPaths: ["apps/frontend/src/components/auth/PasswordInput.vue"],
    isMain: false,
    isDependencyUpdate: false,
    dependencyPhase: 0,
    index,
    graph,
  });
  assert.equal(manifest.mode, "PARTIAL");
  assert.deepEqual(manifest.selected_story_ids, expectedPasswordStories);
  assert.deepEqual(
    normalizeStoryIndex(filteredIndex as StoryIndex).map((entry) => entry.id),
    expectedPasswordStories,
  );
});

test("main remains a FULL per-merge backstop", async () => {
  const index = await loadJson<StoryIndex>("index.json");
  const graph = await loadJson<PreviewGraph>("preview-graph.json");
  const { manifest } = selectVrt({
    baselineCommit: "a".repeat(40),
    headCommit: "b".repeat(40),
    changedPaths: ["apps/frontend/src/components/auth/PasswordInput.vue"],
    isMain: true,
    isDependencyUpdate: false,
    dependencyPhase: 0,
    index,
    graph,
  });
  assert.equal(manifest.mode, "FULL");
  assert.deepEqual(manifest.reason_codes, ["main_full_backstop"]);
});

test("owner normalization uses the package below the second node_modules", async () => {
  const fixture = await loadJson<{
    cases: { moduleId: string; owner: string }[];
  }>("owner-paths.json");
  for (const item of fixture.cases) {
    assert.equal(packageOwnerFromModuleId(item.moduleId), item.owner);
  }
  assert.equal(packageOwnerFromModuleId("/repo/node_modules/.pnpm/vite@8/index.js"), null);
});

test("B..H name-status union includes both sides of renames and copies", () => {
  assert.deepEqual(
    parseGitNameStatus(
      [
        "M",
        "apps/frontend/src/Changed.vue",
        "R100",
        "apps/frontend/src/Old.vue",
        "apps/frontend/src/New.vue",
        "C090",
        "apps/frontend/src/Source.vue",
        "apps/frontend/src/Copy.vue",
        "D",
        "apps/frontend/src/Deleted.vue",
        "",
      ].join("\0"),
    ),
    [
      "apps/frontend/src/Changed.vue",
      "apps/frontend/src/Copy.vue",
      "apps/frontend/src/Deleted.vue",
      "apps/frontend/src/New.vue",
      "apps/frontend/src/Old.vue",
      "apps/frontend/src/Source.vue",
    ],
  );
});

test("capture gate requires exact selected/executed/captured sets and SHA", async () => {
  const index = await loadJson<StoryIndex>("index.json");
  const graph = await loadJson<PreviewGraph>("preview-graph.json");
  const filteredIndex = filterStoryIndex(index, expectedPasswordStories);
  const filteredSha = sha256(stableJson(filteredIndex));
  const { manifest } = selectVrt({
    baselineCommit: "a".repeat(40),
    headCommit: "b".repeat(40),
    changedPaths: ["apps/frontend/src/components/auth/PasswordInput.vue"],
    isMain: false,
    isDependencyUpdate: false,
    dependencyPhase: 0,
    index,
    graph,
  });
  assert.equal(manifest.filtered_index_sha256, filteredSha);
  assert.doesNotThrow(() =>
    assertCaptureGate({
      manifest,
      executedStoryIds: expectedPasswordStories,
      capturedStoryIds: expectedPasswordStories,
      servedIndexSha256: filteredSha,
      pending: 0,
      skipped: 0,
      failed: 0,
      screenshotsOutsideManifest: 0,
    }),
  );
  assert.throws(() =>
    assertCaptureGate({
      manifest,
      executedStoryIds: expectedPasswordStories.slice(1),
      capturedStoryIds: expectedPasswordStories,
      servedIndexSha256: filteredSha,
      pending: 0,
      skipped: 0,
      failed: 0,
      screenshotsOutsideManifest: 0,
    }),
  );
});

test("fixture files are versioned beside the test", () => {
  assert.match(fixturePath("index.json"), /vrt-selector/);
});
