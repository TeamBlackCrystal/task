//! レビュー指摘のコマンド。仕様は `docs/features/review-findings.md` §6 が正。
//!
//! 送信前の検証をここで持つのは、AI が生成した JSON の取り違え（severity の綴り、
//! 必須項目の欠落）をサーバーの検証に任せきりにすると、どこを直せばよいかの
//! 手がかりが薄くなるため。綴りと規則は backend の型（`entity` / `common`）を
//! そのまま使い、CLI 側に写しを作らない。

use std::io::Read;
use std::process::Stdio;

use entity::review_findings::{FindingSeverity, FindingState};
use payload::reviews::{
    CreateFindingInput, CreateReviewRequest, FindingResponse, ReviewDetailResponse, ReviewResponse,
    ReviewSummaryResponse, UpdateFindingStateRequest,
};
use sea_orm::Iterable;
use serde::Serialize;
use serde_json::Value;

use crate::Context;
use crate::cli::ReviewCommand;
use crate::error::{CliError, Result};
use crate::output::{OutputOptions, print};
use crate::resolve::resolve_project;

/// 実行後に呼び出し側へ返す終了コード。ゲートは通してよい理由が揃わなければ 1 で終わる。
pub async fn run(context: &Context, command: ReviewCommand, output: OutputOptions) -> Result<i32> {
    match command {
        ReviewCommand::Submit { file, project, pr } => {
            let pr = pr.as_deref().map(parse_pr_number).transpose()?;
            let payload = parse_submit_payload(&read_json_input(&file)?, pr)?;
            // 検証を終えてから接続する。設定不足で、直せる誤りの指摘を隠さない
            let api = &context.connect()?;
            let project = resolve_project(api, &project).await?;
            let created: ReviewDetailResponse = api
                .post(
                    &[
                        "v1",
                        "tenants",
                        api.tenant_id(),
                        "projects",
                        &project.id.to_string(),
                        "reviews",
                    ],
                    &payload,
                )
                .await?;
            print(&created, output);
        }
        ReviewCommand::List {
            project,
            pr,
            repo,
            state,
            severity,
        } => {
            let pr = parse_pr_number(&pr)?;
            let repo = validate_repo(repo.as_deref())?;
            let state = validate_csv(
                state.as_deref(),
                &FindingState::iter().collect::<Vec<_>>(),
                "state",
            )?;
            let severity = validate_csv(
                severity.as_deref(),
                &FindingSeverity::iter().collect::<Vec<_>>(),
                "severity",
            )?;
            // 検証を終えてから接続する。設定不足で、直せる誤りの指摘を隠さない
            let api = &context.connect()?;
            let project = resolve_project(api, &project).await?;

            let mut query = vec![("pr", pr.to_string())];
            push_optional(&mut query, "repo", repo);
            push_optional(&mut query, "state", state);
            push_optional(&mut query, "severity", severity);

            let findings: Vec<FindingResponse> = api
                .get(
                    &[
                        "v1",
                        "tenants",
                        api.tenant_id(),
                        "projects",
                        &project.id.to_string(),
                        "review-findings",
                    ],
                    &query,
                )
                .await?;
            if output.json {
                print(&findings, output);
            } else {
                for finding in &findings {
                    println!("{}", format_finding(finding));
                }
            }
        }
        ReviewCommand::Rounds { project, pr, repo } => {
            let pr = parse_pr_number(&pr)?;
            let repo = validate_repo(repo.as_deref())?;
            // 検証を終えてから接続する。設定不足で、直せる誤りの指摘を隠さない
            let api = &context.connect()?;
            let project = resolve_project(api, &project).await?;

            let mut query = vec![("pr", pr.to_string())];
            push_optional(&mut query, "repo", repo);

            let rounds: Vec<ReviewResponse> = api
                .get(
                    &[
                        "v1",
                        "tenants",
                        api.tenant_id(),
                        "projects",
                        &project.id.to_string(),
                        "reviews",
                    ],
                    &query,
                )
                .await?;
            if output.json {
                print(&rounds, output);
            } else {
                for round in &rounds {
                    println!("{}", format_round(round));
                }
            }
        }
        ReviewCommand::Resolve {
            id,
            project,
            state,
            note,
        } => {
            let state = parse_state(&state)?;
            // 検証を終えてから接続する。設定不足で、直せる誤りの指摘を隠さない
            let api = &context.connect()?;
            let project = resolve_project(api, &project).await?;
            let updated: FindingResponse = api
                .patch(
                    &[
                        "v1",
                        "tenants",
                        api.tenant_id(),
                        "projects",
                        &project.id.to_string(),
                        "review-findings",
                        &id,
                    ],
                    &UpdateFindingStateRequest { state, note },
                )
                .await?;
            print(&updated, output);
        }
        ReviewCommand::Summary {
            project,
            pr,
            repo,
            head,
            no_head_check,
            allow_unlinked,
        } => {
            let pr = parse_pr_number(&pr)?;
            let repo = validate_repo(repo.as_deref())?;
            // 検証を終えてから接続する。設定不足で、直せる誤りの指摘を隠さない
            let api = &context.connect()?;
            let project = resolve_project(api, &project).await?;

            let mut query = vec![("pr", pr.to_string())];
            push_optional(&mut query, "repo", repo);

            let summary: ReviewSummaryResponse = api
                .get(
                    &[
                        "v1",
                        "tenants",
                        api.tenant_id(),
                        "projects",
                        &project.id.to_string(),
                        "reviews",
                        "summary",
                    ],
                    &query,
                )
                .await?;

            let check_head = !no_head_check;
            let head = check_head.then(|| resolve_head(head.as_deref())).flatten();
            let failure = gate_failure(&summary, head.as_deref(), check_head, allow_unlinked);

            if output.json {
                print(
                    &SummaryOutput {
                        summary: &summary,
                        head: head.as_deref(),
                        blocked_reason: failure.as_deref(),
                    },
                    output,
                );
            } else {
                println!("{}", format_summary(&summary, failure.as_deref()));
            }
            // マージ前ゲートとして使えるよう、通してよい理由が揃わなければ非 0 で終わる
            if failure.is_some() {
                return Ok(1);
            }
        }
    }
    Ok(0)
}

