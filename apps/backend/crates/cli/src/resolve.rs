//! キー・名前・参照から API が要る UUID を引く。

use payload::projects::ProjectResponse;
use payload::statuses::ProjectStatusResponse;
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

fn pick_status_by_name(statuses: &[ProjectStatusResponse], name: &str) -> Result<Uuid> {
    statuses
        .iter()
        .find(|status| status.name.eq_ignore_ascii_case(name))
        .map(|status| status.id)
        .ok_or_else(|| {
            // ステータスはプロジェクトごとに違うので、綴りを外したときに何が使えるか
            // 分からないと詰まる。解決のために一覧はすでに取ってあるので、そのまま添える
            CliError::not_found(format!(
                "Status not found: {name} (this project has {})",
                status_names(statuses)
            ))
        })
}

/// エラーに添えるステータス名。並び順は画面と揃える（position 昇順）。
fn status_names(statuses: &[ProjectStatusResponse]) -> String {
    if statuses.is_empty() {
        return "none".into();
    }
    let mut sorted: Vec<&ProjectStatusResponse> = statuses.iter().collect();
    sorted.sort_by_key(|status| status.position);
    sorted
        .iter()
        .map(|status| status.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
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

    /// 綴りを外したとき、そのプロジェクトで何が使えるかが分からないと詰まる。
    /// 解決のために一覧はすでに取ってあるので、エラーに添える。
    #[test]
    fn lists_the_available_statuses_when_the_name_does_not_match() {
        let statuses = vec![
            status("Todo", false, true, 0),
            status("In Progress", false, false, 1),
            status("Done", true, false, 2),
        ];

        let err = pick_status_by_name(&statuses, "Reviewing").unwrap_err();

        assert!(err.message.contains("Reviewing"), "{}", err.message);
        assert!(
            err.message.contains("Todo, In Progress, Done"),
            "並び順のまま候補を出す: {}",
            err.message
        );
    }

    #[test]
    fn orders_the_listed_statuses_by_position_not_by_input_order() {
        let statuses = vec![
            status("Done", true, false, 2),
            status("Todo", false, true, 0),
            status("In Progress", false, false, 1),
        ];

        let err = pick_status_by_name(&statuses, "nope").unwrap_err();

        assert!(
            err.message.contains("Todo, In Progress, Done"),
            "{}",
            err.message
        );
    }

    #[test]
    fn says_none_when_the_project_has_no_statuses() {
        let err = pick_status_by_name(&[], "Todo").unwrap_err();

        assert!(err.message.contains("none"), "{}", err.message);
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
}
