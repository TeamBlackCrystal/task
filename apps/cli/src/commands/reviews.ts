import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { Command } from "commander";
import { getClient, getTenantId } from "../api/client";
import type {
  CreateFindingInput,
  CreateReviewRequest,
  FindingSeverity,
  FindingState,
  ReviewFinding,
  ReviewSummary,
} from "../api/paths";
import { getOutputOptions } from "../utils/command";
import { CliError, unwrapApiResult } from "../utils/errors";
import type { OutputOptions } from "../utils/output";
import { print } from "../utils/output";
import { resolveProject } from "../utils/projects";

const SEVERITIES: FindingSeverity[] = ["high", "medium", "low", "nit"];
const STATES: FindingState[] = [
  "open",
  "fixed",
  "verified",
  "deferred",
  "rejected",
];

type SubmitOptions = OutputOptions & { project?: string };
type ListOptions = OutputOptions & {
  project?: string;
  pr?: string;
  state?: string;
  severity?: string;
};
type ResolveOptions = OutputOptions & {
  project?: string;
  state?: string;
  note?: string;
};
type SummaryOptions = OutputOptions & {
  project?: string;
  pr?: string;
  head?: string;
  /** commander が `--no-head-check` から作る（既定は true） */
  headCheck?: boolean;
};

function parsePrNumber(raw: string | undefined): number {
  const value = Number(raw);
  if (!Number.isInteger(value) || value < 1) {
    throw new CliError(`--pr must be a positive integer (got: ${raw})`, 2);
  }
  return value;
}

/** カンマ区切りの絞り込みを検証する。綴り違いを黙って通すと結果を誤読する。 */
function validateCsv<T extends string>(
  raw: string | undefined,
  allowed: readonly T[],
  label: string,
): string | undefined {
  if (raw === undefined) return undefined;
  const values = raw
    .split(",")
    .map((part) => part.trim())
    .filter((part) => part.length > 0);
  for (const value of values) {
    if (!allowed.includes(value as T)) {
      throw new CliError(
        `unknown ${label}: ${value} (expected one of ${allowed.join(", ")})`,
        2,
      );
    }
  }
  return values.length > 0 ? values.join(",") : undefined;
}

/** ファイルか標準入力から JSON を読む。`-` は標準入力。 */
function readJsonInput(file: string): unknown {
  const raw =
    file === "-"
      ? readFileSync(0, "utf8")
      : readFileSync(file, "utf8");
  try {
    return JSON.parse(raw);
  } catch (error) {
    throw new CliError(
      `invalid JSON in ${file === "-" ? "stdin" : file}: ${
        error instanceof Error ? error.message : String(error)
      }`,
      2,
    );
  }
}

/**
 * 投入 JSON を検証して API のリクエストへ変換する。
 *
 * サーバー側でも検証するが、AI が生成した JSON の取り違え（severity の綴り、
 * 必須項目の欠落）はここで具体的に指摘したほうが直しやすい。
 */
export function parseSubmitPayload(
  input: unknown,
  prOverride?: number,
): CreateReviewRequest {
  if (typeof input !== "object" || input === null || Array.isArray(input)) {
    throw new CliError("review JSON must be an object", 2);
  }
  const record = input as Record<string, unknown>;

  const prNumber = prOverride ?? record.pr ?? record.pr_number;
  if (typeof prNumber !== "number" || !Number.isInteger(prNumber) || prNumber < 1) {
    throw new CliError("review JSON needs a positive integer `pr`", 2);
  }

  const headSha = record.head_sha;
  if (typeof headSha !== "string" || headSha.trim().length === 0) {
    throw new CliError("review JSON needs `head_sha` (the reviewed commit)", 2);
  }

  const summary = record.summary === undefined ? "" : record.summary;
  if (typeof summary !== "string") {
    throw new CliError("`summary` must be a string", 2);
  }

  const rawFindings = record.findings ?? [];
  if (!Array.isArray(rawFindings)) {
    throw new CliError("`findings` must be an array", 2);
  }

  const findings: CreateFindingInput[] = rawFindings.map((item, index) => {
    const where = `findings[${index}]`;
    if (typeof item !== "object" || item === null || Array.isArray(item)) {
      throw new CliError(`${where} must be an object`, 2);
    }
    const finding = item as Record<string, unknown>;

    const severity = finding.severity;
    if (
      typeof severity !== "string" ||
      !SEVERITIES.includes(severity as FindingSeverity)
    ) {
      throw new CliError(
        `${where}.severity must be one of ${SEVERITIES.join(", ")}`,
        2,
      );
    }
    for (const key of ["title", "body"] as const) {
      const value = finding[key];
      if (typeof value !== "string" || value.trim().length === 0) {
        throw new CliError(`${where}.${key} is required`, 2);
      }
    }
    if (finding.file !== undefined && finding.file !== null && typeof finding.file !== "string") {
      throw new CliError(`${where}.file must be a string`, 2);
    }
    if (
      finding.line !== undefined &&
      finding.line !== null &&
      (typeof finding.line !== "number" || !Number.isInteger(finding.line))
    ) {
      throw new CliError(`${where}.line must be an integer`, 2);
    }

    return {
      severity: severity as FindingSeverity,
      title: finding.title as string,
      body: finding.body as string,
      file: (finding.file as string | null | undefined) ?? null,
      line: (finding.line as number | null | undefined) ?? null,
    };
  });

  return {
    pr_number: prNumber,
    head_sha: headSha.trim(),
    summary,
    findings,
  };
}

