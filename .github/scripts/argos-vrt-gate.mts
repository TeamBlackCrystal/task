import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import {
  assertCaptureGate,
  sha256,
  stableJson,
  type SelectionManifest,
  type StoryIndex,
} from "./vrt-selector.mts";

const repositoryRoot = process.cwd();
const evidenceDirectory = path.join(repositoryRoot, ".vrt");
const manifest = JSON.parse(
  await readFile(path.join(evidenceDirectory, "selection-manifest.json"), "utf8"),
) as SelectionManifest;
const servedIndex = JSON.parse(
  await readFile(path.join(repositoryRoot, "apps/frontend/storybook-static/index.json"), "utf8"),
) as StoryIndex;

const readIds = async (name: string) =>
  (await readFile(path.join(evidenceDirectory, name), "utf8"))
    .split("\n")
    .map((value) => value.trim())
    .filter(Boolean);

const [executedStoryIds, capturedStoryIds] = await Promise.all([
  readIds("executed-story-ids.txt"),
  readIds("captured-story-ids.txt"),
]);

let pending = 0;
let skipped = 0;
let failed = 0;
const testResultsPath = path.join(evidenceDirectory, "test-results.json");
try {
  const testResults = JSON.parse(await readFile(testResultsPath, "utf8")) as {
    numPendingTests?: number;
    numTodoTests?: number;
    numFailedTests?: number;
  };
  pending = testResults.numTodoTests ?? 0;
  skipped = testResults.numPendingTests ?? 0;
  failed = testResults.numFailedTests ?? 0;
} catch (error) {
  if (manifest.mode !== "NONE") {
    throw new Error(`test_results_missing:${error instanceof Error ? error.message : "unknown"}`);
  }
}

const selected = new Set(manifest.selected_story_ids);
const screenshotsOutsideManifest = capturedStoryIds.filter(
  (storyId) => !selected.has(storyId),
).length;
const servedIndexSha256 = sha256(stableJson(servedIndex));
const evidence = {
  mode: manifest.mode,
  selected_story_ids: manifest.selected_story_ids,
  executed_story_ids: executedStoryIds,
  captured_story_ids: capturedStoryIds,
  served_index_sha256: servedIndexSha256,
  pending,
  skipped,
  failed,
  screenshots_outside_manifest: screenshotsOutsideManifest,
};

await writeFile(
  path.join(evidenceDirectory, "capture-gate.json"),
  `${stableJson(evidence)}\n`,
  "utf8",
);
assertCaptureGate({
  manifest,
  executedStoryIds,
  capturedStoryIds,
  servedIndexSha256,
  pending,
  skipped,
  failed,
  screenshotsOutsideManifest,
});
console.log(
  JSON.stringify({
    mode: manifest.mode,
    selected: manifest.selected_story_ids.length,
    executed: executedStoryIds.length,
    captured: capturedStoryIds.length,
    uploadGate: "passed",
  }),
);
