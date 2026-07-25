import assert from "node:assert/strict";
import { test } from "vitest";
import { selectVrt, type PreviewGraph, type StoryIndex } from "./vrt-selector.mts";

const index: StoryIndex = {
  entries: {
    "one--default": {
      id: "one--default",
      type: "story",
      importPath: "./stories/One.stories.ts",
    },
    "two--default": {
      id: "two--default",
      type: "story",
      importPath: "./stories/Two.stories.ts",
    },
  },
};
const graph: PreviewGraph = {
  schemaVersion: 1,
  modules: {
    "apps/frontend/src/One.vue": { reasons: [] },
    "apps/frontend/stories/One.stories.ts": {
      reasons: ["apps/frontend/src/One.vue"],
    },
    "apps/frontend/src/Two.vue": { reasons: [] },
    "apps/frontend/stories/Two.stories.ts": {
      reasons: ["apps/frontend/src/Two.vue"],
    },
  },
};

test("provider adapter inputs remain provider-neutral at selector boundary", () => {
  const result = selectVrt({
    baselineCommit: "1".repeat(40),
    headCommit: "2".repeat(40),
    changedPaths: ["apps/frontend/src/One.vue"],
    isMain: false,
    isDependencyUpdate: false,
    dependencyPhase: 0,
    index,
    graph,
  });
  assert.equal(result.manifest.mode, "PARTIAL");
  assert.deepEqual(result.manifest.selected_story_ids, ["one--default"]);
  assert.equal(
    Object.hasOwn(result.manifest, "argosBuild"),
    false,
    "provider payload must not leak into selector manifest",
  );
});
