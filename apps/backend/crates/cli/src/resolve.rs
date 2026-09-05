//! キー・名前・参照から API が要る UUID を引く。

use payload::labels::LabelResponse;
use payload::milestones::MilestoneResponse;
use payload::projects::ProjectResponse;
use payload::sprints::SprintResponse;
use payload::statuses::ProjectStatusResponse;
use payload::users::UserSummary;
use uuid::Uuid;

use crate::api::ApiClient;
use crate::error::{CliError, Result};

pub fn is_uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok()
}

/// タスクの指し方。`KEY-42` は API がそのまま解決できるので文字列のまま渡す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskRef {
    Uuid(Uuid),
    Seq {
        project_key: String,
        task_id: String,
    },
}

pub fn parse_task_ref(value: &str) -> Result<TaskRef> {
    if let Ok(uuid) = Uuid::parse_str(value) {
        return Ok(TaskRef::Uuid(uuid));
    }
    let Some(dash) = value.rfind('-').filter(|index| *index > 0) else {
        return Err(CliError::validation(format!(
            "Invalid task reference: {value}"
        )));
    };
    let (project_key, seq) = value.split_at(dash);
    let seq = &seq[1..];
    if seq.is_empty() || !seq.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(CliError::validation(format!(
            "Invalid task reference: {value}"
        )));
    }
    Ok(TaskRef::Seq {
        project_key: project_key.to_string(),
        task_id: value.to_string(),
    })
}

pub async fn list_projects(api: &ApiClient) -> Result<Vec<ProjectResponse>> {
    api.get(&["v1", "tenants", api.tenant_id(), "projects"], &[])
        .await
}

pub async fn resolve_project(api: &ApiClient, key_or_id: &str) -> Result<ProjectResponse> {
    if is_uuid(key_or_id) {
        return api
            .get(
                &["v1", "tenants", api.tenant_id(), "projects", key_or_id],
                &[],
            )
            .await;
    }
    list_projects(api)
        .await?
        .into_iter()
        .find(|project| project.key.eq_ignore_ascii_case(key_or_id))
        .ok_or_else(|| CliError::not_found(format!("Project not found: {key_or_id}")))
}

pub async fn list_statuses(
    api: &ApiClient,
    project_id: Uuid,
) -> Result<Vec<ProjectStatusResponse>> {
    let project_id = project_id.to_string();
    api.get(
        &[
            "v1",
            "tenants",
            api.tenant_id(),
            "projects",
            &project_id,
            "statuses",
        ],
        &[],
    )
    .await
}

pub async fn resolve_status_id(api: &ApiClient, project_id: Uuid, name: &str) -> Result<Uuid> {
    pick_status_by_name(&list_statuses(api, project_id).await?, name)
}

pub async fn find_done_status_id(api: &ApiClient, project_id: Uuid) -> Result<Uuid> {
    pick_done_status(&list_statuses(api, project_id).await?)
}

/// `--status` を省いたときに使う状態。
///
/// 作成 API は `status_id` を必須で受けるので、省略時に送らないと必ず 400 になる。
/// 既定に印のある状態、無ければ並び順の先頭を使う。
pub async fn default_status_id(api: &ApiClient, project_id: Uuid) -> Result<Uuid> {
    pick_default_status(&list_statuses(api, project_id).await?)
}

/// プロジェクト配下の一覧を引く（`labels` / `milestones` / `sprints`）。
async fn list_under_project<T: serde::de::DeserializeOwned>(
    api: &ApiClient,
    project_id: Uuid,
    collection: &str,
) -> Result<Vec<T>> {
    let project_id = project_id.to_string();
    api.get(
        &[
            "v1",
            "tenants",
            api.tenant_id(),
            "projects",
            &project_id,
            collection,
        ],
        &[],
    )
    .await
}

/// 名前で引けなかったときのエラー。
///
/// ラベルもマイルストーンもプロジェクトごとに違うので、綴りを外したときに何が使えるか
/// 分からないと詰まる。解決のために一覧はすでに取ってあるので、そのまま添える。
fn not_found_with_candidates(kind: &str, name: &str, candidates: Vec<String>) -> CliError {
    let listed = if candidates.is_empty() {
        "none".to_string()
    } else {
        candidates.join(", ")
    };
    CliError::not_found(format!(
        "{kind} not found: {name} (this project has {listed})"
    ))
}