/// `--json` のときだけ足す判定結果。集計そのものはサーバーの型のまま出す。
#[derive(Serialize)]
struct SummaryOutput<'a> {
    #[serde(flatten)]
    summary: &'a ReviewSummaryResponse,
    head: Option<&'a str>,
    blocked_reason: Option<&'a str>,
}

fn push_optional(
    query: &mut Vec<(&'static str, String)>,
    key: &'static str,
    value: Option<String>,
) {
    if let Some(value) = value {
        query.push((key, value));
    }
}

fn parse_pr_number(raw: &str) -> Result<i32> {
    raw.parse::<i32>()
        .ok()
        .filter(|value| *value >= 1)
        .ok_or_else(|| {
            CliError::validation(format!("--pr must be a positive integer (got: {raw})"))
        })
}

fn parse_state(raw: &str) -> Result<FindingState> {
    raw.parse::<FindingState>().map_err(|_| {
        CliError::validation(format!(
            "unknown state: {raw} (expected one of {})",
            join(&FindingState::iter().collect::<Vec<_>>())
        ))
    })
}

/// カンマ区切りの絞り込みを検証する。綴り違いを黙って通すと結果を誤読する。
fn validate_csv<T: Spelled + Copy>(
    raw: Option<&str>,
    allowed: &[T],
    label: &str,
) -> Result<Option<String>> {
    let Some(raw) = raw else { return Ok(None) };
    let values: Vec<&str> = raw
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    for value in &values {
        if !allowed
            .iter()
            .any(|candidate| candidate.spelling() == *value)
        {
            return Err(CliError::validation(format!(
                "unknown {label}: {value} (expected one of {})",
                join(allowed)
            )));
        }
    }
    Ok((!values.is_empty()).then(|| values.join(",")))
}

/// 読み取りの視界にするリポジトリを検証する。未指定ならサーバーの既定（現在の連携先）。
///
/// 連携を差し替えると旧リポジトリのラウンドが既定の視界から外れるので、過去の
/// 連携先を明示して読めるようにする。空文字は「連携を張る前に溜めたラウンド」を指す
/// （サーバーが空文字列で控えている）ため、未指定に丸めない。
fn validate_repo(raw: Option<&str>) -> Result<Option<String>> {
    let Some(raw) = raw else { return Ok(None) };
    let value = raw.trim();
    if value.is_empty() {
        return Ok(Some(String::new()));
    }
    let parts: Vec<&str> = value.split('/').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(CliError::validation(format!(
            "--repo must be owner/name, or \"\" for rounds recorded before the integration (got: {raw})"
        )));
    }
    Ok(Some(value.to_string()))
}