/** 人間向けの 1 行表示。`--json` のときは使わない。 */
function formatFinding(finding: ReviewFinding): string {
  const location = finding.file
    ? ` ${finding.file}${finding.line ? `:${finding.line}` : ""}`
    : "";
  return [
    finding.id,
    finding.severity.toUpperCase().padEnd(6),
    finding.state.padEnd(8),
    `R${finding.round}`,
    finding.title + location,
  ].join("\t");
}

/**
 * 照合する HEAD。`--head` があればそれ、無ければ実行ディレクトリの HEAD。
 *
 * GitHub へ取りに行かないのは、CI でも手元でも「いま検査している木」の SHA が
 * そこにあるからで、余計な依存と権限を増やさないため（仕様 §6）。
 */
function resolveHead(explicit: string | undefined): string | null {
  if (explicit) return explicit.trim() || null;
  try {
    return execFileSync("git", ["rev-parse", "HEAD"], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
  } catch {
    return null;
  }
}

/** マージ前ゲートとしての判定。通してよい理由が揃わなければ通さない。 */
function gateFailure(
  summary: ReviewSummary,
  head: string | null,
  checkHead: boolean,
): string | null {
  if (summary.rounds === 0) {
    return "this pull request has not been reviewed yet (no rounds)";
  }
  if (!summary.mergeable) {
    return `${summary.blocking} high/medium finding(s) still unresolved`;
  }
  if (!checkHead) return null;
  if (!head) {
    return "cannot determine the HEAD to compare (pass --head or --no-head-check)";
  }
  if (summary.latest_head_sha !== head) {
    return `reviewed ${summary.latest_head_sha ?? "(none)"} but HEAD is ${head}; re-review is needed`;
  }
  return null;
}

function formatSummary(summary: ReviewSummary, failure: string | null): string {
  const verdict = failure ? `blocked (${failure})` : "mergeable";
  const lines = [
    `PR #${summary.pr_number}\trounds: R${summary.rounds}\t${verdict}`,
  ];
  for (const entry of summary.counts) {
    lines.push(`  ${entry.severity}\t${entry.state}\t${entry.count}`);
  }
  return lines.join("\n");
}

export function registerReviewCommands(program: Command): void {
  const review = program
    .command("review")
    .description("Review findings commands");

  review
    .command("submit")
    .description("Submit one review round (findings are created together)")
    .argument("<file>", "JSON file with the round, or - for stdin")
    .requiredOption("--project <key>", "Project key or UUID")
    .option("--pr <number>", "PR number (overrides `pr` in the JSON)")
    .action(async (file: string, opts: SubmitOptions & { pr?: string }, cmd) => {
      const output = getOutputOptions(cmd);
      const payload = parseSubmitPayload(
        readJsonInput(file),
        opts.pr === undefined ? undefined : parsePrNumber(opts.pr),
      );
      const project = await resolveProject(opts.project!);
      const client = getClient();
      const result = await client.POST(
        "/v1/tenants/{tenant_id}/projects/{project_id}/reviews",
        {
          params: {
            path: { tenant_id: getTenantId(), project_id: project.id },
          },
          body: payload,
        },
      );
      print(unwrapApiResult(result), output);
    });

  review
    .command("list")
    .description("List findings for a PR")
    .requiredOption("--project <key>", "Project key or UUID")
    .requiredOption("--pr <number>", "PR number")
    .option("--state <states>", `Filter by state (${STATES.join(",")})`)
    .option("--severity <severities>", `Filter by severity (${SEVERITIES.join(",")})`)
    .action(async (opts: ListOptions, cmd) => {
      const output = getOutputOptions(cmd);
      const pr = parsePrNumber(opts.pr);
      const state = validateCsv(opts.state, STATES, "state");
      const severity = validateCsv(opts.severity, SEVERITIES, "severity");
      const project = await resolveProject(opts.project!);
      const client = getClient();
      const result = await client.GET(
        "/v1/tenants/{tenant_id}/projects/{project_id}/review-findings",
        {
          params: {
            path: { tenant_id: getTenantId(), project_id: project.id },
            query: { pr, state, severity },
          },
        },
      );
      const findings = unwrapApiResult(result);
      if (output.json) {
        print(findings, output);
        return;
      }
      for (const finding of findings) {
        console.log(formatFinding(finding));
      }
    });

  review
    .command("rounds")
    .description("List review rounds for a PR")
    .requiredOption("--project <key>", "Project key or UUID")
    .requiredOption("--pr <number>", "PR number")
    .action(async (opts: SummaryOptions, cmd) => {
      const output = getOutputOptions(cmd);
      const pr = parsePrNumber(opts.pr);
      const project = await resolveProject(opts.project!);
      const client = getClient();
      const result = await client.GET(
        "/v1/tenants/{tenant_id}/projects/{project_id}/reviews",
        {
          params: {
            path: { tenant_id: getTenantId(), project_id: project.id },
            query: { pr },
          },
        },
      );
      const rounds = unwrapApiResult(result);
      if (output.json) {
        print(rounds, output);
        return;
      }
      for (const round of rounds) {
        console.log(
          [
            `R${round.round}`,
            round.head_sha.slice(0, 12),
            round.reviewer.username,
            `${round.finding_count} findings`,
          ].join("\t"),
        );
      }
    });

  review
    .command("resolve")
    .description("Move a finding to a new state")
    .argument("<id>", "Finding UUID")
    .requiredOption("--project <key>", "Project key or UUID")
    .requiredOption("--state <state>", `New state (${STATES.join(",")})`)
    .option("--note <text>", "Why the state changed (kept in the history)")
    .action(async (id: string, opts: ResolveOptions, cmd) => {
      const output = getOutputOptions(cmd);
      const state = opts.state as FindingState;
      if (!STATES.includes(state)) {
        throw new CliError(
          `unknown state: ${opts.state} (expected one of ${STATES.join(", ")})`,
          2,
        );
      }
      const project = await resolveProject(opts.project!);
      const client = getClient();
      const result = await client.PATCH(
        "/v1/tenants/{tenant_id}/projects/{project_id}/review-findings/{id}",
        {
          params: {
            path: { tenant_id: getTenantId(), project_id: project.id, id },
          },
          body: { state, note: opts.note ?? null },
        },
      );
      print(unwrapApiResult(result), output);
    });

  review
    .command("summary")
    .description(
      "Show the merge verdict for a PR (exits 1 unless it is reviewed, clean, and up to date)",
    )
    .requiredOption("--project <key>", "Project key or UUID")
    .requiredOption("--pr <number>", "PR number")
    .option(
      "--head <sha>",
      "Commit to compare with the reviewed HEAD (default: git rev-parse HEAD)",
    )
    .option("--no-head-check", "Do not compare the reviewed HEAD with the working tree")
    .action(async (opts: SummaryOptions, cmd) => {
      const output = getOutputOptions(cmd);
      const pr = parsePrNumber(opts.pr);
      const project = await resolveProject(opts.project!);
      const client = getClient();
      const result = await client.GET(
        "/v1/tenants/{tenant_id}/projects/{project_id}/reviews/summary",
        {
          params: {
            path: { tenant_id: getTenantId(), project_id: project.id },
            query: { pr },
          },
        },
      );
      const summary = unwrapApiResult(result);
      const checkHead = opts.headCheck !== false;
      const head = checkHead ? resolveHead(opts.head) : null;
      const failure = gateFailure(summary, head, checkHead);

      if (output.json) {
        print({ ...summary, head, blocked_reason: failure }, output);
      } else {
        console.log(formatSummary(summary, failure));
      }
      // マージ前ゲートとして使えるよう、通してよい理由が揃わなければ非 0 で終わる
      if (failure) {
        process.exitCode = 1;
      }
    });
}