pub async fn list_labels(api: &ApiClient, project_id: Uuid) -> Result<Vec<LabelResponse>> {
    list_under_project(api, project_id, "labels").await
}

pub async fn resolve_label_id(api: &ApiClient, project_id: Uuid, name: &str) -> Result<Uuid> {
    let labels = list_labels(api, project_id).await?;
    labels
        .iter()
        .find(|label| label.name.eq_ignore_ascii_case(name))
        .map(|label| label.id)
        .ok_or_else(|| {
            not_found_with_candidates(
                "Label",
                name,
                labels.iter().map(|label| label.name.clone()).collect(),
            )
        })
}

pub async fn list_milestones(api: &ApiClient, project_id: Uuid) -> Result<Vec<MilestoneResponse>> {
    list_under_project(api, project_id, "milestones").await
}

pub async fn resolve_milestone_id(api: &ApiClient, project_id: Uuid, name: &str) -> Result<Uuid> {
    if let Ok(uuid) = Uuid::parse_str(name) {
        return Ok(uuid);
    }
    let milestones = list_milestones(api, project_id).await?;
    milestones
        .iter()
        .find(|milestone| milestone.name.eq_ignore_ascii_case(name))
        .map(|milestone| milestone.id)
        .ok_or_else(|| {
            not_found_with_candidates(
                "Milestone",
                name,
                milestones
                    .iter()
                    .map(|milestone| milestone.name.clone())
                    .collect(),
            )
        })
}

pub async fn list_sprints(api: &ApiClient, project_id: Uuid) -> Result<Vec<SprintResponse>> {
    list_under_project(api, project_id, "sprints").await
}

pub async fn resolve_sprint_id(api: &ApiClient, project_id: Uuid, name: &str) -> Result<Uuid> {
    if let Ok(uuid) = Uuid::parse_str(name) {
        return Ok(uuid);
    }
    let sprints = list_sprints(api, project_id).await?;
    sprints
        .iter()
        .find(|sprint| sprint.name.eq_ignore_ascii_case(name))
        .map(|sprint| sprint.id)
        .ok_or_else(|| {
            not_found_with_candidates(
                "Sprint",
                name,
                sprints.iter().map(|sprint| sprint.name.clone()).collect(),
            )
        })
}

pub async fn list_assignable_users(api: &ApiClient, project_id: Uuid) -> Result<Vec<UserSummary>> {
    list_under_project(api, project_id, "assignable-users").await
}

/// 担当者はユーザー名で指す。UUID をそのまま渡す道も残す。
pub async fn resolve_user_id(api: &ApiClient, project_id: Uuid, name: &str) -> Result<Uuid> {
    if let Ok(uuid) = Uuid::parse_str(name) {
        return Ok(uuid);
    }
    let users = list_assignable_users(api, project_id).await?;
    users
        .iter()
        .find(|user| user.username.eq_ignore_ascii_case(name))
        .map(|user| user.id)
        .ok_or_else(|| {
            not_found_with_candidates(
                "Assignable user",
                name,
                users.iter().map(|user| user.username.clone()).collect(),
            )
        })
}

fn pick_status_by_name(statuses: &[ProjectStatusResponse], name: &str) -> Result<Uuid> {
    statuses
        .iter()
        .find(|status| status.name.eq_ignore_ascii_case(name))
        .map(|status| status.id)
        .ok_or_else(|| CliError::not_found(format!("Status not found: {name}")))
}

fn pick_done_status(statuses: &[ProjectStatusResponse]) -> Result<Uuid> {
    statuses
        .iter()
        .find(|status| status.is_done_state)
        .or_else(|| {
            statuses.iter().find(|status| {
                let name = status.name.to_ascii_lowercase();
                name.contains("done") || name.contains("complete")
            })
        })
        .map(|status| status.id)
        .ok_or_else(|| CliError::not_found("No done status found for project"))
}