/// ファイルか標準入力から JSON を読む。`-` は標準入力。
fn read_json_input(file: &str) -> Result<Value> {
    let raw = if file == "-" {
        let mut buffer = String::new();
        std::io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|err| CliError::validation(format!("cannot read stdin: {err}")))?;
        buffer
    } else {
        std::fs::read_to_string(file)
            .map_err(|err| CliError::validation(format!("cannot read {file}: {err}")))?
    };
    let where_ = if file == "-" { "stdin" } else { file };
    serde_json::from_str(&raw)
        .map_err(|err| CliError::validation(format!("invalid JSON in {where_}: {err}")))
}

/// 投入 JSON を検証して API のリクエストへ変換する。
pub fn parse_submit_payload(
    input: &Value,
    pr_override: Option<i32>,
) -> Result<CreateReviewRequest> {
    let Some(record) = input.as_object() else {
        return Err(CliError::validation("review JSON must be an object"));
    };

    let pr_number = match pr_override {
        Some(value) => value,
        None => {
            let raw = non_null(record.get("pr")).or_else(|| non_null(record.get("pr_number")));
            raw.and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok())
                .filter(|value| *value >= 1)
                .ok_or_else(|| CliError::validation("review JSON needs a positive integer `pr`"))?
        }
    };

    let head_sha = record
        .get("head_sha")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            CliError::validation("review JSON needs `head_sha` (the reviewed commit)")
        })?;
    // ゲートは head_sha を厳密一致で比べる。`git log --oneline` が見せるのは短縮 SHA
    // なので "60cdd77" と書くのは自然だが、それで確定したラウンドは指摘を全部
    // 解消しても通らなくなる。しかも出るのは「同じ commit に見えるのに再レビューが
    // 要る」という読み解きにくい形なので、投入時に弾く
    if !common::validation::COMMIT_SHA_REGEX.is_match(head_sha) {
        let raw = record
            .get("head_sha")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return Err(CliError::validation(format!(
            "`head_sha` must be the full 40-character commit SHA (got: {raw})"
        )));
    }

    let summary = match record.get("summary") {
        None => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(_) => return Err(CliError::validation("`summary` must be a string")),
    };

    let findings = match non_null(record.get("findings")) {
        None => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(index, item)| parse_finding(item, index))
            .collect::<Result<Vec<_>>>()?,
        Some(_) => return Err(CliError::validation("`findings` must be an array")),
    };

    Ok(CreateReviewRequest {
        pr_number,
        head_sha: head_sha.to_string(),
        summary,
        findings,
    })
}

fn parse_finding(item: &Value, index: usize) -> Result<CreateFindingInput> {
    let where_ = format!("findings[{index}]");
    let Some(finding) = item.as_object() else {
        return Err(CliError::validation(format!("{where_} must be an object")));
    };

    let severity = finding
        .get("severity")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<FindingSeverity>().ok())
        .ok_or_else(|| {
            CliError::validation(format!(
                "{where_}.severity must be one of {}",
                join(&FindingSeverity::iter().collect::<Vec<_>>())
            ))
        })?;

    let mut text = [("title", String::new()), ("body", String::new())];
    for (key, slot) in &mut text {
        let value = finding
            .get(*key)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| CliError::validation(format!("{where_}.{key} is required")))?;
        *slot = value.to_string();
    }
    let [(_, title), (_, body)] = text;

    let file = match non_null(finding.get("file")) {
        None => None,
        Some(Value::String(text)) => Some(text.clone()),
        Some(_) => {
            return Err(CliError::validation(format!(
                "{where_}.file must be a string"
            )));
        }
    };
    let line = match non_null(finding.get("line")) {
        None => None,
        Some(value) => Some(
            value
                .as_i64()
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| CliError::validation(format!("{where_}.line must be an integer")))?,
        ),
    };

    Ok(CreateFindingInput {
        severity,
        title,
        body,
        file,
        line,
    })
}

