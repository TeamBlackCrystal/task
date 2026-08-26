import { Command } from "commander";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { parseSubmitPayload, registerReviewCommands } from "../reviews";
import { CliError } from "../../utils/errors";

const mocks = vi.hoisted(() => ({
  GET: vi.fn(),
  POST: vi.fn(),
  PATCH: vi.fn(),
  print: vi.fn(),
  resolveProject: vi.fn(),
  readFileSync: vi.fn(),
}));

vi.mock("node:fs", () => ({ readFileSync: mocks.readFileSync }));
vi.mock("../../api/client", () => ({
  getClient: () => ({
    GET: mocks.GET,
    POST: mocks.POST,
    PATCH: mocks.PATCH,
  }),
  getTenantId: () => "tenant-1",
}));
vi.mock("../../utils/command", () => ({
  getOutputOptions: () => ({ json: false }),
}));
vi.mock("../../utils/output", () => ({ print: mocks.print }));
vi.mock("../../utils/projects", () => ({
  isUuid: () => false,
  resolveProject: mocks.resolveProject,
}));

function program(): Command {
  const cmd = new Command().exitOverride().option("--json", "JSON", false);
  registerReviewCommands(cmd);
  return cmd;
}

beforeEach(() => {
  vi.clearAllMocks();
  process.exitCode = undefined;
  mocks.resolveProject.mockResolvedValue({
    id: "project-1",
    key: "APP",
    name: "App",
  });
  mocks.GET.mockResolvedValue({ data: [], response: { status: 200 } });
  mocks.POST.mockResolvedValue({
    data: { id: "review-1" },
    response: { status: 201 },
  });
  mocks.PATCH.mockResolvedValue({
    data: { id: "finding-1", state: "fixed" },
    response: { status: 200 },
  });
});

describe("parseSubmitPayload", () => {
  const valid = {
    pr: 618,
    head_sha: "60cdd7795f94fa4e4148ce996c2efb4c363e3f5e",
    summary: "総評",
    findings: [
      {
        severity: "medium",
        title: "セレクタが複数一致する",
        body: "説明文にも一致するため",
        file: "src/App.vue",
        line: 42,
      },
    ],
  };

  it("正しい JSON を API のリクエストに変換する", () => {
    expect(parseSubmitPayload(valid)).toEqual({
      pr_number: 618,
      head_sha: "60cdd7795f94fa4e4148ce996c2efb4c363e3f5e",
      summary: "総評",
      findings: [
        {
          severity: "medium",
          title: "セレクタが複数一致する",
          body: "説明文にも一致するため",
          file: "src/App.vue",
          line: 42,
        },
      ],
    });
  });

  it("指摘ゼロと summary 省略も正当（指摘なしの記録）", () => {
    const payload = parseSubmitPayload({
      pr: 1,
      head_sha: "abc",
    });
    expect(payload.findings).toEqual([]);
    expect(payload.summary).toBe("");
  });

  it("--pr は JSON の pr より優先される", () => {
    expect(parseSubmitPayload(valid, 999).pr_number).toBe(999);
  });

  it.each([
    [{ head_sha: "abc" }, "positive integer `pr`"],
    [{ pr: 0, head_sha: "abc" }, "positive integer `pr`"],
    [{ pr: 1 }, "`head_sha`"],
    [{ pr: 1, head_sha: "   " }, "`head_sha`"],
    [{ pr: 1, head_sha: "abc", findings: {} }, "`findings` must be an array"],
  ])("必須項目が欠けていたら弾く: %j", (input, expected) => {
    expect(() => parseSubmitPayload(input)).toThrow(expected);
  });

  it.each([
    [
      { severity: "critical", title: "t", body: "b" },
      "severity must be one of",
    ],
    [{ severity: "high", title: "", body: "b" }, "title is required"],
    [{ severity: "high", title: "t" }, "body is required"],
    [
      { severity: "high", title: "t", body: "b", line: 1.5 },
      "line must be an integer",
    ],
  ])("指摘の不備は位置つきで弾く: %j", (finding, expected) => {
    expect(() =>
      parseSubmitPayload({ pr: 1, head_sha: "abc", findings: [finding] }),
    ).toThrow(expected);
    // どの指摘が悪いのか分かるように添字を出す
    expect(() =>
      parseSubmitPayload({ pr: 1, head_sha: "abc", findings: [finding] }),
    ).toThrow(/findings\[0\]/);
  });

  it("オブジェクトでない入力を弾く", () => {
    expect(() => parseSubmitPayload([])).toThrow("must be an object");
    expect(() => parseSubmitPayload(null)).toThrow("must be an object");
  });
});

