import { execFileSync } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import {
  parseGitNameStatus,
  reachableStoryIds,
  selectVrt,
  stableJson,
  type PreviewGraph,
  type StoryIndex,
} from "./vrt-selector.mts";

type ArgosBuild = {
  head: { sha: string; branch: string | null };
};

const repositoryRoot = process.cwd();
const evidenceDirectory = path.join(repositoryRoot, ".vrt");
const storybookDirectory = path.join(repositoryRoot, "apps/frontend/storybook-static");
const sourceIndexPath = path.join(storybookDirectory, "index.json");
const graphPath = path.join(evidenceDirectory, "preview-graph.json");
const manifestPath = path.join(evidenceDirectory, "selection-manifest.json");
const fixtureDirectory = path.join(repositoryRoot, ".github/scripts/fixtures/vrt-selector");

const git = (...args: string[]) =>
  execFileSync("git", args, { cwd: repositoryRoot, encoding: "utf8" }).trim();

const changedPathsBetween = (baseline: string | null, head: string) => {
  if (!baseline) {
    return parseGitNameStatus(
      execFileSync("git", ["show", "--format=", "--name-status", "-z", "--find-renames", head], {
        cwd: repositoryRoot,
        encoding: "utf8",
      }),
    );
  }
  return parseGitNameStatus(
    execFileSync(
      "git",
      [
        "log",
        "--format=",
        "--name-status",
        "-z",
        "--find-renames",
        `${baseline}..${head}`,
      ],
      {
        cwd: repositoryRoot,
        encoding: "utf8",
      },
    ),
  );
};

async function resolveArgosBaseline(head: string): Promise<string | null> {
  const token = process.env.ARGOS_TOKEN;
  if (!token) return null;
  const referenceBranch = process.env.ARGOS_REFERENCE_BRANCH ?? "main";
  let base = head;
  try {
    base = git("merge-base", `origin/${referenceBranch}`, head);
  } catch {
    // The API result remains authoritative; a missing local merge-base fails
    // closed because no candidate can be submitted.
    return null;
  }
  const commits = git("rev-list", base).split("\n").filter(Boolean);
  if (commits.length === 0) return null;
  const response = await fetch(
    `${process.env.ARGOS_API_BASE_URL ?? "https://api.argos-ci.com/v2/"}baseline`,
    {
      method: "POST",
      headers: {
        authorization: `Bearer ${token}`,
        "content-type": "application/json",
      },
      body: JSON.stringify({
        commits,
        name: process.env.ARGOS_BUILD_NAME ?? "default",
        mode: "ci",
      }),
    },
  );
  if (!response.ok) {
    throw new Error(`argos_baseline_http_${response.status}`);
  }
  const payload = (await response.json()) as { baseline: ArgosBuild | null };
  const baseline = payload.baseline?.head.sha ?? null;
  if (baseline) {
    try {
      git("merge-base", "--is-ancestor", baseline, head);
    } catch {
      throw new Error("argos_baseline_not_ancestor");
    }
  }
  return baseline;
}

const headCommit = git("rev-parse", "HEAD");
let baselineCommit: string | null = null;
const reasonCodes: string[] = [];
try {
  baselineCommit = await resolveArgosBaseline(headCommit);
} catch (error) {
  reasonCodes.push(error instanceof Error ? error.message : "argos_baseline_unknown_error");
}

const [index, graph, fixtureIndex, fixtureGraph] = await Promise.all([
  readFile(sourceIndexPath, "utf8").then((value) => JSON.parse(value) as StoryIndex),
  readFile(graphPath, "utf8").then((value) => JSON.parse(value) as PreviewGraph),
  readFile(path.join(fixtureDirectory, "index.json"), "utf8").then(
    (value) => JSON.parse(value) as StoryIndex,
  ),
  readFile(path.join(fixtureDirectory, "preview-graph.json"), "utf8").then(
    (value) => JSON.parse(value) as PreviewGraph,
  ),
]);
const expectedFixtureStories = [
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
const actualFixtureStories = reachableStoryIds(fixtureIndex, fixtureGraph, [
  "apps/frontend/src/components/auth/PasswordInput.vue",
]);
const preflightErrors =
  actualFixtureStories.join("\0") === expectedFixtureStories.join("\0")
    ? []
    : ["password_input_exact_set_fixture_mismatch"];
const changedPaths = changedPathsBetween(baselineCommit, headCommit);
const isMain =
  process.env.GITHUB_EVENT_NAME === "push" && process.env.GITHUB_REF === "refs/heads/main";
const headRef = process.env.GITHUB_HEAD_REF ?? "";
const isDependencyUpdate =
  process.env.VRT_DEPENDENCY_UPDATE === "true" || headRef.startsWith("renovate/");

const selection = selectVrt({
  baselineCommit,
  headCommit,
  changedPaths,
  isMain,
  isDependencyUpdate,
  dependencyPhase: process.env.VRT_DEPENDENCY_PHASE === "1" ? 1 : 0,
  index,
  graph,
  preflightErrors,
});
selection.manifest.reason_codes.push(...reasonCodes);

await mkdir(evidenceDirectory, { recursive: true });
await writeFile(
  path.join(evidenceDirectory, "source-index.json"),
  `${stableJson(index)}\n`,
  "utf8",
);
await writeFile(manifestPath, `${stableJson(selection.manifest)}\n`, "utf8");
if (selection.filteredIndex) {
  await writeFile(sourceIndexPath, `${stableJson(selection.filteredIndex)}\n`, "utf8");
}
await writeFile(path.join(evidenceDirectory, "executed-story-ids.txt"), "", "utf8");
await writeFile(path.join(evidenceDirectory, "captured-story-ids.txt"), "", "utf8");

const githubOutput = process.env.GITHUB_OUTPUT;
if (githubOutput) {
  await writeFile(githubOutput, `mode=${selection.manifest.mode}\nmanifest=${manifestPath}\n`, {
    encoding: "utf8",
    flag: "a",
  });
}

console.log(
  JSON.stringify({
    mode: selection.manifest.mode,
    baselineCommit,
    headCommit,
    selectedCount: selection.manifest.selected_story_ids.length,
    reasonCodes: selection.manifest.reason_codes,
    manifestPath,
  }),
);