/// 未指定と `null` を同じ「無い」として扱う（JSON の生成側で揺れやすいため）。
fn non_null(value: Option<&Value>) -> Option<&Value> {
    value.filter(|value| !value.is_null())
}

/// 人間向けの 1 行表示。`--json` のときは使わない。
fn format_finding(finding: &FindingResponse) -> String {
    let location = match (&finding.file, finding.line) {
        (Some(file), Some(line)) => format!(" {file}:{line}"),
        (Some(file), None) => format!(" {file}"),
        (None, _) => String::new(),
    };
    format!(
        "{}\t{:<6}\t{:<8}\tR{}\t{}{}",
        finding.id,
        finding.severity.as_str().to_uppercase(),
        finding.state.as_str(),
        finding.round,
        finding.title,
        location,
    )
}

fn format_round(round: &ReviewResponse) -> String {
    format!(
        "R{}\t{}\t{}\t{} findings",
        round.round,
        &round.head_sha[..round.head_sha.len().min(12)],
        round.reviewer.username,
        round.finding_count,
    )
}

/// 照合する HEAD。`--head` があればそれ、無ければ実行ディレクトリの HEAD。
///
/// GitHub へ取りに行かないのは、CI でも手元でも「いま検査している木」の SHA が
/// そこにあるからで、余計な依存と権限を増やさないため（仕様 §6）。
fn resolve_head(explicit: Option<&str>) -> Option<String> {
    // 比較は厳密一致なので、大文字で渡された SHA が別物にならないよう揃える
    if let Some(explicit) = explicit {
        let value = explicit.trim().to_lowercase();
        return (!value.is_empty()).then_some(value);
    }
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?.trim().to_lowercase();
    (!value.is_empty()).then_some(value)
}

/// マージ前ゲートとしての判定。通してよい理由が揃わなければ通さない。
fn gate_failure(
    summary: &ReviewSummaryResponse,
    head: Option<&str>,
    check_head: bool,
    allow_unlinked: bool,
) -> Option<String> {
    // 集計の視界は現在の連携先で決まる。連携が無いと視界が空になり、空のラウンド
    // 1 本で「レビュー済み・指摘なし」を作れてしまうので、確定しない集計は通さない
    if summary.repository.is_none() && !allow_unlinked {
        return Some(
            "this project has no GitHub integration, so the reviewed repository is unknown (pass --allow-unlinked to skip)"
                .to_string(),
        );
    }
    if summary.rounds == 0 {
        return Some("this pull request has not been reviewed yet (no rounds)".to_string());
    }
    if !summary.mergeable {
        return Some(format!(
            "{} high/medium finding(s) still unresolved",
            summary.blocking
        ));
    }
    if !check_head {
        return None;
    }
    let Some(head) = head else {
        return Some(
            "cannot determine the HEAD to compare (pass --head or --no-head-check)".to_string(),
        );
    };
    let reviewed = summary.latest_head_sha.as_deref();
    if reviewed.map(str::to_lowercase).as_deref() != Some(head) {
        return Some(format!(
            "reviewed {} but HEAD is {head}; re-review is needed",
            reviewed.unwrap_or("(none)")
        ));
    }
    None
}

fn format_summary(summary: &ReviewSummaryResponse, failure: Option<&str>) -> String {
    let verdict = match failure {
        Some(reason) => format!("blocked ({reason})"),
        None => "mergeable".to_string(),
    };
    let mut lines = vec![format!(
        "PR #{}\trounds: R{}\t{verdict}",
        summary.pr_number, summary.rounds
    )];
    for entry in &summary.counts {
        lines.push(format!(
            "  {}\t{}\t{}",
            entry.severity.as_str(),
            entry.state.as_str(),
            entry.count
        ));
    }
    lines.join("\n")
}