describe("review commands", () => {
  it("submit は JSON を読んで一括作成 API を 1 回だけ呼ぶ", async () => {
    mocks.readFileSync.mockReturnValue(
      JSON.stringify({
        pr: 618,
        head_sha: "abc",
        findings: [{ severity: "high", title: "t", body: "b" }],
      }),
    );

    await program().parseAsync([
      "node",
      "task",
      "review",
      "submit",
      "findings.json",
      "--project",
      "APP",
    ]);

    expect(mocks.POST).toHaveBeenCalledTimes(1);
    expect(mocks.POST).toHaveBeenCalledWith(
      "/v1/tenants/{tenant_id}/projects/{project_id}/reviews",
      {
        params: {
          path: { tenant_id: "tenant-1", project_id: "project-1" },
        },
        body: {
          pr_number: 618,
          head_sha: "abc",
          summary: "",
          findings: [
            {
              severity: "high",
              title: "t",
              body: "b",
              file: null,
              line: null,
            },
          ],
        },
      },
    );
  });

  it("submit は - で標準入力から読む", async () => {
    mocks.readFileSync.mockReturnValue('{"pr":1,"head_sha":"abc"}');
    await program().parseAsync([
      "node",
      "task",
      "review",
      "submit",
      "-",
      "--project",
      "APP",
    ]);
    expect(mocks.readFileSync).toHaveBeenCalledWith(0, "utf8");
    expect(mocks.POST).toHaveBeenCalledTimes(1);
  });

  it("list は絞り込みをクエリに載せる", async () => {
    await program().parseAsync([
      "node",
      "task",
      "review",
      "list",
      "--project",
      "APP",
      "--pr",
      "618",
      "--state",
      "open,fixed",
      "--severity",
      "high",
    ]);

    expect(mocks.GET).toHaveBeenCalledWith(
      "/v1/tenants/{tenant_id}/projects/{project_id}/review-findings",
      {
        params: {
          path: { tenant_id: "tenant-1", project_id: "project-1" },
          query: { pr: 618, state: "open,fixed", severity: "high" },
        },
      },
    );
  });

  it.each([
    ["--state", "closed", "unknown state"],
    ["--severity", "critical", "unknown severity"],
  ])(
    "list は綴り違いの絞り込みを送信前に弾く（%s=%s）",
    async (flag, value, expected) => {
      await expect(
        program().parseAsync([
          "node",
          "task",
          "review",
          "list",
          "--project",
          "APP",
          "--pr",
          "618",
          flag,
          value,
        ]),
      ).rejects.toThrow(expected);
      expect(mocks.GET).not.toHaveBeenCalled();
    },
  );

  it("--pr が整数でなければ送信しない", async () => {
    await expect(
      program().parseAsync([
        "node",
        "task",
        "review",
        "list",
        "--project",
        "APP",
        "--pr",
        "abc",
      ]),
    ).rejects.toThrow(CliError);
    expect(mocks.GET).not.toHaveBeenCalled();
  });

  it("resolve は状態と理由を送る", async () => {
    await program().parseAsync([
      "node",
      "task",
      "review",
      "resolve",
      "finding-1",
      "--project",
      "APP",
      "--state",
      "deferred",
      "--note",
      "後で直す",
    ]);

    expect(mocks.PATCH).toHaveBeenCalledWith(
      "/v1/tenants/{tenant_id}/projects/{project_id}/review-findings/{id}",
      {
        params: {
          path: {
            tenant_id: "tenant-1",
            project_id: "project-1",
            id: "finding-1",
          },
        },
        body: { state: "deferred", note: "後で直す" },
      },
    );
  });

  it("resolve は未知の状態を送信前に弾く", async () => {
    await expect(
      program().parseAsync([
        "node",
        "task",
        "review",
        "resolve",
        "finding-1",
        "--project",
        "APP",
        "--state",
        "done",
      ]),
    ).rejects.toThrow("unknown state");
    expect(mocks.PATCH).not.toHaveBeenCalled();
  });

  const REVIEWED_HEAD = "60cdd7795f94fa4e4148ce996c2efb4c363e3f5e";

  const runSummary = (extra: string[] = []) =>
    program().parseAsync([
      "node",
      "task",
      "review",
      "summary",
      "--project",
      "APP",
      "--pr",
      "618",
      ...extra,
    ]);

  it("summary はレビュー済みで未解決ゼロ・HEAD 一致のときだけ 0 で終わる", async () => {
    mocks.GET.mockResolvedValue({
      data: {
        pr_number: 618,
        rounds: 1,
        counts: [],
        blocking: 0,
        latest_head_sha: REVIEWED_HEAD,
        mergeable: true,
      },
      response: { status: 200 },
    });
    await runSummary(["--head", REVIEWED_HEAD]);
    expect(process.exitCode).toBeUndefined();

    mocks.GET.mockResolvedValue({
      data: {
        pr_number: 618,
        rounds: 2,
        counts: [{ severity: "high", state: "open", count: 1 }],
        blocking: 1,
        latest_head_sha: REVIEWED_HEAD,
        mergeable: false,
      },
      response: { status: 200 },
    });
    await runSummary(["--head", REVIEWED_HEAD]);
    // マージ前確認に使えるよう、未解決が残っていれば失敗として終わる
    expect(process.exitCode).toBe(1);
  });

  it("summary はレビューされていない PR を通さない", async () => {
    mocks.GET.mockResolvedValue({
      data: {
        pr_number: 618,
        rounds: 0,
        counts: [],
        blocking: 0,
        latest_head_sha: null,
        mergeable: false,
      },
      response: { status: 200 },
    });
    await runSummary(["--head", REVIEWED_HEAD]);
    expect(process.exitCode).toBe(1);
  });

  it("summary はレビュー後にコミットが積まれていれば通さない", async () => {
    mocks.GET.mockResolvedValue({
      data: {
        pr_number: 618,
        rounds: 1,
        counts: [],
        blocking: 0,
        latest_head_sha: REVIEWED_HEAD,
        mergeable: true,
      },
      response: { status: 200 },
    });
    await runSummary(["--head", "0000000000000000000000000000000000000000"]);
    expect(process.exitCode).toBe(1);

    // --no-head-check なら鮮度を見ない（明示的に外したときだけ）
    process.exitCode = undefined;
    await runSummary(["--no-head-check"]);
    expect(process.exitCode).toBeUndefined();
  });
});