fn pick_default_status(statuses: &[ProjectStatusResponse]) -> Result<Uuid> {
    statuses
        .iter()
        .find(|status| status.is_default)
        .or_else(|| statuses.iter().min_by_key(|status| status.position))
        .map(|status| status.id)
        .ok_or_else(|| CliError::not_found("This project has no statuses to create a task in"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn status(
        name: &str,
        is_done_state: bool,
        is_default: bool,
        position: i16,
    ) -> ProjectStatusResponse {
        ProjectStatusResponse {
            // 並びの検証をするので、id は位置から決まる固定値にする
            id: Uuid::from_u128(position as u128 + 1),
            project_id: Uuid::nil(),
            name: name.into(),
            color: "#000000".into(),
            position,
            is_default,
            is_done_state,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn reads_uuid_and_key_number_task_references() {
        let uuid = Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap();
        assert!(is_uuid("00000000-0000-4000-8000-000000000001"));
        assert_eq!(
            parse_task_ref("00000000-0000-4000-8000-000000000001").unwrap(),
            TaskRef::Uuid(uuid)
        );
        assert_eq!(
            parse_task_ref("TEAM-42").unwrap(),
            TaskRef::Seq {
                project_key: "TEAM".into(),
                task_id: "TEAM-42".into()
            }
        );
    }

    #[test]
    fn keeps_a_multi_dash_project_key_whole() {
        assert_eq!(
            parse_task_ref("TEAM-CORE-7").unwrap(),
            TaskRef::Seq {
                project_key: "TEAM-CORE".into(),
                task_id: "TEAM-CORE-7".into()
            }
        );
    }

    #[test]
    fn rejects_references_without_a_numeric_sequence() {
        for value in ["TEAM-nope", "TEAM-", "-7", "TEAM", "TEAM-4a"] {
            let err = parse_task_ref(value).unwrap_err();
            assert!(
                err.message.contains("Invalid task reference"),
                "{value}: {}",
                err.message
            );
            assert_eq!(err.exit_code, 2, "{value}");
        }
    }

    #[test]
    fn matches_a_status_name_regardless_of_case() {
        let statuses = vec![
            status("Todo", false, true, 0),
            status("Doing", false, false, 1),
        ];
        assert_eq!(
            pick_status_by_name(&statuses, "doing").unwrap(),
            statuses[1].id
        );
        assert_eq!(
            pick_status_by_name(&statuses, "nope")
                .unwrap_err()
                .exit_code,
            5
        );
    }

    #[test]
    fn prefers_the_declared_done_state_over_a_name_that_merely_looks_done() {
        let statuses = vec![
            status("Completed archive", false, false, 0),
            status("Shipped", true, false, 1),
        ];
        assert_eq!(pick_done_status(&statuses).unwrap(), statuses[1].id);
    }

    #[test]
    fn falls_back_to_a_done_looking_name_when_no_state_is_marked() {
        let statuses = vec![
            status("Todo", false, true, 0),
            status("Complete", false, false, 1),
        ];
        assert_eq!(pick_done_status(&statuses).unwrap(), statuses[1].id);
    }

    #[test]
    fn reports_a_project_with_no_done_state_instead_of_guessing() {
        let statuses = vec![
            status("Todo", false, true, 0),
            status("Doing", false, false, 1),
        ];
        assert_eq!(pick_done_status(&statuses).unwrap_err().exit_code, 5);
    }

    #[test]
    fn picks_the_marked_default_status_then_the_lowest_position() {
        let marked = vec![
            status("Todo", false, false, 5),
            status("Inbox", false, true, 9),
        ];
        assert_eq!(pick_default_status(&marked).unwrap(), marked[1].id);

        let unmarked = vec![
            status("Doing", false, false, 3),
            status("Todo", false, false, 1),
        ];
        assert_eq!(pick_default_status(&unmarked).unwrap(), unmarked[1].id);

        assert!(pick_default_status(&[]).is_err());
    }

    /// ラベルやマイルストーンもプロジェクトごとに違う。綴りを外したとき、
    /// 何が使えるか出さないと、名前を当てるまで総当たりになる。
    #[test]
    fn lists_the_candidates_when_a_name_does_not_match() {
        let err =
            not_found_with_candidates("Label", "buf", vec!["bug".into(), "enhancement".into()]);

        assert!(
            err.message.contains("Label not found: buf"),
            "{}",
            err.message
        );
        assert!(err.message.contains("bug, enhancement"), "{}", err.message);
        assert_eq!(err.exit_code, 5);
    }

    #[test]
    fn says_none_when_there_is_nothing_to_suggest() {
        let err = not_found_with_candidates("Sprint", "week-1", vec![]);

        assert!(err.message.contains("none"), "{}", err.message);
    }
}