/// 綴りを 1 か所（entity の `as_str`）から取るための橋渡し。
trait Spelled {
    fn spelling(&self) -> &'static str;
}

impl Spelled for FindingSeverity {
    fn spelling(&self) -> &'static str {
        self.as_str()
    }
}

impl Spelled for FindingState {
    fn spelling(&self) -> &'static str {
        self.as_str()
    }
}

fn join<T: Spelled>(values: &[T]) -> String {
    values
        .iter()
        .map(Spelled::spelling)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use payload::reviews::SeverityStateCount;
    use serde_json::json;

    /// 40 桁の小文字 16 進。ゲートが厳密一致で比べるので、短縮 SHA は投入時に弾かれる。
    const HEAD_SHA: &str = "60cdd7795f94fa4e4148ce996c2efb4c363e3f5e";

    fn valid_round() -> Value {
        json!({
            "pr": 618,
            "head_sha": HEAD_SHA,
            "summary": "総評",
            "findings": [{
                "severity": "medium",
                "title": "セレクタが複数一致する",
                "body": "説明文にも一致するため",
                "file": "src/App.vue",
                "line": 42,
            }],
        })
    }

    #[test]
    fn turns_a_valid_round_into_the_api_request() {
        let payload = parse_submit_payload(&valid_round(), None).unwrap();

        assert_eq!(payload.pr_number, 618);
        assert_eq!(payload.head_sha, HEAD_SHA);
        assert_eq!(payload.summary, "総評");
        assert_eq!(payload.findings.len(), 1);
        assert_eq!(payload.findings[0].severity, FindingSeverity::Medium);
        assert_eq!(payload.findings[0].file.as_deref(), Some("src/App.vue"));
        assert_eq!(payload.findings[0].line, Some(42));
    }

    #[test]
    fn accepts_a_round_with_no_findings_and_no_summary() {
        // 「指摘なし」の記録も 1 ラウンドとして正当
        let payload =
            parse_submit_payload(&json!({ "pr": 1, "head_sha": HEAD_SHA }), None).unwrap();
        assert!(payload.findings.is_empty());
        assert_eq!(payload.summary, "");
    }

    #[test]
    fn lets_the_pr_option_override_the_json() {
        assert_eq!(
            parse_submit_payload(&valid_round(), Some(999))
                .unwrap()
                .pr_number,
            999
        );
    }

    #[test]
    fn reads_pr_number_as_an_alias_of_pr() {
        let payload =
            parse_submit_payload(&json!({ "pr_number": 7, "head_sha": HEAD_SHA }), None).unwrap();
        assert_eq!(payload.pr_number, 7);
    }

    /// ゲートは head_sha を厳密一致で比べる。`git log --oneline` が見せるのは短縮 SHA
    /// なので書き間違えやすく、通してしまうとそのラウンドは指摘を全部解消しても
    /// 抜けられない（「同じ commit に見えるのに再レビューが要る」と出る）。
    #[test]
    fn rejects_a_head_sha_that_is_not_forty_lowercase_hex_digits() {
        for sha in [
            "60cdd77",                                   // 短縮 SHA
            "60CDD7795F94FA4E4148CE996C2EFB4C363E3F5E",  // 大文字
            "60cdd7795f94fa4e4148ce996c2efb4c363e3f5e0", // 41 桁
            "zzcdd7795f94fa4e4148ce996c2efb4c363e3f5e",  // 16 進でない文字
        ] {
            let err = parse_submit_payload(&json!({ "pr": 1, "head_sha": sha }), None).unwrap_err();
            assert!(
                err.message
                    .contains("must be the full 40-character commit SHA"),
                "{sha}: {}",
                err.message
            );
            assert_eq!(err.exit_code, 2, "{sha}");
        }
    }

    #[test]
    fn rejects_a_round_missing_its_required_fields() {
        let cases: [(Value, &str); 6] = [
            (json!({ "head_sha": HEAD_SHA }), "positive integer `pr`"),
            (
                json!({ "pr": 0, "head_sha": HEAD_SHA }),
                "positive integer `pr`",
            ),
            (
                json!({ "pr": 1.5, "head_sha": HEAD_SHA }),
                "positive integer `pr`",
            ),
            (json!({ "pr": 1 }), "`head_sha`"),
            (json!({ "pr": 1, "head_sha": "   " }), "`head_sha`"),
            (
                json!({ "pr": 1, "head_sha": HEAD_SHA, "findings": {} }),
                "`findings` must be an array",
            ),
        ];
        for (input, expected) in cases {
            let err = parse_submit_payload(&input, None).unwrap_err();
            assert!(err.message.contains(expected), "{input}: {}", err.message);
            assert_eq!(err.exit_code, 2, "{input}");
        }
    }

    #[test]
    fn points_at_the_offending_finding_by_index() {
        let cases: [(Value, &str); 5] = [
            (
                json!({ "severity": "critical", "title": "t", "body": "b" }),
                "severity must be one of",
            ),
            (
                json!({ "severity": "high", "title": "", "body": "b" }),
                "title is required",
            ),
            (
                json!({ "severity": "high", "title": "t" }),
                "body is required",
            ),
            (
                json!({ "severity": "high", "title": "t", "body": "b", "line": 1.5 }),
                "line must be an integer",
            ),
            (
                json!({ "severity": "high", "title": "t", "body": "b", "file": 7 }),
                "file must be a string",
            ),
        ];
        for (finding, expected) in cases {
            let input = json!({ "pr": 1, "head_sha": HEAD_SHA, "findings": [finding] });
            let err = parse_submit_payload(&input, None).unwrap_err();
            assert!(err.message.contains(expected), "{}", err.message);
            // どの指摘が悪いのか分かるように添字を出す
            assert!(err.message.contains("findings[0]"), "{}", err.message);
        }
    }

    #[test]
    fn treats_null_findings_and_null_locations_as_absent() {
        let payload = parse_submit_payload(
            &json!({
                "pr": 1,
                "head_sha": HEAD_SHA,
                "findings": null,
            }),
            None,
        )
        .unwrap();
        assert!(payload.findings.is_empty());

        let payload = parse_submit_payload(
            &json!({
                "pr": 1,
                "head_sha": HEAD_SHA,
                "findings": [{ "severity": "nit", "title": "t", "body": "b", "file": null, "line": null }],
            }),
            None,
        )
        .unwrap();
        assert_eq!(payload.findings[0].file, None);
        assert_eq!(payload.findings[0].line, None);
    }

    #[test]
    fn rejects_input_that_is_not_an_object() {
        for input in [json!([]), json!(null), json!("round")] {
            assert!(
                parse_submit_payload(&input, None)
                    .unwrap_err()
                    .message
                    .contains("must be an object")
            );
        }
    }

    #[test]
    fn rejects_a_non_string_summary_but_accepts_an_absent_one() {
        let err = parse_submit_payload(
            &json!({ "pr": 1, "head_sha": HEAD_SHA, "summary": 7 }),
            None,
        )
        .unwrap_err();
        assert!(
            err.message.contains("`summary` must be a string"),
            "{}",
            err.message
        );
    }

    #[test]
    fn rejects_a_pr_option_that_is_not_a_positive_integer() {
        for raw in ["abc", "0", "-3", "1.5", ""] {
            let err = parse_pr_number(raw).unwrap_err();
            assert!(
                err.message.contains("--pr must be a positive integer"),
                "{raw}"
            );
            assert_eq!(err.exit_code, 2, "{raw}");
        }
        assert_eq!(parse_pr_number("618").unwrap(), 618);
    }

    #[test]
    fn rejects_misspelled_filters_before_sending_them() {
        let states: Vec<FindingState> = FindingState::iter().collect();
        let severities: Vec<FindingSeverity> = FindingSeverity::iter().collect();

        let err = validate_csv(Some("closed"), &states, "state").unwrap_err();
        assert!(
            err.message.starts_with("unknown state: closed"),
            "{}",
            err.message
        );
        assert_eq!(err.exit_code, 2);

        let err = validate_csv(Some("critical"), &severities, "severity").unwrap_err();
        assert!(
            err.message.starts_with("unknown severity: critical"),
            "{}",
            err.message
        );

        assert_eq!(
            validate_csv(Some("open, fixed"), &states, "state")
                .unwrap()
                .as_deref(),
            Some("open,fixed")
        );
        assert_eq!(validate_csv(None, &states, "state").unwrap(), None);
        // 区切りだけの入力は絞り込み無しと同じ
        assert_eq!(validate_csv(Some(" , "), &states, "state").unwrap(), None);
    }

    #[test]
    fn accepts_owner_name_and_the_empty_repository_but_nothing_else() {
        assert_eq!(
            validate_repo(Some("acme/old")).unwrap().as_deref(),
            Some("acme/old")
        );
        // 連携を張る前に溜めたラウンドは空文字で指す。未指定に丸めると到達手段が無くなる
        assert_eq!(validate_repo(Some("")).unwrap().as_deref(), Some(""));
        assert_eq!(validate_repo(None).unwrap(), None);

        for value in ["acme", "acme/", "/old", "acme/old/extra"] {
            let err = validate_repo(Some(value)).unwrap_err();
            assert!(
                err.message.starts_with("--repo must be owner/name"),
                "{value}"
            );
            assert_eq!(err.exit_code, 2, "{value}");
        }
    }

    fn summary(
        rounds: i32,
        blocking: u64,
        repository: Option<&str>,
        mergeable: bool,
    ) -> ReviewSummaryResponse {
        ReviewSummaryResponse {
            pr_number: 618,
            rounds,
            counts: vec![],
            blocking,
            latest_head_sha: (rounds > 0).then(|| HEAD_SHA.to_string()),
            cached_pr_head_sha: None,
            pr_head_checked_at: None,
            owner_override_rejections: 0,
            repository: repository.map(str::to_string),
            mergeable,
        }
    }

    #[test]
    fn passes_only_a_reviewed_clean_and_current_pull_request() {
        let clean = summary(1, 0, Some("acme/app"), true);
        assert_eq!(gate_failure(&clean, Some(HEAD_SHA), true, false), None);
    }

    #[test]
    fn blocks_a_pull_request_with_unresolved_high_or_medium_findings() {
        let blocked = summary(2, 1, Some("acme/app"), false);
        let reason = gate_failure(&blocked, Some(HEAD_SHA), true, false).unwrap();
        assert_eq!(reason, "1 high/medium finding(s) still unresolved");
    }

    #[test]
    fn blocks_a_pull_request_that_has_never_been_reviewed() {
        // 未レビューと「指摘なし」は違う。件数だけで判定すると素通りする
        let unreviewed = summary(0, 0, Some("acme/app"), false);
        let reason = gate_failure(&unreviewed, Some(HEAD_SHA), true, false).unwrap();
        assert!(reason.contains("has not been reviewed yet"), "{reason}");
    }

    #[test]
    fn blocks_a_project_without_a_github_integration_unless_it_is_waived() {
        // 連携を外すと集計の視界が空になり、空のラウンド 1 本で「可」を作れてしまう
        let unlinked = summary(1, 0, None, true);
        assert!(gate_failure(&unlinked, Some(HEAD_SHA), true, false).is_some());
        assert_eq!(gate_failure(&unlinked, Some(HEAD_SHA), true, true), None);
    }

    #[test]
    fn blocks_a_pull_request_that_gained_commits_after_the_review() {
        let clean = summary(1, 0, Some("acme/app"), true);
        let stale = gate_failure(
            &clean,
            Some("0000000000000000000000000000000000000000"),
            true,
            false,
        );
        assert!(stale.unwrap().contains("re-review is needed"));

        // --no-head-check なら鮮度を見ない（明示的に外したときだけ）
        assert_eq!(gate_failure(&clean, None, false, false), None);
    }

    #[test]
    fn blocks_when_the_head_to_compare_cannot_be_determined() {
        let clean = summary(1, 0, Some("acme/app"), true);
        let reason = gate_failure(&clean, None, true, false).unwrap();
        assert!(reason.contains("cannot determine the HEAD"), "{reason}");
    }

    #[test]
    fn compares_the_reviewed_head_case_insensitively() {
        let clean = summary(1, 0, Some("acme/app"), true);
        // resolve_head が小文字へ揃えるので、集計側が大文字でも同じ commit と読む
        assert_eq!(
            resolve_head(Some(&HEAD_SHA.to_uppercase())).as_deref(),
            Some(HEAD_SHA)
        );
        assert_eq!(gate_failure(&clean, Some(HEAD_SHA), true, false), None);
    }

    #[test]
    fn reports_no_reviewed_commit_when_the_summary_has_none() {
        let mut clean = summary(1, 0, Some("acme/app"), true);
        clean.latest_head_sha = None;
        let reason = gate_failure(&clean, Some(HEAD_SHA), true, false).unwrap();
        assert!(
            reason.starts_with("reviewed (none) but HEAD is"),
            "{reason}"
        );
    }

    #[test]
    fn renders_the_verdict_and_the_severity_breakdown() {
        let mut blocked = summary(2, 1, Some("acme/app"), false);
        blocked.counts = vec![SeverityStateCount {
            severity: FindingSeverity::High,
            state: FindingState::Open,
            count: 1,
        }];
        let rendered = format_summary(&blocked, Some("1 high/medium finding(s) still unresolved"));

        assert_eq!(
            rendered,
            "PR #618\trounds: R2\tblocked (1 high/medium finding(s) still unresolved)\n  high\topen\t1"
        );
        assert_eq!(
            format_summary(&summary(1, 0, Some("a/b"), true), None),
            "PR #618\trounds: R1\tmergeable"
        );
    }

    fn finding(file: Option<&str>, line: Option<i32>) -> FindingResponse {
        FindingResponse {
            id: uuid::Uuid::nil(),
            review_id: uuid::Uuid::nil(),
            pr_number: 618,
            round: 2,
            severity: FindingSeverity::Medium,
            title: "セレクタが複数一致する".into(),
            body: "…".into(),
            file: file.map(str::to_string),
            line,
            state: FindingState::Open,
            deferred_task_id: None,
            fixed_by: None,
            created_at: Utc.timestamp_opt(0, 0).unwrap(),
            updated_at: Utc.timestamp_opt(0, 0).unwrap(),
            transitions: vec![],
        }
    }

    #[test]
    fn renders_a_finding_with_and_without_a_location() {
        let with_line = format_finding(&finding(Some("src/App.vue"), Some(42)));
        assert!(
            with_line.ends_with("R2\tセレクタが複数一致する src/App.vue:42"),
            "{with_line}"
        );

        let file_only = format_finding(&finding(Some("src/App.vue"), None));
        assert!(file_only.ends_with("src/App.vue"), "{file_only}");

        let no_location = format_finding(&finding(None, Some(42)));
        assert!(
            no_location.ends_with("セレクタが複数一致する"),
            "{no_location}"
        );
        assert!(no_location.contains("MEDIUM"), "{no_location}");
    }

    #[test]
    fn rejects_an_unknown_state_for_resolve() {
        let err = parse_state("done").unwrap_err();
        assert!(
            err.message.starts_with("unknown state: done"),
            "{}",
            err.message
        );
        assert_eq!(err.exit_code, 2);
        assert_eq!(parse_state("deferred").unwrap(), FindingState::Deferred);
    }

    #[test]
    fn spells_every_severity_and_state_the_way_the_api_does() {
        // 綴りの一覧は entity から取る。CLI 側に写しを作ると、変種が増えたときに黙ってずれる
        assert_eq!(
            join(&FindingSeverity::iter().collect::<Vec<_>>()),
            "high, medium, low, nit"
        );
        assert_eq!(
            join(&FindingState::iter().collect::<Vec<_>>()),
            "open, fixed, verified, deferred, rejected"
        );
    }
}
