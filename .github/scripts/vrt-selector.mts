import { createHash } from "node:crypto";

export type VrtMode = "FULL" | "PARTIAL" | "NONE";

export type StoryIndexEntry = {
  id: string;
  type: string;
  importPath: string;
  [key: string]: unknown;
};

export type StoryIndex = {
  entries: Record<string, StoryIndexEntry>;
  [key: string]: unknown;
};

export type PreviewGraph = {
  schemaVersion: number;
  modules: Record<string, { reasons: string[] }>;
};

export type SelectionManifest = {
  mode: VrtMode;
  baseline_commit: string | null;
  head_commit: string;
  changed_paths: string[];
  in_scope_paths: string[];
  selected_story_ids: string[];
  reason_codes: string[];
  source_index_sha256: string;
  filtered_index_sha256: string | null;
  graph_artifact_sha256: string;
};

export type SelectionInput = {
  baselineCommit: string | null;
  headCommit: string;
  changedPaths: string[];
  isMain: boolean;
  isDependencyUpdate: boolean;
  dependencyPhase: 0 | 1;
  index: StoryIndex;
  graph: PreviewGraph;
  preflightErrors?: string[];
};

const normalizePath = (value: string) => value.replaceAll("\\", "/").replace(/^\.?\//, "");

const canonicalModuleId = (value: string) => {
  const normalized = normalizePath(value).split(/[?#]/, 1)[0] ?? "";
  const frontendMarker = "/apps/frontend/";
  const markerIndex = normalized.lastIndexOf(frontendMarker);
  if (markerIndex >= 0) {
    return normalized.slice(markerIndex + frontendMarker.length);
  }
  return normalized.replace(/^apps\/frontend\//, "");
};

const sortedUnique = (values: Iterable<string>) =>
  [...new Set(values)].sort((left, right) => left.localeCompare(right));

export function sha256(value: string | Uint8Array): string {
  return createHash("sha256").update(value).digest("hex");
}

export function stableJson(value: unknown): string {
  if (Array.isArray(value)) {
    return `[${value.map((item) => stableJson(item)).join(",")}]`;
  }
  if (value && typeof value === "object") {
    const record = value as Record<string, unknown>;
    return `{${Object.keys(record)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${stableJson(record[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

export function normalizeStoryIndex(index: StoryIndex): StoryIndexEntry[] {
  if (!index || typeof index !== "object" || !index.entries) {
    throw new Error("index_entries_missing");
  }
  const storyEntries = Object.values(index.entries).filter((entry) => entry.type === "story");
  const ids = storyEntries.map((entry) => entry.id);
  if (
    storyEntries.some(
      (entry) =>
        typeof entry.id !== "string" ||
        entry.id.length === 0 ||
        typeof entry.importPath !== "string" ||
        entry.importPath.length === 0,
    )
  ) {
    throw new Error("index_story_shape_invalid");
  }
  if (new Set(ids).size !== ids.length) {
    throw new Error("index_story_id_duplicate");
  }
  return storyEntries.sort((left, right) => left.id.localeCompare(right.id));
}

export function validatePreviewGraph(graph: PreviewGraph): void {
  if (graph?.schemaVersion !== 1 || !graph.modules) {
    throw new Error("graph_schema_invalid");
  }
  for (const [moduleId, module] of Object.entries(graph.modules)) {
    if (
      moduleId.length === 0 ||
      !module ||
      !Array.isArray(module.reasons) ||
      module.reasons.some((reason) => typeof reason !== "string")
    ) {
      throw new Error("graph_module_invalid");
    }
  }
}

export function reachableModules(graph: PreviewGraph, seedModules: string[]): Set<string> {
  validatePreviewGraph(graph);
  const canonicalToModule = new Map(
    Object.keys(graph.modules).map((moduleId) => [canonicalModuleId(moduleId), moduleId]),
  );
  const queue: string[] = [];
  for (const seed of seedModules) {
    const moduleId = canonicalToModule.get(canonicalModuleId(seed));
    if (!moduleId) {
      throw new Error(`graph_seed_unresolved:${seed}`);
    }
    queue.push(moduleId);
  }

  const importers = new Map<string, string[]>();
  for (const [moduleId, module] of Object.entries(graph.modules)) {
    for (const reason of module.reasons) {
      const canonicalReason = canonicalToModule.get(canonicalModuleId(reason));
      if (!canonicalReason) {
        throw new Error(`graph_reason_unresolved:${reason}`);
      }
      const values = importers.get(canonicalReason) ?? [];
      values.push(moduleId);
      importers.set(canonicalReason, values);
    }
  }

  const reached = new Set<string>();
  while (queue.length > 0) {
    const moduleId = queue.shift();
    if (!moduleId || reached.has(moduleId)) continue;
    reached.add(moduleId);
    queue.push(...(importers.get(moduleId) ?? []));
  }
  return reached;
}

export function reachableStoryIds(
  index: StoryIndex,
  graph: PreviewGraph,
  seedModules: string[],
): string[] {
  const reached = reachableModules(graph, seedModules);
  const reachedCanonical = new Set([...reached].map(canonicalModuleId));
  return normalizeStoryIndex(index)
    .filter((entry) => reachedCanonical.has(canonicalModuleId(entry.importPath)))
    .map((entry) => entry.id);
}

export function filterStoryIndex(index: StoryIndex, selectedStoryIds: string[]): StoryIndex {
  const selected = new Set(selectedStoryIds);
  const entries = Object.fromEntries(
    Object.entries(index.entries).filter(
      ([, entry]) => entry.type === "story" && selected.has(entry.id),
    ),
  );
  return { ...index, entries };
}

export function packageOwnerFromModuleId(moduleId: string): string | null {
  const segments = normalizePath(moduleId).split("/");
  const nodeModulesPositions = segments
    .map((segment, index) => (segment === "node_modules" ? index : -1))
    .filter((index) => index >= 0);
  if (nodeModulesPositions.length < 2) return null;
  const ownerStart = nodeModulesPositions[1] + 1;
  const first = segments[ownerStart];
  if (!first) return null;
  if (first.startsWith("@")) {
    const second = segments[ownerStart + 1];
    return second ? `${first}/${second}` : null;
  }
  return first;
}

export function parseGitNameStatus(raw: string): string[] {
  const fields = raw.split("\0").filter(Boolean);
  const paths: string[] = [];
  for (let index = 0; index < fields.length;) {
    const status = fields[index++];
    if (!status) break;
    const firstPath = fields[index++];
    if (!firstPath) {
      throw new Error("git_name_status_path_missing");
    }
    paths.push(firstPath);
    if (status.startsWith("R") || status.startsWith("C")) {
      const secondPath = fields[index++];
      if (!secondPath) {
        throw new Error("git_name_status_rename_target_missing");
      }
      paths.push(secondPath);
    }
  }
  return sortedUnique(paths.map(normalizePath));
}

export function isInScopePath(path: string): boolean {
  const normalized = normalizePath(path);
  return (
    normalized.startsWith("apps/frontend/") ||
    normalized === "pnpm-lock.yaml" ||
    normalized === "pnpm-workspace.yaml" ||
    normalized === ".github/workflows/argos.yml"
  );
}

export function unsafePathReason(path: string): string | null {
  const normalized = normalizePath(path);
  if (
    normalized === "pnpm-lock.yaml" ||
    normalized === "pnpm-workspace.yaml" ||
    normalized === "apps/frontend/pnpm-lock.yaml" ||
    normalized === "apps/frontend/package.json"
  ) {
    return "denylist_build_chain";
  }
  if (
    normalized.startsWith("apps/frontend/.storybook/") ||
    normalized === ".github/workflows/argos.yml"
  ) {
    return "denylist_storybook_provider";
  }
  if (/\.(css|scss|sass|less)$/.test(normalized)) {
    return "denylist_global_style";
  }
  if (
    normalized.includes("/icons/") ||
    normalized.includes("/icon/") ||
    normalized.includes("virtual:") ||
    normalized.includes("\0")
  ) {
    return "denylist_visual_or_virtual";
  }
  return null;
}

export function sourceSeedsFromPaths(paths: string[]): string[] {
  return sortedUnique(
    paths.filter(
      (path) =>
        normalizePath(path).startsWith("apps/frontend/") && /\.(vue|[cm]?[jt]sx?)$/.test(path),
    ),
  );
}

export function selectVrt(input: SelectionInput): {
  manifest: SelectionManifest;
  filteredIndex: StoryIndex | null;
} {
  const changedPaths = sortedUnique(input.changedPaths.map(normalizePath));
  const inScopePaths = changedPaths.filter(isInScopePath);
  const sourceIndexSha = sha256(stableJson(input.index));
  const graphSha = sha256(stableJson(input.graph));
  const allStoryIds = normalizeStoryIndex(input.index).map((entry) => entry.id);
  const unsafeReasons = sortedUnique(
    inScopePaths.map(unsafePathReason).filter((reason): reason is string => !!reason),
  );

  let selectedStoryIds: string[] = [];
  let mode: VrtMode = "FULL";
  let reasonCodes: string[] = [];

  if (inScopePaths.length === 0) {
    mode = "NONE";
    reasonCodes = ["out_of_scope"];
  } else if (input.isMain) {
    selectedStoryIds = allStoryIds;
    reasonCodes = ["main_full_backstop"];
  } else if (input.preflightErrors?.length) {
    selectedStoryIds = allStoryIds;
    reasonCodes = ["preflight_fail_closed", ...input.preflightErrors];
  } else if (!input.baselineCommit) {
    selectedStoryIds = allStoryIds;
    reasonCodes = ["baseline_unconfirmed"];
  } else if (unsafeReasons.length > 0) {
    selectedStoryIds = allStoryIds;
    reasonCodes = unsafeReasons;
  } else {
    const seeds = sourceSeedsFromPaths(inScopePaths);
    try {
      selectedStoryIds = reachableStoryIds(input.index, input.graph, seeds);
      if (input.isDependencyUpdate && selectedStoryIds.length === 0) {
        mode = "NONE";
        reasonCodes = ["dependency_graph_zero_reach"];
      } else if (input.isDependencyUpdate && input.dependencyPhase === 0) {
        selectedStoryIds = allStoryIds;
        reasonCodes = ["dependency_pr_full_gate"];
      } else if (selectedStoryIds.length === 0) {
        selectedStoryIds = allStoryIds;
        reasonCodes = ["zero_reach_fail_closed"];
      } else if (selectedStoryIds.length === allStoryIds.length) {
        reasonCodes = ["all_stories_reached"];
      } else {
        mode = "PARTIAL";
        reasonCodes = [
          input.isDependencyUpdate ? "dependency_pr_reachable_gate" : "exact_reachability",
        ];
      }
    } catch (error) {
      selectedStoryIds = allStoryIds;
      reasonCodes = [
        "preflight_fail_closed",
        error instanceof Error ? error.message : "unknown_preflight_error",
      ];
    }
  }

  selectedStoryIds = sortedUnique(selectedStoryIds);
  const filteredIndex = mode === "PARTIAL" ? filterStoryIndex(input.index, selectedStoryIds) : null;
  const filteredIndexSha = filteredIndex ? sha256(stableJson(filteredIndex)) : null;

  return {
    manifest: {
      mode,
      baseline_commit: input.baselineCommit,
      head_commit: input.headCommit,
      changed_paths: changedPaths,
      in_scope_paths: inScopePaths,
      selected_story_ids: selectedStoryIds,
      reason_codes: reasonCodes,
      source_index_sha256: sourceIndexSha,
      filtered_index_sha256: filteredIndexSha,
      graph_artifact_sha256: graphSha,
    },
    filteredIndex,
  };
}

export function assertCaptureGate(input: {
  manifest: SelectionManifest;
  executedStoryIds: string[];
  capturedStoryIds: string[];
  servedIndexSha256: string;
  pending: number;
  skipped: number;
  failed: number;
  screenshotsOutsideManifest: number;
}): void {
  const selected = sortedUnique(input.manifest.selected_story_ids);
  const executed = sortedUnique(input.executedStoryIds);
  const captured = sortedUnique(input.capturedStoryIds);
  const expectedIndexSha =
    input.manifest.filtered_index_sha256 ?? input.manifest.source_index_sha256;
  const failures = [
    selected.join("\0") !== executed.join("\0") ? "selected_executed_mismatch" : "",
    selected.join("\0") !== captured.join("\0") ? "selected_captured_mismatch" : "",
    input.servedIndexSha256 !== expectedIndexSha ? "served_index_sha_mismatch" : "",
    input.pending !== 0 ? "pending_nonzero" : "",
    input.skipped !== 0 ? "skipped_nonzero" : "",
    input.failed !== 0 ? "failed_nonzero" : "",
    input.screenshotsOutsideManifest !== 0 ? "screenshots_outside_manifest" : "",
    input.manifest.mode !== "NONE" && selected.length === 0 ? "capture_set_empty" : "",
  ].filter(Boolean);
  if (failures.length > 0) {
    throw new Error(failures.join(","));
  }
}
